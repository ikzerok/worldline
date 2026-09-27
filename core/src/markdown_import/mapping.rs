use super::inline::{resolve_relative_href, Href};
use super::source::output_path_exists;
use super::{
    apply, digest_plan, hash_bytes, hash_strings, loss, markdown, source, source_fingerprint,
    valid_id, MarkdownAttachmentMapping, MarkdownImportConflict, MarkdownImportOptions,
    MarkdownImportPlan, MarkdownLinkMapping, MarkdownNameConflict, MarkdownPageMapping,
    ParsedMarkdownSource, PlanDigest, PreparedPreview, Project, ResolvedAttachment,
    MAX_ATTACHMENT_BYTES_TOTAL, MAX_IMPORT_REFERENCES, MAX_NAMESPACE_BYTES,
};
use crate::catalog::TargetRef;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

pub(super) fn prepare_preview_from_pages(
    project: &Project,
    options: &MarkdownImportOptions,
    source: ParsedMarkdownSource<'_>,
) -> Result<PreparedPreview, String> {
    let ParsedMarkdownSource {
        source_root,
        inputs,
        markdown,
        mut pages,
    } = source;
    let baseline = options.expected_baseline.clone();
    let current = project.compile_current();
    let namespace = match options.namespace.as_deref() {
        Some(value) if valid_id(value) && value.len() <= MAX_NAMESPACE_BYTES => value.to_string(),
        Some(_) => {
            return Err(format!(
                "迁移命名空间必须是长度不超过 {MAX_NAMESPACE_BYTES} 字节的 ASCII 标识符"
            ))
        }
        None => format!(
            "md_{:016x}",
            hash_strings(markdown.iter().map(|file| file.relative.as_str()))
        ),
    };
    let mut conflicts = Vec::new();
    let mut name_conflicts = Vec::new();
    markdown::resolve_page_ids(&current, &mut pages, options, &mut conflicts);

    let mut title_sources: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for page in &pages {
        title_sources
            .entry(page.title.clone())
            .or_default()
            .push(page.relative.clone());
    }
    for (title, sources) in title_sources {
        if sources.len() > 1 {
            name_conflicts.push(MarkdownNameConflict {
                title,
                sources,
                existing_targets: Vec::new(),
                message: "多个来源页面同名；它们会保留为不同的稳定 ID，不自动合并".into(),
            });
        }
    }
    let mut existing_titles: BTreeMap<String, Vec<TargetRef>> = BTreeMap::new();
    for object in &current.analysis.catalog.objects {
        existing_titles
            .entry(object.display.clone())
            .or_default()
            .push(object.target.clone());
    }
    for conflict in &mut name_conflicts {
        conflict.existing_targets = existing_titles.remove(&conflict.title).unwrap_or_default();
    }
    for page in &pages {
        if let Some(existing) = existing_titles.get(&page.title) {
            name_conflicts.push(MarkdownNameConflict {
                title: page.title.clone(),
                sources: vec![page.relative.clone()],
                existing_targets: existing.clone(),
                message: "工程中存在同名资料；导入将保留为独立 ID，不自动合并".into(),
            });
        }
    }

    let output_root = format!(".world/markdown-imports/{namespace}");
    if output_path_exists(project, &output_root)? {
        conflicts.push(MarkdownImportConflict {
            code: "OUTPUT_PATH_EXISTS".into(),
            source: None,
            preferred_id: Some(namespace.clone()),
            candidates: Vec::new(),
            message: format!("迁移命名空间已存在：{output_root}；请显式选择新的 namespace"),
        });
    }

    let mut heading_targets: BTreeMap<(String, String), Vec<TargetRef>> = BTreeMap::new();
    let mut generated_anchor_ids = BTreeSet::new();
    for page in &mut pages {
        let mut seen = BTreeMap::<String, usize>::new();
        for heading in &mut page.headings {
            let count = seen.entry(heading.slug.clone()).or_default();
            *count += 1;
            heading.id = format!(
                "mdh_{:016x}",
                hash_bytes(format!("{}\0{}\0{}", page.id, heading.slug, count).as_bytes())
            );
            if current.analysis.catalog.anchors.contains_key(&heading.id)
                || !generated_anchor_ids.insert(heading.id.clone())
            {
                conflicts.push(MarkdownImportConflict {
                    code: "ANCHOR_ID_CONFLICT".into(),
                    source: Some(page.relative.clone()),
                    preferred_id: Some(heading.id.clone()),
                    candidates: Vec::new(),
                    message: format!("Markdown 标题 anchor ID 已被占用：{}", heading.id),
                });
            }
            heading_targets
                .entry((page.relative.clone(), heading.slug.clone()))
                .or_default()
                .push(TargetRef::new("anchor", &heading.id));
        }
    }
    let page_by_path: BTreeMap<_, _> = pages
        .iter()
        .map(|page| (page.relative.clone(), page.id.clone()))
        .collect();

    let mut links = Vec::new();
    let relation_type_id = format!("markdown_link_{namespace}");
    if !pages.iter().all(|page| page.links.is_empty())
        && current
            .analysis
            .catalog
            .relation_types
            .contains_key(&relation_type_id)
    {
        conflicts.push(MarkdownImportConflict {
            code: "RELATION_TYPE_ID_CONFLICT".into(),
            source: None,
            preferred_id: Some(relation_type_id.clone()),
            candidates: Vec::new(),
            message: format!("Markdown 链接关系类型 ID 已被占用：{relation_type_id}"),
        });
    }
    let mut attachments = Vec::new();
    let mut resolved_attachments = Vec::new();
    let mut losses: Vec<_> = pages
        .iter()
        .flat_map(|page| page.losses.iter().cloned())
        .collect();
    let mut references = 0usize;
    let input_by_path: BTreeMap<_, _> = inputs
        .iter()
        .map(|file| (file.relative.as_str(), file.clone()))
        .collect();
    let mut attachment_total = 0usize;
    let mut attachment_contents = BTreeMap::<String, Arc<Vec<u8>>>::new();
    let mut generated_relation_ids = BTreeSet::new();

    for page in &pages {
        for (link_index, reference) in page.links.iter().enumerate() {
            references = references.saturating_add(1);
            if references > MAX_IMPORT_REFERENCES {
                return Err("Markdown 页面链接与附件引用超过单次迁移预算".into());
            }
            let target = match resolve_relative_href(&page.relative, &reference.href) {
                Href::Local { path, fragment } => {
                    let Some(target_page_id) = page_by_path.get(&path) else {
                        losses.push(loss(
                            "BROKEN_MARKDOWN_LINK",
                            &page.relative,
                            reference.line,
                            format!("页面链接目标不存在：{}", reference.href),
                            Some(format!("{output_root}/sources/{}", page.relative)),
                        ));
                        continue;
                    };
                    if let Some(fragment) = fragment {
                        let targets = heading_targets.get(&(path.clone(), fragment.clone()));
                        match targets {
                            Some(targets) if targets.len() == 1 => targets[0].clone(),
                            Some(_) => {
                                losses.push(loss(
                                    "AMBIGUOUS_MARKDOWN_FRAGMENT",
                                    &page.relative,
                                    reference.line,
                                    format!("標题片段重复，无法确定链接目标：{}", reference.href),
                                    Some(format!("{output_root}/sources/{}", page.relative)),
                                ));
                                continue;
                            }
                            None => {
                                losses.push(loss(
                                    "BROKEN_MARKDOWN_FRAGMENT",
                                    &page.relative,
                                    reference.line,
                                    format!("标题片段不存在：{}", reference.href),
                                    Some(format!("{output_root}/sources/{}", page.relative)),
                                ));
                                continue;
                            }
                        }
                    } else {
                        TargetRef::new("entity", target_page_id)
                    }
                }
                Href::External => {
                    losses.push(loss(
                        "REMOTE_RESOURCE_ISOLATED",
                        &page.relative,
                        reference.line,
                        format!(
                            "远程链接保持在原文副本中，不会下载或执行：{}",
                            reference.href
                        ),
                        Some(format!("{output_root}/sources/{}", page.relative)),
                    ));
                    continue;
                }
                Href::Unsafe => {
                    losses.push(loss(
                        "UNSAFE_LINK_ISOLATED",
                        &page.relative,
                        reference.line,
                        format!("绝对或越界链接不会读取：{}", reference.href),
                        Some(format!("{output_root}/sources/{}", page.relative)),
                    ));
                    continue;
                }
                Href::Malformed => {
                    losses.push(loss(
                        "UNSUPPORTED_LINK_SYNTAX",
                        &page.relative,
                        reference.line,
                        format!("链接格式无法无歧义映射：{}", reference.href),
                        Some(format!("{output_root}/sources/{}", page.relative)),
                    ));
                    continue;
                }
            };
            let relation_id = format!(
                "mdr_{:016x}",
                hash_bytes(
                    format!(
                        "{}\0{}\0{}\0{}\0{}",
                        namespace, page.relative, reference.line, reference.href, link_index
                    )
                    .as_bytes()
                )
            );
            if current
                .analysis
                .catalog
                .relations
                .contains_key(&relation_id)
                || !generated_relation_ids.insert(relation_id.clone())
            {
                conflicts.push(MarkdownImportConflict {
                    code: "RELATION_ID_CONFLICT".into(),
                    source: Some(page.relative.clone()),
                    preferred_id: Some(relation_id),
                    candidates: Vec::new(),
                    message: format!("Markdown 链接关系 ID 已被占用：{}", reference.href),
                });
                continue;
            }
            links.push(MarkdownLinkMapping {
                source: page.relative.clone(),
                line: reference.line,
                href: reference.href.clone(),
                label: reference.label.clone(),
                target,
                relation_id,
            });
        }
        for reference in &page.attachments {
            references = references.saturating_add(1);
            if references > MAX_IMPORT_REFERENCES {
                return Err("Markdown 页面链接与附件引用超过单次迁移预算".into());
            }
            match resolve_relative_href(&page.relative, &reference.href) {
                Href::Local {
                    path,
                    fragment: None,
                } => {
                    let Some(input) = input_by_path.get(path.as_str()).cloned() else {
                        losses.push(loss(
                            "MISSING_ATTACHMENT",
                            &page.relative,
                            reference.line,
                            format!("本地附件不存在：{}", reference.href),
                            Some(format!("{output_root}/sources/{}", page.relative)),
                        ));
                        continue;
                    };
                    let bytes = if let Some(bytes) = attachment_contents.get(&input.relative) {
                        bytes.clone()
                    } else {
                        let bytes = Arc::new(source::read_input_file(&input)?);
                        if bytes.len() > MAX_ATTACHMENT_BYTES_TOTAL {
                            return Err(format!("附件超过单次预算：{}", input.relative));
                        }
                        attachment_total = attachment_total
                            .checked_add(bytes.len())
                            .ok_or("附件大小溢出")?;
                        if attachment_total > MAX_ATTACHMENT_BYTES_TOTAL {
                            return Err("Markdown 附件超过单次迁移预算".into());
                        }
                        attachment_contents.insert(input.relative.clone(), bytes.clone());
                        bytes
                    };
                    let extension = apply::safe_extension(&input.relative);
                    let id = format!(
                        "mda_{:016x}",
                        hash_bytes(format!("{}\0{}", namespace, input.relative).as_bytes())
                    );
                    let quarantined = matches!(
                        extension.as_str(),
                        "js" | "mjs"
                            | "cjs"
                            | "jsx"
                            | "ts"
                            | "tsx"
                            | "py"
                            | "rb"
                            | "pl"
                            | "lua"
                            | "php"
                            | "sh"
                            | "bash"
                            | "zsh"
                            | "fish"
                            | "ps1"
                            | "psm1"
                            | "vbs"
                            | "bat"
                            | "cmd"
                            | "exe"
                            | "com"
                            | "dll"
                            | "wasm"
                            | "jar"
                            | "class"
                            | "html"
                            | "htm"
                            | "xhtml"
                            | "svg"
                            | "docm"
                            | "xlsm"
                            | "pptm"
                    );
                    let category = if quarantined { "quarantine" } else { "assets" };
                    let output_path = format!("{output_root}/{category}/{id}.{extension}");
                    if !quarantined && current.analysis.catalog.assets.contains_key(&id) {
                        conflicts.push(MarkdownImportConflict {
                            code: "ATTACHMENT_PATH_CONFLICT".into(),
                            source: Some(page.relative.clone()),
                            preferred_id: Some(id.clone()),
                            candidates: Vec::new(),
                            message: format!("附件目标已存在：{output_path}"),
                        });
                        continue;
                    }
                    if quarantined {
                        losses.push(loss(
                            "SCRIPT_ATTACHMENT_QUARANTINED",
                            &page.relative,
                            reference.line,
                            format!(
                                "可执行、脚本、HTML 或宏附件只复制到隔离目录，不登记为活动素材：{}",
                                reference.href
                            ),
                            Some(output_path.clone()),
                        ));
                    } else {
                        attachments.push(MarkdownAttachmentMapping {
                            source: page.relative.clone(),
                            line: reference.line,
                            href: reference.href.clone(),
                            id: id.clone(),
                            output_path: output_path.clone(),
                            alt: reference.label.clone(),
                        });
                    }
                    resolved_attachments.push(ResolvedAttachment {
                        source_page: page.relative.clone(),
                        reference: reference.clone(),
                        input,
                        bytes,
                        id,
                        output_path,
                        quarantined,
                    });
                }
                Href::External => losses.push(loss(
                    "REMOTE_ATTACHMENT_ISOLATED",
                    &page.relative,
                    reference.line,
                    format!("远程附件不会下载：{}", reference.href),
                    Some(format!("{output_root}/sources/{}", page.relative)),
                )),
                Href::Unsafe => losses.push(loss(
                    "UNSAFE_ATTACHMENT_ISOLATED",
                    &page.relative,
                    reference.line,
                    format!("绝对或越界附件路径不会读取：{}", reference.href),
                    Some(format!("{output_root}/sources/{}", page.relative)),
                )),
                Href::Malformed
                | Href::Local {
                    fragment: Some(_), ..
                } => losses.push(loss(
                    "UNSUPPORTED_ATTACHMENT_LINK",
                    &page.relative,
                    reference.line,
                    format!("附件链接不能无歧义映射：{}", reference.href),
                    Some(format!("{output_root}/sources/{}", page.relative)),
                )),
            }
        }
    }
    let referenced_paths: BTreeSet<_> = resolved_attachments
        .iter()
        .map(|attachment| attachment.input.relative.as_str())
        .collect();
    for input in &inputs {
        let is_markdown = Path::new(&input.relative)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
        if !is_markdown && !referenced_paths.contains(input.relative.as_str()) {
            losses.push(loss(
                "UNREFERENCED_SOURCE_FILE",
                &input.relative,
                0,
                "文件没有被 Markdown 页面引用，因此不会复制到工程；来源目录保持原样".into(),
                None,
            ));
        }
    }
    let page_sources: BTreeSet<_> = pages.iter().map(|page| page.relative.as_str()).collect();
    for item in &mut losses {
        if item.preserved_at.is_none() && page_sources.contains(item.source.as_str()) {
            item.preserved_at = Some(format!("{output_root}/sources/{}", item.source));
        }
    }
    losses.sort_by(|left, right| {
        (&left.source, left.line, &left.code).cmp(&(&right.source, right.line, &right.code))
    });
    conflicts.sort_by(|left, right| {
        (&left.code, &left.source, &left.preferred_id).cmp(&(
            &right.code,
            &right.source,
            &right.preferred_id,
        ))
    });

    let current_version = current.options.language_version;
    let requires_language_upgrade = current_version != crate::LanguageVersion::V1_10;
    let pages_view: Vec<MarkdownPageMapping> = pages
        .iter()
        .map(|page| MarkdownPageMapping {
            source: page.relative.clone(),
            id: page.id.clone(),
            title: page.title.clone(),
            entity_type: page.entity_type.clone(),
            target: TargetRef::new("entity", &page.id),
        })
        .collect();
    let source_fingerprint = source_fingerprint(&inputs, &pages, &resolved_attachments)?;
    let (candidate, additional_files, files, candidate_conflicts) = apply::build_candidate(
        project,
        &current,
        &pages,
        &links,
        &resolved_attachments,
        &namespace,
        requires_language_upgrade,
    )?;
    conflicts.extend(candidate_conflicts);
    conflicts.sort_by(|left, right| {
        (&left.code, &left.source, &left.preferred_id).cmp(&(
            &right.code,
            &right.source,
            &right.preferred_id,
        ))
    });
    let candidate_new_baseline = candidate.content_baseline();
    let plan_digest = digest_plan(PlanDigest {
        source_fingerprint: &source_fingerprint,
        baseline: &baseline,
        namespace: &namespace,
        pages: &pages_view,
        links: &links,
        attachments: &attachments,
        losses: &losses,
        conflicts: &conflicts,
        name_conflicts: &name_conflicts,
        files: &files,
        requires_language_upgrade,
    })?;
    let can_apply = conflicts.is_empty()
        && (losses.is_empty() || options.accept_losses)
        && (!requires_language_upgrade || options.allow_language_upgrade);
    let plan = MarkdownImportPlan {
        source_root,
        source_fingerprint,
        plan_digest,
        baseline: baseline.clone(),
        new_baseline: candidate_new_baseline,
        namespace,
        pages: pages_view,
        links,
        attachments,
        losses,
        conflicts,
        name_conflicts,
        files,
        requires_language_upgrade,
        can_apply,
    };
    Ok(PreparedPreview {
        plan,
        candidate,
        additional_files,
    })
}

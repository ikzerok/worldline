//! 外部 Markdown 的安全预检和候选映射。
//!
//! 此模块只处理显式调用方选择的目录，不参与普通工作区扫描。

use crate::catalog::TargetRef;
use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const MAX_IMPORT_FILES: usize = 512;
pub const MAX_MARKDOWN_BYTES_PER_PAGE: usize = 1024 * 1024;
pub const MAX_MARKDOWN_BYTES_TOTAL: usize = 16 * 1024 * 1024;
pub const MAX_ATTACHMENT_BYTES_TOTAL: usize = 64 * 1024 * 1024;
pub const MAX_IMPORT_REFERENCES: usize = 4096;
const MAX_IMPORT_ENTRIES: usize = MAX_IMPORT_FILES * 4;
const MAX_NAMESPACE_BYTES: usize = 64;
const MAX_IMPORT_PATH_BYTES: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkdownImportRequest {
    pub source_root: PathBuf,
    pub expected_baseline: String,
    #[serde(default)]
    pub id_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub accept_losses: bool,
    #[serde(default)]
    pub allow_language_upgrade: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownPageMapping {
    pub source: String,
    pub id: String,
    pub title: String,
    pub entity_type: String,
    pub target: TargetRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownLinkMapping {
    pub source: String,
    pub line: u32,
    pub href: String,
    pub label: String,
    pub target: TargetRef,
    pub relation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownAttachmentMapping {
    pub source: String,
    pub line: u32,
    pub href: String,
    pub id: String,
    pub output_path: String,
    pub alt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownImportLoss {
    pub code: String,
    pub source: String,
    pub line: u32,
    pub message: String,
    pub preserved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownImportConflict {
    pub code: String,
    pub source: Option<String>,
    pub preferred_id: Option<String>,
    pub candidates: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownNameConflict {
    pub title: String,
    pub sources: Vec<String>,
    pub existing_targets: Vec<TargetRef>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownImportFilePreview {
    pub path: String,
    pub kind: String,
    pub source: Option<String>,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownImportPlan {
    pub source_root: PathBuf,
    pub source_fingerprint: String,
    pub plan_digest: String,
    pub baseline: String,
    pub new_baseline: String,
    pub namespace: String,
    pub pages: Vec<MarkdownPageMapping>,
    pub links: Vec<MarkdownLinkMapping>,
    pub attachments: Vec<MarkdownAttachmentMapping>,
    pub losses: Vec<MarkdownImportLoss>,
    pub conflicts: Vec<MarkdownImportConflict>,
    pub name_conflicts: Vec<MarkdownNameConflict>,
    pub files: Vec<MarkdownImportFilePreview>,
    pub requires_language_upgrade: bool,
    pub can_apply: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownImportResult {
    pub plan: MarkdownImportPlan,
    pub changed_files: Vec<PathBuf>,
    pub baseline: String,
    pub new_baseline: String,
}

#[derive(Debug, Clone)]
struct InputFile {
    relative: String,
    absolute: PathBuf,
    length: u64,
}

#[derive(Debug, Clone)]
struct SourcePage {
    relative: String,
    bytes: Vec<u8>,
    id: String,
    invalid_front_matter_id: bool,
    title: String,
    entity_type: String,
    description: String,
    headings: Vec<Heading>,
    links: Vec<InlineReference>,
    attachments: Vec<InlineReference>,
    losses: Vec<MarkdownImportLoss>,
}

#[derive(Debug, Clone)]
struct Heading {
    slug: String,
    id: String,
    title: String,
    line: u32,
}

#[derive(Debug, Clone)]
struct InlineReference {
    href: String,
    label: String,
    line: u32,
}

#[derive(Debug, Clone)]
struct ResolvedAttachment {
    source_page: String,
    reference: InlineReference,
    input: InputFile,
    bytes: Arc<Vec<u8>>,
    id: String,
    output_path: String,
    quarantined: bool,
}

struct PreparedPreview {
    plan: MarkdownImportPlan,
    candidate: Project,
    additional_files: Vec<(PathBuf, Vec<u8>)>,
}

type CandidateBuild = (
    Project,
    Vec<(PathBuf, Vec<u8>)>,
    Vec<MarkdownImportFilePreview>,
    Vec<MarkdownImportConflict>,
);

#[derive(Serialize)]
struct PlanDigest<'a> {
    source_fingerprint: &'a str,
    baseline: &'a str,
    namespace: &'a str,
    pages: &'a [MarkdownPageMapping],
    links: &'a [MarkdownLinkMapping],
    attachments: &'a [MarkdownAttachmentMapping],
    losses: &'a [MarkdownImportLoss],
    conflicts: &'a [MarkdownImportConflict],
    name_conflicts: &'a [MarkdownNameConflict],
    files: &'a [MarkdownImportFilePreview],
    requires_language_upgrade: bool,
}

impl Project {
    /// 对显式选择的 Markdown 目录构建只读迁移预览。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn preview_markdown_import(
        &self,
        request: &MarkdownImportRequest,
    ) -> Result<MarkdownImportPlan, String> {
        prepare_preview(self, request).map(|prepared| prepared.plan)
    }

    /// 重新验证预览后，在一个 Project 候选和可恢复保存事务中应用迁移。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn apply_markdown_import(
        &mut self,
        request: &MarkdownImportRequest,
        plan_digest: &str,
    ) -> Result<MarkdownImportResult, String> {
        let mut prepared = prepare_preview(self, request)?;
        if prepared.plan.plan_digest != plan_digest {
            return Err("Markdown 迁移预览已过期；来源、映射或工程基线已变化".into());
        }
        if !prepared.plan.can_apply {
            return Err("Markdown 迁移仍有未解决的冲突，或缺少损失/语言升级确认".into());
        }
        let changed_files = prepared
            .plan
            .files
            .iter()
            .map(|file| self.root.join(&file.path))
            .chain(std::iter::once(self.entry.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let baseline = self.content_baseline();
        prepared
            .candidate
            .save_with_additional_files(&prepared.additional_files)?;
        let new_baseline = prepared.candidate.content_baseline();
        prepared.plan.new_baseline = new_baseline.clone();
        *self = prepared.candidate;
        Ok(MarkdownImportResult {
            plan: prepared.plan,
            changed_files,
            baseline,
            new_baseline,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn prepare_preview(
    project: &Project,
    request: &MarkdownImportRequest,
) -> Result<PreparedPreview, String> {
    if !project.authoring_diagnostics().is_empty() {
        return Err("工作区存在只读诊断，不能预览迁移写入".into());
    }
    if !project.recovery_conflicts().is_empty() {
        return Err("工程存在未解决的保存事务冲突".into());
    }
    let baseline = project.content_baseline();
    if request.expected_baseline != baseline {
        return Err(format!(
            "工程基线已过期，拒绝迁移预览；当前基线为 {baseline}"
        ));
    }
    check_tracked_disk_baselines(project)?;

    let source_root = checked_source_root(&request.source_root)?;
    let inputs = enumerate_source_files(&source_root)?;
    let markdown = inputs
        .iter()
        .filter(|file| {
            Path::new(&file.relative)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        })
        .cloned()
        .collect::<Vec<_>>();
    if markdown.is_empty() {
        return Err("来源目录中没有 Markdown 页面".into());
    }

    let mut text_total = 0usize;
    let mut pages = Vec::with_capacity(markdown.len());
    for input in &markdown {
        if input.length > MAX_MARKDOWN_BYTES_PER_PAGE as u64 {
            return Err(format!("Markdown 页面超过单页预算：{}", input.relative));
        }
        text_total = text_total
            .checked_add(input.length as usize)
            .ok_or("Markdown 文本大小溢出")?;
        if text_total > MAX_MARKDOWN_BYTES_TOTAL {
            return Err("Markdown 文本超过单次迁移预算".into());
        }
        let bytes = read_input_file(&source_root, input)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| format!("Markdown 页面不是有效 UTF-8：{}", input.relative))?;
        let parsed = parse_page(&input.relative, &bytes, text)?;
        pages.push(parsed);
    }
    pages.sort_by(|left, right| left.relative.cmp(&right.relative));

    let current = project.compile_current();
    let namespace = match request.namespace.as_deref() {
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
    resolve_page_ids(&current, &mut pages, request, &mut conflicts);

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
                        let bytes = Arc::new(read_input_file(&source_root, &input)?);
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
                    let extension = safe_extension(&input.relative);
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
                    if output_path_exists(project, &output_path)?
                        || (!quarantined && current.analysis.catalog.assets.contains_key(&id))
                    {
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
    let (candidate, additional_files, files, candidate_conflicts) = build_candidate(
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
        && (losses.is_empty() || request.accept_losses)
        && (!requires_language_upgrade || request.allow_language_upgrade);
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

#[cfg(not(target_arch = "wasm32"))]
fn build_candidate(
    project: &Project,
    current: &crate::CompileResult,
    pages: &[SourcePage],
    links: &[MarkdownLinkMapping],
    attachments: &[ResolvedAttachment],
    namespace: &str,
    requires_language_upgrade: bool,
) -> Result<CandidateBuild, String> {
    let mut candidate = project.clone();
    let output_root = format!(".world/markdown-imports/{namespace}");
    let relative_source = PathBuf::from(&output_root).join("import.wl");
    let absolute_source = candidate.root.join(&relative_source);
    let import_source_root = format!("{output_root}/sources");
    let mut generated = String::new();

    for page in pages {
        let source_copy = format!("{import_source_root}/{}", page.relative);
        generated.push_str(&format!(
            "entity {} kind {} as {}\n  description {}\n  property markdown_source = {}\n\n",
            page.id,
            page.entity_type,
            crate::authoring::quote(&page.title),
            crate::authoring::quote(&page.description),
            crate::authoring::quote(&source_copy),
        ));
        for heading in &page.headings {
            generated.push_str(&format!(
                "anchor_def {} as {}\n  description {}\nanchor_link {} entity {}\n\n",
                heading.id,
                crate::authoring::quote(&heading.title),
                crate::authoring::quote(&format!("{}:{}", page.relative, heading.line)),
                heading.id,
                page.id,
            ));
        }
    }

    let relation_type = format!("markdown_link_{namespace}");
    if !links.is_empty() {
        generated.push_str(&format!(
            "relation_type {relation_type} as {}\n  direction directed\n\n",
            crate::authoring::quote("Markdown 链接"),
        ));
        for link in links {
            let from = pages
                .iter()
                .find(|page| page.relative == link.source)
                .map(|page| page.id.as_str())
                .ok_or("Markdown 关系来源页面不存在")?;
            generated.push_str(&format!(
                "relation_def {} type {} from entity {} to {} {}\n  description {}\n  source_note {}\n\n",
                link.relation_id,
                relation_type,
                from,
                link.target.kind,
                link.target.id,
                crate::authoring::quote(&link.label),
                crate::authoring::quote(&format!("{}:{}", link.source, link.line)),
            ));
        }
    }

    let mut additional_by_path = BTreeMap::<PathBuf, Vec<u8>>::new();
    let mut seen_assets = BTreeSet::new();
    let mut seen_asset_links = BTreeSet::new();
    for attachment in attachments {
        match additional_by_path.get(&PathBuf::from(&attachment.output_path)) {
            Some(existing) if existing != attachment.bytes.as_ref() => {
                return Err(format!(
                    "相同附件目标对应不同来源字节：{}",
                    attachment.output_path
                ));
            }
            Some(_) => {}
            None => {
                additional_by_path.insert(
                    PathBuf::from(&attachment.output_path),
                    attachment.bytes.as_ref().clone(),
                );
            }
        }
        if !attachment.quarantined {
            let page_id = pages
                .iter()
                .find(|page| page.relative == attachment.source_page)
                .map(|page| page.id.as_str())
                .ok_or("Markdown 附件来源页面不存在")?;
            let relative_asset = attachment
                .output_path
                .strip_prefix(&format!("{output_root}/"))
                .ok_or("Markdown 附件目标路径不在导入目录内")?;
            let extension = safe_extension(&attachment.input.relative);
            if seen_assets.insert(attachment.id.clone()) {
                generated.push_str(&format!(
                    "asset {} {} {} as {}\n",
                    attachment.id,
                    asset_kind(&extension),
                    crate::authoring::quote(relative_asset),
                    crate::authoring::quote(&attachment.reference.label),
                ));
            }
            if seen_asset_links.insert((page_id.to_string(), attachment.id.clone())) {
                generated.push_str(&format!(
                    "attach entity {} with {}\n",
                    page_id, attachment.id
                ));
            }
            generated.push('\n');
        }
    }
    for page in pages {
        additional_by_path.insert(
            PathBuf::from(&import_source_root).join(&page.relative),
            page.bytes.clone(),
        );
    }

    candidate.add_file(&relative_source)?;
    candidate.set_text(&absolute_source, generated.clone())?;
    let manifest_relative = PathBuf::from(".world/project.json");
    let manifest_was_present = candidate
        .authoring_documents
        .contains_key(&candidate.root.join(&manifest_relative));
    update_import_manifest(&mut candidate, &relative_source)?;

    // Compile the complete proposal once so cross-file IDs and references are
    // validated against the same candidate that would be saved.
    let candidate_result = candidate.compile_current();
    let baseline_errors = diagnostic_error_counts(current);
    let candidate_errors = diagnostic_error_counts(&candidate_result);
    let mut conflicts = Vec::new();
    for (error, count) in candidate_errors {
        let previous = baseline_errors.get(&error).copied().unwrap_or_default();
        for _ in previous..count {
            conflicts.push(MarkdownImportConflict {
                code: "CANDIDATE_COMPILE_ERROR".into(),
                source: None,
                preferred_id: None,
                candidates: Vec::new(),
                message: format!(
                    "候选源码产生新编译错误 {}：{}:{} {}",
                    error.0, error.1, error.2, error.3
                ),
            });
        }
    }

    let mut files = Vec::new();
    for (path, document) in &candidate.documents {
        if document.is_dirty() {
            let relative = path
                .strip_prefix(&candidate.root)
                .map_err(|_| format!("写入路径不在工程目录内：{}", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            files.push(MarkdownImportFilePreview {
                path: relative,
                kind: if document.is_deleted() {
                    "project_source_delete".into()
                } else if path == &absolute_source {
                    "generated_source".into()
                } else {
                    "project_source_update".into()
                },
                source: None,
                bytes: if document.is_deleted() {
                    0
                } else {
                    document.text.len()
                },
            });
        }
    }
    for (path, document) in &candidate.authoring_documents {
        if document.is_dirty() {
            let relative = path
                .strip_prefix(&candidate.root)
                .map_err(|_| format!("写入路径不在工程目录内：{}", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            files.push(MarkdownImportFilePreview {
                path: relative,
                kind: if path == &candidate.root.join(&manifest_relative) {
                    if manifest_was_present && requires_language_upgrade {
                        "manifest_language_upgrade".into()
                    } else if manifest_was_present {
                        "manifest_preserving_update".into()
                    } else {
                        "manifest_create".into()
                    }
                } else if document.is_deleted() {
                    "authoring_document_delete".into()
                } else {
                    "authoring_document_update".into()
                },
                source: None,
                bytes: if document.is_deleted() {
                    0
                } else {
                    document.bytes.len()
                },
            });
        }
    }
    for page in pages {
        files.push(MarkdownImportFilePreview {
            path: format!("{import_source_root}/{}", page.relative),
            kind: "source_copy".into(),
            source: Some(page.relative.clone()),
            bytes: page.bytes.len(),
        });
    }
    for (relative, bytes) in &additional_by_path {
        let path = relative.to_string_lossy().replace('\\', "/");
        let attachment = attachments
            .iter()
            .find(|attachment| attachment.output_path == path);
        files.push(MarkdownImportFilePreview {
            path,
            kind: attachment.map_or_else(
                || "source_copy".into(),
                |attachment| {
                    if attachment.quarantined {
                        "quarantine_copy".into()
                    } else {
                        "attachment".into()
                    }
                },
            ),
            source: attachment
                .map(|attachment| attachment.input.relative.clone())
                .or_else(|| {
                    pages
                        .iter()
                        .find(|page| relative.ends_with(Path::new(&page.relative)))
                        .map(|page| page.relative.clone())
                }),
            bytes: bytes.len(),
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files.dedup_by(|left, right| left.path == right.path);
    let additional_files = additional_by_path.into_iter().collect();
    Ok((candidate, additional_files, files, conflicts))
}

fn diagnostic_error_counts(
    result: &crate::CompileResult,
) -> BTreeMap<(String, String, u32, String), usize> {
    let mut counts = BTreeMap::new();
    for diagnostic in result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == crate::Severity::Error)
    {
        *counts
            .entry((
                diagnostic.code.to_string(),
                diagnostic.file.clone(),
                diagnostic.span.line,
                diagnostic.message.clone(),
            ))
            .or_default() += 1;
    }
    counts
}

#[cfg(not(target_arch = "wasm32"))]
fn update_import_manifest(project: &mut Project, import_source: &Path) -> Result<(), String> {
    let manifest_path = crate::workspace_documents::manifest_path(&project.root);
    let relative_source = import_source.to_string_lossy().replace('\\', "/");
    let exists = project.authoring_documents.contains_key(&manifest_path);
    let mut value = if exists {
        let document = project.authoring_document(&manifest_path)?;
        crate::workspace_documents::parse_unique_json(&document.bytes)?
    } else {
        let entry = project
            .entry
            .strip_prefix(&project.root)
            .map_err(|_| "工程入口不在工作区内")?
            .to_string_lossy()
            .replace('\\', "/");
        serde_json::json!({
            "schema_version": 1,
            "language_version": "1.10",
            "entry": entry,
            "required_features": []
        })
    };
    let object = value
        .as_object_mut()
        .ok_or("工作区清单顶层必须是 JSON 对象")?;
    let mut changed = !exists;
    if object
        .get("language_version")
        .and_then(serde_json::Value::as_str)
        != Some("1.10")
    {
        object.insert(
            "language_version".into(),
            serde_json::Value::String("1.10".into()),
        );
        changed = true;
    }
    let mut needs_source_set_feature = false;
    for required in ["content.entities.v1", "content.relations.v1"] {
        let required_features = object
            .entry("required_features")
            .or_insert_with(|| serde_json::Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or("required_features 必须是数组")?;
        if !required_features
            .iter()
            .any(|feature| feature.as_str() == Some(required))
        {
            required_features.push(serde_json::Value::String(required.into()));
            changed = true;
        }
    }
    if let Some(source_config) = object.get_mut("source_config") {
        let source_config = source_config
            .as_object_mut()
            .ok_or("source_config 必须是对象")?;
        let active = source_config
            .get_mut("active")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or("source_config.active 必须是数组")?;
        if !active
            .iter()
            .any(|path| path.as_str() == Some(&relative_source))
        {
            active.push(serde_json::Value::String(relative_source));
            changed = true;
        }
        active.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        needs_source_set_feature = true;
    }
    if needs_source_set_feature {
        let required_features = object
            .get_mut("required_features")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or("required_features 必须是数组")?;
        if !required_features
            .iter()
            .any(|feature| feature.as_str() == Some("workspace.source_sets.v1"))
        {
            required_features.push(serde_json::Value::String("workspace.source_sets.v1".into()));
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }
    let bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    if exists {
        project.set_authoring_document(&manifest_path, bytes)?;
    } else {
        project.create_authoring_document(&manifest_path, bytes)?;
    }
    Ok(())
}

fn asset_kind(extension: &str) -> &'static str {
    match extension {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => "image",
        "wav" | "mp3" | "ogg" | "flac" | "m4a" | "aac" => "audio",
        _ => "file",
    }
}

fn safe_extension(relative: &str) -> String {
    let extension = Path::new(relative)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.is_empty()
        && extension.len() <= 16
        && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        extension.to_ascii_lowercase()
    } else {
        "bin".into()
    }
}

fn validate_import_relative_path(relative: &str) -> Result<(), String> {
    if relative.len() > MAX_IMPORT_PATH_BYTES {
        return Err(format!("Markdown 来源相对路径过长：{relative}"));
    }
    for component in relative.split('/') {
        let stem = component
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let reserved_device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|suffix| {
                    suffix.len() == 1
                        && suffix.as_bytes()[0].is_ascii_digit()
                        && suffix.as_bytes()[0] != b'0'
                })
            });
        if component.is_empty()
            || component.len() > 200
            || component.ends_with(['.', ' '])
            || component.chars().any(|character| {
                character.is_control()
                    || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\\')
            })
            || reserved_device
        {
            return Err(format!(
                "Markdown 来源路径不是可安全迁移的相对路径：{relative}"
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn check_tracked_disk_baselines(project: &Project) -> Result<(), String> {
    for path in project
        .documents
        .keys()
        .chain(project.authoring_documents.keys())
    {
        let state = project
            .tracked_file_state(path)
            .ok_or_else(|| format!("无法读取 Project 基线：{}", path.display()))?;
        let disk = match crate::file_access::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("无法核对 Project 基线：{error}")),
        };
        if disk != state.baseline {
            return Err(format!("工程文件在打开后发生外部变化：{}", path.display()));
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn checked_source_root(path: &Path) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("无法读取 Markdown 来源目录：{error}"))?;
    if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
        return Err("Markdown 来源必须是普通目录，不能是符号链接或联接".into());
    }
    std::fs::canonicalize(path).map_err(|error| format!("无法规范化 Markdown 来源目录：{error}"))
}

#[cfg(not(target_arch = "wasm32"))]
fn read_input_file(root: &Path, input: &InputFile) -> Result<Vec<u8>, String> {
    let metadata = std::fs::symlink_metadata(&input.absolute)
        .map_err(|error| format!("无法检查来源文件 {}：{error}", input.relative))?;
    if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_file() {
        return Err(format!(
            "来源文件在预览期间变为链接或特殊文件：{}",
            input.relative
        ));
    }
    let canonical = std::fs::canonicalize(&input.absolute)
        .map_err(|error| format!("无法规范化来源文件 {}：{error}", input.relative))?;
    if !canonical.starts_with(root) {
        return Err(format!("来源文件越过所选目录：{}", input.relative));
    }
    if metadata.len() != input.length {
        return Err(format!("来源文件在扫描后发生变化：{}", input.relative));
    }
    let bytes = std::fs::read(&canonical)
        .map_err(|error| format!("无法读取来源文件 {}：{error}", input.relative))?;
    if bytes.len() as u64 != input.length {
        return Err(format!("来源文件在读取期间发生变化：{}", input.relative));
    }
    Ok(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
fn enumerate_source_files(root: &Path) -> Result<Vec<InputFile>, String> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut entries_seen = 0usize;
    let mut portable_paths = BTreeMap::<String, String>::new();
    while let Some(directory) = pending.pop() {
        let metadata = std::fs::symlink_metadata(&directory)
            .map_err(|error| format!("无法检查 Markdown 来源目录：{error}"))?;
        let canonical = std::fs::canonicalize(&directory)
            .map_err(|error| format!("无法规范化 Markdown 来源目录：{error}"))?;
        if crate::file_access::is_link_or_junction(&metadata)
            || !metadata.is_dir()
            || !canonical.starts_with(root)
        {
            return Err("Markdown 来源目录在扫描期间变为链接或越界目录".into());
        }
        let entries = std::fs::read_dir(&directory)
            .map_err(|error| format!("无法枚举 Markdown 来源目录：{error}"))?;
        for entry in entries {
            entries_seen = entries_seen.saturating_add(1);
            if entries_seen > MAX_IMPORT_ENTRIES {
                return Err("Markdown 来源目录项超过单次迁移预算".into());
            }
            let entry = entry.map_err(|error| format!("无法枚举 Markdown 来源目录：{error}"))?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| format!("无法检查 Markdown 来源路径：{error}"))?;
            if crate::file_access::is_link_or_junction(&metadata) {
                return Err(format!(
                    "Markdown 来源不允许符号链接或联接：{}",
                    path.display()
                ));
            }
            let relative_path = path
                .strip_prefix(root)
                .map_err(|_| "Markdown 来源路径越界")?;
            if relative_path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err(format!("Markdown 来源路径不安全：{}", path.display()));
            }
            let relative = relative_path
                .to_str()
                .ok_or_else(|| format!("Markdown 来源路径不是 UTF-8：{}", path.display()))?
                .replace('\\', "/");
            validate_import_relative_path(&relative)?;
            let folded = relative.to_lowercase();
            if portable_paths
                .insert(folded, relative.clone())
                .is_some_and(|previous| previous != relative)
            {
                return Err(format!(
                    "Markdown 来源包含大小写折叠后重名的路径：{relative}"
                ));
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                files.push(InputFile {
                    relative,
                    absolute: path,
                    length: metadata.len(),
                });
            } else {
                return Err(format!("Markdown 来源包含特殊文件：{}", path.display()));
            }
            if files.len() > MAX_IMPORT_FILES {
                return Err("Markdown 来源文件数超过单次迁移预算".into());
            }
        }
    }
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}

fn parse_page(relative: &str, bytes: &[u8], text: &str) -> Result<SourcePage, String> {
    let mut lines = text.lines().enumerate().peekable();
    let mut frontmatter = BTreeMap::new();
    if lines
        .peek()
        .is_some_and(|(_, line)| line.trim_end_matches('\r') == "---")
    {
        lines.next();
        let mut closed = false;
        for (line_index, raw) in lines.by_ref() {
            let line = raw.trim_end_matches('\r');
            if line == "---" || line == "..." {
                closed = true;
                break;
            }
            let Some((key, value)) = line.split_once(':') else {
                return Err(format!(
                    "front matter 必须为 key: value 标量：{relative}:{}",
                    line_index + 1
                ));
            };
            let key = key.trim();
            let value = parse_scalar(value.trim()).ok_or_else(|| {
                format!("front matter 不支持该标量值：{relative}:{}", line_index + 1)
            })?;
            if !valid_id(key) {
                return Err(format!(
                    "front matter 键名无效：{relative}:{}",
                    line_index + 1
                ));
            }
            if frontmatter.insert(key.to_string(), value).is_some() {
                return Err(format!("front matter 存在重复键：{relative}:{key}"));
            }
        }
        if !closed {
            return Err(format!("front matter 缺少结束分隔线：{relative}"));
        }
    }
    let mut title = frontmatter.get("title").cloned();
    let mut explicit_id = frontmatter.get("id").cloned();
    let entity_type = frontmatter
        .get("kind")
        .cloned()
        .unwrap_or_else(|| "lore".into());
    if !valid_id(&entity_type) {
        return Err(format!("front matter kind 不是有效标识符：{relative}"));
    }
    let mut losses = Vec::new();
    for key in frontmatter.keys() {
        if !matches!(key.as_str(), "id" | "title" | "kind") {
            losses.push(loss(
                "UNSUPPORTED_FRONT_MATTER_FIELD",
                relative,
                1,
                format!("front matter 字段 `{key}` 不会转成实体字段"),
                None,
            ));
        }
    }
    if title.as_ref().is_some_and(|value| value.trim().is_empty()) {
        title = None;
    }
    let fallback_id = format!("md_{:016x}", hash_bytes(relative.as_bytes()));
    let preferred_id = explicit_id.take().unwrap_or_else(|| fallback_id.clone());
    let valid_explicit_id = valid_id(&preferred_id);
    let id = if valid_explicit_id {
        preferred_id
    } else {
        losses.push(loss(
            "INVALID_FRONT_MATTER_ID",
            relative,
            1,
            "front matter id 无效；预览会提供确定的映射候选".into(),
            None,
        ));
        fallback_id
    };

    let body_start = if text.starts_with("---\n") || text.starts_with("---\r\n") {
        let mut offset = 0usize;
        let mut boundary = None;
        for line in text.split_inclusive('\n').skip(1) {
            offset += line.len();
            let content = line
                .strip_suffix('\n')
                .unwrap_or(line)
                .trim_end_matches('\r');
            if content == "---" || content == "..." {
                boundary = Some(offset);
                break;
            }
        }
        boundary.unwrap_or(text.len())
    } else {
        0
    };
    let body = &text[body_start..];
    let body_first_line = text[..body_start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count() as u32
        + 1;
    let mut rendered = Vec::new();
    let mut headings = Vec::new();
    let mut links = Vec::new();
    let mut attachments = Vec::new();
    let mut in_fence = false;
    let mut fence_start_line = 0u32;
    let mut first_heading_title = None;
    let mut prose = Vec::<String>::new();

    let flush_prose = |prose: &mut Vec<String>, rendered: &mut Vec<String>| {
        if !prose.is_empty() {
            rendered.push(prose.join(" "));
            prose.clear();
        }
    };
    for (body_line_index, raw) in body.lines().enumerate() {
        let line_number = body_first_line + body_line_index as u32;
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if !in_fence {
                fence_start_line = line_number;
                flush_prose(&mut prose, &mut rendered);
                losses.push(loss(
                    "UNSUPPORTED_CODE_BLOCK",
                    relative,
                    line_number,
                    "代码块只保留在原文副本中，不执行或转换".into(),
                    None,
                ));
                rendered.push("[未转换代码块；请查看原文副本]".into());
            }
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.is_empty() {
            flush_prose(&mut prose, &mut rendered);
            continue;
        }
        if let Some((level, heading_text)) = parse_atx_heading(trimmed) {
            flush_prose(&mut prose, &mut rendered);
            let (visible, references, formatting_loss) = parse_inline(heading_text, line_number);
            if formatting_loss {
                losses.push(loss(
                    "UNSUPPORTED_INLINE_MARKUP",
                    relative,
                    line_number,
                    "标题内行内标记被折叠为可见文字，原文副本保留原语法".into(),
                    None,
                ));
            }
            if level == 1 && first_heading_title.is_none() {
                first_heading_title = Some(visible.clone());
            }
            let slug = heading_slug(&visible);
            headings.push(Heading {
                slug,
                id: String::new(),
                title: visible.clone(),
                line: line_number,
            });
            rendered.push(visible);
            collect_references(
                references,
                relative,
                &mut links,
                &mut attachments,
                &mut losses,
            );
            continue;
        }
        if trimmed.starts_with('>') {
            flush_prose(&mut prose, &mut rendered);
            losses.push(loss(
                "UNSUPPORTED_BLOCK_QUOTE",
                relative,
                line_number,
                "该块结构只保留在原文副本中".into(),
                None,
            ));
            let quote_text = trimmed.trim_start_matches('>').trim_start();
            let (visible, references, formatting_loss) = parse_inline(quote_text, line_number);
            if formatting_loss {
                losses.push(loss(
                    "UNSUPPORTED_INLINE_MARKUP",
                    relative,
                    line_number,
                    "引用块内行内标记被折叠为可见文字，原文副本保留原语法".into(),
                    None,
                ));
            }
            rendered.push(visible);
            collect_references(
                references,
                relative,
                &mut links,
                &mut attachments,
                &mut losses,
            );
            continue;
        }
        if trimmed.starts_with('|') && trimmed.contains('|') {
            flush_prose(&mut prose, &mut rendered);
            losses.push(loss(
                "UNSUPPORTED_TABLE",
                relative,
                line_number,
                "表格结构会折叠为单行文字，原文副本保留原始单元格".into(),
                None,
            ));
            let mut cells = Vec::new();
            for cell in trimmed.trim_matches('|').split('|') {
                let (visible, references, formatting_loss) = parse_inline(cell.trim(), line_number);
                if formatting_loss {
                    losses.push(loss(
                        "UNSUPPORTED_INLINE_MARKUP",
                        relative,
                        line_number,
                        "表格单元格内行内标记被折叠为可见文字，原文副本保留原语法".into(),
                        None,
                    ));
                }
                cells.push(visible);
                collect_references(
                    references,
                    relative,
                    &mut links,
                    &mut attachments,
                    &mut losses,
                );
            }
            rendered.push(cells.join(" | "));
            continue;
        }
        if trimmed.starts_with('<') {
            flush_prose(&mut prose, &mut rendered);
            losses.push(loss(
                "RAW_HTML_ISOLATED",
                relative,
                line_number,
                "HTML 不会执行，原文只保留在隔离副本中".into(),
                None,
            ));
            rendered.push("[未转换 HTML；请查看原文副本]".into());
            continue;
        }
        if let Some(item_text) = strip_list_marker(trimmed) {
            flush_prose(&mut prose, &mut rendered);
            losses.push(loss(
                "UNSUPPORTED_LIST",
                relative,
                line_number,
                "列表层级会折叠为独立文字行，原文副本保留标记与缩进".into(),
                None,
            ));
            let (visible, references, formatting_loss) = parse_inline(item_text, line_number);
            if formatting_loss {
                losses.push(loss(
                    "UNSUPPORTED_INLINE_MARKUP",
                    relative,
                    line_number,
                    "列表项内行内标记被折叠为可见文字，原文副本保留原语法".into(),
                    None,
                ));
            }
            rendered.push(visible);
            collect_references(
                references,
                relative,
                &mut links,
                &mut attachments,
                &mut losses,
            );
            continue;
        }
        if is_horizontal_rule(trimmed) {
            flush_prose(&mut prose, &mut rendered);
            losses.push(loss(
                "UNSUPPORTED_HORIZONTAL_RULE",
                relative,
                line_number,
                "分隔线只保留在原文副本中".into(),
                None,
            ));
            continue;
        }
        let text_line = trimmed;
        let (visible, references, formatting_loss) = parse_inline(text_line, line_number);
        if formatting_loss {
            losses.push(loss(
                "UNSUPPORTED_INLINE_MARKUP",
                relative,
                line_number,
                "段落内行内标记被折叠为可见文字，原文副本保留原语法".into(),
                None,
            ));
        }
        prose.push(visible);
        collect_references(
            references,
            relative,
            &mut links,
            &mut attachments,
            &mut losses,
        );
    }
    flush_prose(&mut prose, &mut rendered);
    if in_fence {
        losses.push(loss(
            "UNCLOSED_CODE_BLOCK",
            relative,
            fence_start_line.max(1),
            "代码块未闭合；原文副本保留全部内容".into(),
            None,
        ));
    }
    if title.is_none() {
        title = first_heading_title;
    }
    if title.is_none() {
        title = Path::new(relative)
            .file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_owned);
    }
    let title = title
        .filter(|title| !title.trim().is_empty())
        .ok_or_else(|| format!("无法生成 Markdown 页面显示名：{relative}"))?;
    let description = rendered
        .into_iter()
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok(SourcePage {
        relative: relative.to_string(),
        bytes: bytes.to_vec(),
        id,
        invalid_front_matter_id: !valid_explicit_id,
        title,
        entity_type,
        description,
        headings,
        links,
        attachments,
        losses,
    })
}

fn resolve_page_ids(
    current: &crate::CompileResult,
    pages: &mut [SourcePage],
    request: &MarkdownImportRequest,
    conflicts: &mut Vec<MarkdownImportConflict>,
) {
    let mut used = BTreeSet::new();
    for page in pages {
        let had_override = request.id_overrides.contains_key(&page.relative);
        if page.invalid_front_matter_id && !had_override {
            conflicts.push(MarkdownImportConflict {
                code: "INVALID_FRONT_MATTER_ID".into(),
                source: Some(page.relative.clone()),
                preferred_id: Some(page.id.clone()),
                candidates: vec![page.id.clone()],
                message: "front matter ID 无效；请通过 source-relative-path 显式确认替代 ID".into(),
            });
            continue;
        }
        let preferred = request
            .id_overrides
            .get(&page.relative)
            .cloned()
            .unwrap_or_else(|| page.id.clone());
        if !valid_id(&preferred) {
            conflicts.push(MarkdownImportConflict {
                code: "INVALID_ID_MAPPING".into(),
                source: Some(page.relative.clone()),
                preferred_id: Some(preferred.clone()),
                candidates: vec![format!("md_{:016x}", hash_bytes(page.relative.as_bytes()))],
                message: format!("目标 ID 不是有效的语言标识符：{preferred}"),
            });
            continue;
        }
        let target = TargetRef::new("entity", &preferred);
        let exists = current.analysis.catalog.object(&target).is_some();
        if exists || !used.insert(preferred.clone()) {
            let candidates = id_candidates(&preferred, current, &used);
            conflicts.push(MarkdownImportConflict {
                code: if exists {
                    "ENTITY_ID_CONFLICT".into()
                } else {
                    "DUPLICATE_IMPORT_ID".into()
                },
                source: Some(page.relative.clone()),
                preferred_id: Some(preferred.clone()),
                candidates,
                message: format!(
                    "ID `{preferred}` 已被占用；不会自动合并，请显式提供此来源页的 ID 映射"
                ),
            });
            continue;
        }
        page.id = preferred;
    }
}

fn id_candidates(
    preferred: &str,
    current: &crate::CompileResult,
    used: &BTreeSet<String>,
) -> Vec<String> {
    let mut candidates = Vec::new();
    for suffix in 2..=4 {
        let candidate = format!("{preferred}_import_{suffix}");
        if !used.contains(&candidate)
            && current
                .analysis
                .catalog
                .object(&TargetRef::new("entity", &candidate))
                .is_none()
        {
            candidates.push(candidate);
        }
    }
    candidates
}

fn parse_scalar(value: &str) -> Option<String> {
    if value.len() >= 2 {
        let first = value.as_bytes()[0];
        let last = *value.as_bytes().last()?;
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            let inner = &value[1..value.len() - 1];
            if first == b'"' && inner.contains('\\') {
                return None;
            }
            if first == b'\'' && inner.contains('\'') {
                return None;
            }
            return Some(inner.to_string());
        }
    }
    (!value.is_empty()
        && !value.starts_with(|character: char| {
            matches!(character, '[' | '{' | '&' | '*' | '!' | '|' | '>')
        }))
    .then(|| value.to_string())
}

fn parse_atx_heading(line: &str) -> Option<(usize, &str)> {
    let count = line.bytes().take_while(|byte| *byte == b'#').count();
    if count == 0
        || count > 6
        || !line
            .as_bytes()
            .get(count)
            .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        return None;
    }
    let text = line[count..].trim().trim_end_matches('#').trim_end();
    Some((count, text))
}

fn strip_list_marker(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'-' | b'+' | b'*') && bytes[1].is_ascii_whitespace()
    {
        return Some(line[2..].trim_start());
    }
    let digit_end = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digit_end > 0
        && bytes
            .get(digit_end)
            .is_some_and(|byte| matches!(byte, b'.' | b')'))
        && bytes
            .get(digit_end + 1)
            .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        return Some(line[digit_end + 2..].trim_start());
    }
    None
}

fn is_horizontal_rule(line: &str) -> bool {
    let marker = line
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    marker.len() >= 3
        && matches!(marker[0], b'-' | b'*' | b'_')
        && marker.iter().all(|byte| *byte == marker[0])
}

fn parse_inline(input: &str, line: u32) -> (String, Vec<(InlineReference, bool)>, bool) {
    let mut output = String::new();
    let mut references = Vec::new();
    let link_delimiters = unescaped_positions(input, b"](");
    let close_parens = unescaped_positions(input, b")");
    let mut index = 0usize;
    let mut formatted = false;
    let mut code_delimiter = None;
    while index < input.len() {
        let rest = &input[index..];
        if rest.starts_with('`') && !is_escaped(input, index) {
            let width = rest.bytes().take_while(|byte| *byte == b'`').count();
            match code_delimiter {
                None => {
                    code_delimiter = Some(width);
                    formatted = true;
                    index += width;
                    continue;
                }
                Some(open_width) if width == open_width => {
                    code_delimiter = None;
                    index += width;
                    continue;
                }
                Some(_) => {
                    output.push_str(&rest[..width]);
                    index += width;
                    continue;
                }
            }
        }
        if code_delimiter.is_some() {
            let character = rest.chars().next().unwrap();
            output.push(character);
            index += character.len_utf8();
            continue;
        }
        if let Some(unescaped) = rest.strip_prefix('\\') {
            if let Some(escaped) = unescaped.chars().next() {
                if escaped.is_ascii_punctuation() {
                    output.push(escaped);
                    index += 1 + escaped.len_utf8();
                    continue;
                }
            }
        }
        let image = rest.starts_with("![");
        let opening = if image { index + 1 } else { index };
        if (image || rest.starts_with('[')) && !is_escaped(input, index) {
            if let Some((label, href, end)) =
                parse_link_at(input, opening, &link_delimiters, &close_parens)
            {
                let reference = InlineReference {
                    href: href.clone(),
                    label: label.clone(),
                    line,
                };
                references.push((reference, image));
                output.push_str(&label);
                index = end;
                continue;
            } else {
                formatted = true;
            }
        }
        let character = rest.chars().next().unwrap();
        if character == '<' && rest.contains('>') {
            formatted = true;
        }
        if matches!(character, '*' | '_') {
            formatted = true;
            let marker_width = if rest.starts_with("**") || rest.starts_with("__") {
                2
            } else {
                1
            };
            if rest.len() >= marker_width * 2 {
                index += marker_width;
                continue;
            }
        }
        output.push(character);
        index += character.len_utf8();
    }
    (output, references, formatted)
}

fn parse_link_at(
    input: &str,
    opening: usize,
    link_delimiters: &[usize],
    close_parens: &[usize],
) -> Option<(String, String, usize)> {
    let closing_label = next_after(link_delimiters, opening)?;
    let closing_href = next_after(close_parens, closing_label + 1)?;
    let label = input[opening + 1..closing_label].to_string();
    let href = input[closing_label + 2..closing_href]
        .split_once(char::is_whitespace)
        .map(|(href, _)| href)
        .unwrap_or(&input[closing_label + 2..closing_href])
        .trim_matches(['<', '>'])
        .to_string();
    Some((label, href, closing_href + 1))
}

fn unescaped_positions(input: &str, marker: &[u8]) -> Vec<usize> {
    let bytes = input.as_bytes();
    let mut positions = Vec::new();
    let mut index = 0;
    let mut backslashes = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            backslashes += 1;
            index += 1;
            continue;
        }
        if backslashes.is_multiple_of(2) && bytes[index..].starts_with(marker) {
            positions.push(index);
        }
        backslashes = 0;
        let character = input[index..].chars().next().unwrap();
        index += character.len_utf8();
    }
    positions
}

fn next_after(positions: &[usize], index: usize) -> Option<usize> {
    positions
        .get(positions.partition_point(|position| *position <= index))
        .copied()
}

fn is_escaped(value: &str, index: usize) -> bool {
    value.as_bytes()[..index]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

fn collect_references(
    references: Vec<(InlineReference, bool)>,
    _source: &str,
    links: &mut Vec<InlineReference>,
    attachments: &mut Vec<InlineReference>,
    losses: &mut Vec<MarkdownImportLoss>,
) {
    for (reference, image) in references {
        if image {
            attachments.push(reference);
        } else if is_markdown_link(&reference.href) || has_uri_scheme(&reference.href) {
            links.push(reference);
        } else if reference.href.is_empty() {
            losses.push(loss(
                "UNSUPPORTED_LINK_SYNTAX",
                _source,
                reference.line,
                "Markdown 链接目标为空，保留在原文副本".into(),
                None,
            ));
        } else {
            // 普通文件链接也作为附件预览；脚本会在目标分类阶段隔离。
            attachments.push(reference);
        }
    }
}

enum Href {
    Local {
        path: String,
        fragment: Option<String>,
    },
    External,
    Unsafe,
    Malformed,
}

fn resolve_relative_href(source: &str, href: &str) -> Href {
    let decoded = match percent_decode(href.trim()) {
        Some(value) => value,
        None => return Href::Malformed,
    };
    let href = decoded.as_str();
    if has_uri_scheme(href) {
        return Href::External;
    }
    if href.starts_with('/')
        || href.starts_with('\\')
        || href.as_bytes().get(1) == Some(&b':')
        || href.starts_with("//")
    {
        return Href::Unsafe;
    }
    let (without_fragment, fragment) = match href.split_once('#') {
        Some((path, fragment)) => (path, Some(normalize_fragment(fragment))),
        None => (href, None),
    };
    if without_fragment.contains('?') {
        return Href::Malformed;
    }
    let parent = Path::new(source).parent().unwrap_or_else(|| Path::new(""));
    let mut components = Vec::new();
    for component in parent.join(without_fragment).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => {
                let Some(value) = value.to_str() else {
                    return Href::Malformed;
                };
                components.push(value.to_string());
            }
            Component::ParentDir if !components.is_empty() => {
                components.pop();
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Href::Unsafe
            }
        }
    }
    let path = components.join("/");
    if path.is_empty() && fragment.is_none() {
        return Href::Malformed;
    }
    if path.is_empty() {
        return Href::Local {
            path: source.to_string(),
            fragment,
        };
    }
    Href::Local { path, fragment }
}

fn normalize_fragment(value: &str) -> String {
    heading_slug(value.trim().trim_start_matches('#'))
}

fn has_uri_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn is_markdown_link(href: &str) -> bool {
    if href.starts_with('#') {
        return true;
    }
    let decoded = percent_decode(href).unwrap_or_else(|| href.to_string());
    let path = decoded.split(['#', '?']).next().unwrap_or_default();
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            decoded.push((hex_value(high)? << 4) | hex_value(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn heading_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(character.to_ascii_lowercase());
        } else if character.is_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.extend(character.to_lowercase());
        } else if character.is_whitespace() || character == '-' {
            pending_dash = true;
        }
    }
    if slug.is_empty() {
        format!("h_{:016x}", hash_bytes(value.as_bytes()))
    } else {
        slug
    }
}

fn valid_id(value: &str) -> bool {
    crate::lexer::valid_identifier(value)
}

fn loss(
    code: &str,
    source: &str,
    line: u32,
    message: String,
    preserved_at: Option<String>,
) -> MarkdownImportLoss {
    MarkdownImportLoss {
        code: code.to_string(),
        source: source.to_string(),
        line,
        message,
        preserved_at,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn output_path_exists(project: &Project, relative: &str) -> Result<bool, String> {
    let relative = Path::new(relative);
    let target = crate::file_access::within(&project.root, &project.root.join(relative))?;
    let mut current = project.root.clone();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(part) = component else {
            return Err("导入目标路径包含非普通路径段".into());
        };
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if crate::file_access::is_link_or_junction(&metadata) {
                    return Err(format!("导入目标路径包含链接或联接：{}", current.display()));
                }
                if index + 1 < components.len() && !metadata.is_dir() {
                    return Ok(true);
                }
                if index + 1 == components.len() {
                    return Ok(true);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("无法检查导入目标 {}：{error}", target.display())),
        }
    }
    Ok(false)
}

#[cfg(not(target_arch = "wasm32"))]
fn source_fingerprint(
    inputs: &[InputFile],
    pages: &[SourcePage],
    attachments: &[ResolvedAttachment],
) -> Result<String, String> {
    let mut bytes = Vec::new();
    for input in inputs {
        bytes.extend_from_slice(input.relative.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&input.length.to_le_bytes());
        bytes.push(0xff);
    }
    for page in pages {
        bytes.extend_from_slice(page.relative.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&page.bytes);
        bytes.push(0xff);
    }
    let mut unique = BTreeMap::<&str, &[u8]>::new();
    for attachment in attachments {
        unique
            .entry(&attachment.input.relative)
            .or_insert_with(|| attachment.bytes.as_slice());
    }
    for (relative, contents) in unique {
        bytes.extend_from_slice(relative.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(contents);
        bytes.push(0xff);
    }
    Ok(format!("{:016x}", hash_bytes(&bytes)))
}

fn hash_strings<'a>(values: impl IntoIterator<Item = &'a str>) -> u64 {
    let mut bytes = Vec::new();
    for value in values {
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0);
    }
    hash_bytes(&bytes)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn digest_plan(plan: PlanDigest<'_>) -> Result<String, String> {
    let bytes = serde_json::to_vec(&plan)
        .map_err(|error| format!("无法生成 Markdown 预览摘要：{error}"))?;
    Ok(format!("{:016x}", hash_bytes(&bytes)))
}

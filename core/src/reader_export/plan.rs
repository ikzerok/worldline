use super::inputs::{build_exclusions, read_public_asset};
use super::progress::{check_budget, report};
use super::render::render_object_body;
use super::site::{html_escape, relative_url};
use super::*;
use crate::manuscript::{ManuscriptEntryKind, ManuscriptReferenceStatus};
use std::path::Path;

impl Project {
    pub fn preview_reader_export(
        &self,
        selection: &ReaderExportSelection,
    ) -> Result<ReaderExportPreview, String> {
        self.preview_reader_export_with_progress(selection, &mut |_| true)
    }

    pub fn build_reader_export(
        &self,
        selection: &ReaderExportSelection,
        expected_plan_digest: &str,
    ) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
        self.build_reader_export_with_progress(selection, expected_plan_digest, &mut |_| true)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn export_reader_site(
        &self,
        selection: &ReaderExportSelection,
        expected_plan_digest: &str,
        destination: &Path,
    ) -> Result<(), String> {
        self.export_reader_site_with_progress(
            selection,
            expected_plan_digest,
            destination,
            &mut |_| true,
        )
    }
}

pub(super) fn prepare_with_routes(
    project: &Project,
    selection: &ReaderExportSelection,
    overrides: &[ReaderProfileRoute],
    progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
) -> Result<PreparedExport, String> {
    report(progress, "validate", 0, 1)?;
    super::routes::validate_profile_routes(overrides)?;
    validate_selection(selection)?;
    #[cfg(not(target_arch = "wasm32"))]
    project.ensure_storage_ready()?;

    // Compile a clone so include loading never mutates the caller's buffers.
    let mut compile_project = project.clone();
    report(progress, "compile", 0, 1)?;
    let compiled = compile_project.compile();
    report(progress, "compile", 1, 1)?;
    if compiled.has_errors() {
        let details = compiled
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == crate::Severity::Error)
            .map(|diagnostic| format!("{}:{}", diagnostic.code, diagnostic.message))
            .collect::<Vec<_>>()
            .join("；");
        return Err(format!(
            "工程存在编译错误，无法安全生成静态阅读包：{details}"
        ));
    }
    // Enumerating paths checks symlink and workspace transaction boundaries without
    // reading or copying unrelated private file contents.
    let workspace_paths = crate::file_access::workspace_files(&project.root)
        .map_err(|error| format!("无法安全读取工作区：{error}"))?;
    let indexes = project.manuscript_indices();

    let selected_objects: BTreeSet<_> = selection.objects.iter().cloned().collect();
    for target in &selection.objects {
        if !public_object_kind(&target.kind) {
            return Err(format!("不支持公开此类对象：{}", target.kind));
        }
        if compiled.analysis.catalog.object(target).is_none() {
            return Err(format!("公开选择中有未知对象：{}", target.kind));
        }
    }
    super::fields::validate_fields(&compiled, selection)?;
    let selected_asset_ids: BTreeSet<_> = selection.attachments.iter().cloned().collect();
    let mut attachment_bytes = 0usize;
    let mut attachments = Vec::new();
    let mut included = Vec::new();
    for (index, id) in selection.attachments.iter().enumerate() {
        let asset = compiled
            .analysis
            .catalog
            .assets
            .get(id)
            .ok_or_else(|| format!("公开选择中有未知附件：{id}"))?;
        let (extension, bytes, source_path) = read_public_asset(project, asset, &workspace_paths)?;
        attachment_bytes = attachment_bytes
            .checked_add(bytes.len())
            .ok_or("附件总大小超出限制")?;
        if attachment_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err("附件总大小超出 64 MiB 限制".into());
        }
        attachments.push(PublicAttachment {
            id: id.clone(),
            display: asset.display.clone(),
            source_path,
            output_path: PathBuf::from(super::routes::target_route(
                selection,
                &TargetRef::new("asset", id),
                format!("assets/a{:04}.{extension}", index + 1),
                overrides,
            )?),
            bytes,
        });
    }

    let mut routes = BTreeMap::<TargetRef, String>::new();
    let mut object_order = selection.objects.clone();
    object_order.sort();
    for (index, target) in object_order.iter().enumerate() {
        routes.insert(
            target.clone(),
            super::routes::target_route(
                selection,
                target,
                format!("objects/o{:04}.html", index + 1),
                overrides,
            )?,
        );
    }
    for attachment in &attachments {
        routes.insert(
            TargetRef::new("asset", &attachment.id),
            super::portable_output_path(&attachment.output_path)?,
        );
        included.push(ReaderExportIncluded {
            target: Some(TargetRef::new("asset", &attachment.id)),
            manuscript_id: None,
            chapter_id: None,
            title: attachment.display.clone(),
            output_path: super::portable_output_path(&attachment.output_path)?,
        });
    }

    let mut file_routes = BTreeMap::new();
    for id in &selected_asset_ids {
        if let Some(asset) = compiled.analysis.catalog.assets.get(id) {
            let path = crate::compiler::source_path(Path::new(&asset.resolved_path));
            if let Some(route) = routes.get(&TargetRef::new("asset", id)) {
                file_routes.insert(path.to_string_lossy().into_owned(), route.clone());
            }
        }
    }

    let public_field_references = super::fields::public_references(&compiled, selection);
    let mut pages = Vec::new();
    for (object_index, target) in object_order.into_iter().enumerate() {
        report(progress, "objects", object_index, selection.objects.len())?;
        let route = routes.get(&target).expect("selected object route").clone();
        let object = compiled
            .analysis
            .catalog
            .object(&target)
            .expect("selection was validated");
        let (body_html, searchable_text) =
            render_object_body(&compiled, &target, &routes, &file_routes, selection)?;
        let empty_content = searchable_text.trim().is_empty();
        let aliases = if selection.schema_version == READER_SITE_SCHEMA_VERSION {
            compiled
                .analysis
                .catalog
                .aliases
                .iter()
                .filter(|alias| alias.target == target)
                .map(|alias| alias.name.clone())
                .collect()
        } else {
            Vec::new()
        };
        let mut page = PublicPage {
            title: object.display.clone(),
            output_path: PathBuf::from(&route),
            body_html,
            searchable_text,
            kind: target.kind.clone(),
            aliases,
            anchors: Vec::new(),
            empty_content,
        };
        if selection.schema_version == READER_SITE_SCHEMA_VERSION {
            super::semantics::append_object(
                &compiled,
                &target,
                &routes,
                &public_field_references,
                &mut page,
            )?;
        }
        pages.push(page);
        check_budget(&pages, &attachments)?;
        included.push(ReaderExportIncluded {
            target: Some(target),
            manuscript_id: None,
            chapter_id: None,
            title: object.display.clone(),
            output_path: route,
        });
    }

    let mut selected_chapter_pairs = BTreeSet::new();
    for (book_index, book_selection) in selection.manuscripts.iter().enumerate() {
        let index = indexes
            .get(&book_selection.id)
            .ok_or_else(|| format!("未注册书稿：{}", book_selection.id))?;
        validate_manuscript(index, book_selection)?;
        for (chapter_index, chapter_id) in book_selection.chapters.iter().enumerate() {
            report(
                progress,
                "chapters",
                selected_chapter_pairs.len(),
                selection
                    .manuscripts
                    .iter()
                    .map(|book| book.chapters.len())
                    .sum(),
            )?;
            let route = super::routes::chapter_route(
                selection,
                &book_selection.id,
                chapter_id,
                format!(
                    "manuscripts/m{:04}-c{:04}.html",
                    book_index + 1,
                    chapter_index + 1
                ),
                overrides,
            )?;
            let chapter = index
                .entries
                .iter()
                .find(|entry| entry.id == *chapter_id && entry.kind == ManuscriptEntryKind::Chapter)
                .expect("selected chapters were validated");
            selected_chapter_pairs.insert((book_selection.id.clone(), chapter_id.clone()));
            let mut body_html = String::new();
            let mut searchable_text = String::new();
            if let Some(summary) = &chapter.summary {
                body_html.push_str(&format!("<p>{}</p>", html_escape(summary)));
                searchable_text.push_str(summary);
            }
            if let Some(target) = &chapter.target_ref {
                if let Some(target_route) = routes.get(target) {
                    let href = relative_url(&route, target_route);
                    body_html.push_str(&format!(
                        "<p><a href=\"{}\">阅读已公开的来源内容</a></p>",
                        html_escape(&href)
                    ));
                    searchable_text.push_str(" 阅读已公开的来源内容");
                    if selection.schema_version == READER_SITE_SCHEMA_VERSION {
                        let (html, plain) = super::render::render_object_body_at(
                            &compiled,
                            target,
                            &routes,
                            &file_routes,
                            selection,
                            &route,
                        )?;
                        body_html.push_str(&html);
                        searchable_text.push_str(&plain);
                    }
                } else {
                    body_html.push_str("<p class=\"unavailable\">未公开内容</p>");
                    searchable_text.push_str(" 未公开内容");
                }
            }
            if selection.schema_version == READER_SITE_SCHEMA_VERSION {
                for (label, next_index) in [
                    ("上一章", chapter_index.checked_sub(1)),
                    (
                        "下一章",
                        (chapter_index + 1 < book_selection.chapters.len())
                            .then_some(chapter_index + 1),
                    ),
                ] {
                    if let Some(next_index) = next_index {
                        let next = super::routes::chapter_route(
                            selection,
                            &book_selection.id,
                            &book_selection.chapters[next_index],
                            format!(
                                "manuscripts/m{:04}-c{:04}.html",
                                book_index + 1,
                                next_index + 1
                            ),
                            overrides,
                        )?;
                        body_html.push_str(&format!(
                            "<p><a href=\"{}\">{label}</a></p>",
                            html_escape(&relative_url(&route, next))
                        ));
                    }
                }
            }
            let empty_content = searchable_text.trim().is_empty();
            pages.push(PublicPage {
                title: chapter.title.clone(),
                output_path: PathBuf::from(&route),
                body_html,
                searchable_text,
                kind: "chapter".into(),
                aliases: Vec::new(),
                anchors: Vec::new(),
                empty_content,
            });
            check_budget(&pages, &attachments)?;
            included.push(ReaderExportIncluded {
                target: None,
                manuscript_id: Some(book_selection.id.clone()),
                chapter_id: Some(chapter_id.clone()),
                title: chapter.title.clone(),
                output_path: route,
            });
        }
    }

    super::maps::append_maps(
        super::maps::MapExportContext {
            project,
            compiled: &compiled,
            selection,
            routes: &routes,
            overrides,
        },
        &mut pages,
        &mut included,
        progress,
    )?;
    if selection.schema_version == READER_SITE_SCHEMA_VERSION {
        super::semantic_pages::append_world_pages(&compiled, selection, &routes, &mut pages)?;
        super::semantics::append_map_backlinks(project, selection, &routes, overrides, &mut pages)?;
    }
    super::routes::validate_unique_paths(&pages, &attachments)?;
    check_budget(&pages, &attachments)?;

    let selected_asset_paths: BTreeSet<_> = attachments
        .iter()
        .map(|attachment| attachment.source_path.clone())
        .collect();
    let exclusions = build_exclusions(ExclusionInput {
        compiled: &compiled,
        indexes: &indexes,
        selected_objects: &selected_objects,
        selected_chapters: &selected_chapter_pairs,
        selected_assets: &selected_asset_ids,
        selected_asset_paths: &selected_asset_paths,
        workspace_paths: &workspace_paths,
        project,
    });
    let content_baseline = project.content_baseline();
    let mut plan_digest = digest_plan(selection, &content_baseline, &attachments)?;
    // Bind rendered map content too, including unsaved presentation changes.
    let mut map_hash = 0xcbf29ce484222325u64;
    // clone 编译可能补载原 Project 尚未跟踪的 include；必须绑定实际消费的源码。
    for (path, source) in &compiled.sources {
        let relative = path
            .strip_prefix(&project.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        mix(&mut map_hash, relative.as_bytes());
        mix(&mut map_hash, source.as_bytes());
    }
    for page in &pages {
        mix(&mut map_hash, page.title.as_bytes());
        mix(&mut map_hash, page.body_html.as_bytes());
        mix(&mut map_hash, page.searchable_text.as_bytes());
        mix(&mut map_hash, page.kind.as_bytes());
        for alias in &page.aliases {
            mix(&mut map_hash, alias.as_bytes());
        }
        for anchor in &page.anchors {
            mix(&mut map_hash, anchor.id.as_bytes());
            mix(&mut map_hash, anchor.label.as_bytes());
            mix(&mut map_hash, anchor.text.as_bytes());
        }
        mix(
            &mut map_hash,
            super::portable_output_path(&page.output_path)?.as_bytes(),
        );
    }
    plan_digest.push_str(&format!("-{map_hash:016x}"));
    let preview = ReaderExportPreview {
        content: if selection.schema_version >= READER_FIELDS_SCHEMA_VERSION {
            pages
                .iter()
                .map(|page| {
                    Ok(ReaderContentPreview {
                        title: page.title.clone(),
                        output_path: super::portable_output_path(&page.output_path)?,
                        text: page.searchable_text.clone(),
                        empty_content: page.empty_content,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?
        } else {
            Vec::new()
        },
        schema_version: selection.schema_version,
        plan_digest,
        content_baseline,
        included,
        exclusions,
    };
    Ok(PreparedExport {
        preview,
        pages,
        attachments,
        site_title: selection.site_title.clone(),
        world_site: selection.schema_version == READER_SITE_SCHEMA_VERSION,
    })
}

fn validate_selection(selection: &ReaderExportSelection) -> Result<(), String> {
    if !matches!(
        selection.schema_version,
        READER_EXPORT_SCHEMA_VERSION | READER_FIELDS_SCHEMA_VERSION | READER_SITE_SCHEMA_VERSION
    ) {
        return Err("不支持的阅读包选择版本".into());
    }
    super::fields::validate_version(selection)?;
    if selection.site_title.trim().is_empty() || selection.site_title.chars().count() > 160 {
        return Err("站点标题必须为 1 至 160 个字符".into());
    }
    let object_limit = if selection.schema_version == READER_SITE_SCHEMA_VERSION {
        MAX_SITE_OBJECTS
    } else {
        MAX_OBJECTS
    };
    if selection.objects.len() > object_limit
        || selection.manuscripts.len() > MAX_MANUSCRIPTS
        || selection.attachments.len() > MAX_ATTACHMENTS
    {
        return Err("阅读包选择超过数量限制".into());
    }
    if selection.maps.len() > 100
        || selection
            .maps
            .iter()
            .any(|map| map.placements.len() > 5000 || map.raster_layers.len() > 128)
    {
        return Err("公开地图选择超过数量限制".into());
    }
    let mut selected_chapters = 0usize;
    for manuscript in &selection.manuscripts {
        selected_chapters = selected_chapters.saturating_add(manuscript.chapters.len());
        if selected_chapters > MAX_CHAPTERS {
            return Err("公开章节数量超出 5000 限制".into());
        }
    }
    if has_duplicates(selection.objects.iter())
        || has_duplicates(selection.manuscripts.iter().map(|book| &book.id))
        || has_duplicates(selection.attachments.iter())
        || selection
            .manuscripts
            .iter()
            .any(|book| has_duplicates(book.chapters.iter()))
    {
        return Err("阅读包选择不能包含重复对象、书稿、章节或附件".into());
    }
    if selection
        .manuscripts
        .iter()
        .any(|book| book.chapters.is_empty())
    {
        return Err("公开书稿必须至少选择一个章节".into());
    }
    Ok(())
}

fn has_duplicates<'a, T: Ord + 'a>(items: impl Iterator<Item = &'a T>) -> bool {
    let mut seen = BTreeSet::new();
    items.into_iter().any(|item| !seen.insert(item))
}

pub(super) fn public_object_kind(kind: &str) -> bool {
    matches!(
        kind,
        "event"
            | "fragment"
            | "rule"
            | "scene"
            | "character"
            | "entity"
            | "world"
            | "storyline"
            | "period"
            | "anchor"
            | "state"
            | "tag"
            | "relation"
            | "variable"
    )
}

fn validate_manuscript(
    index: &ManuscriptIndex,
    selection: &ReaderManuscriptSelection,
) -> Result<(), String> {
    if index.read_only || !index.diagnostics.is_empty() {
        return Err(format!(
            "书稿 `{}` 存在只读或结构诊断，不能公开",
            selection.id
        ));
    }
    if index.id.as_deref() != Some(selection.id.as_str()) {
        return Err("书稿注册 ID 与文档 ID 不一致".into());
    }
    for chapter_id in &selection.chapters {
        let chapter = index
            .entries
            .iter()
            .find(|entry| entry.id == *chapter_id && entry.kind == ManuscriptEntryKind::Chapter)
            .ok_or_else(|| format!("书稿章节不存在：{chapter_id}"))?;
        if chapter.target_ref.is_some()
            && chapter
                .source
                .as_ref()
                .is_none_or(|source| source.status != ManuscriptReferenceStatus::Resolved)
        {
            return Err(format!("书稿章节 `{chapter_id}` 的来源不可解析"));
        }
    }
    Ok(())
}

fn digest_plan(
    selection: &ReaderExportSelection,
    baseline: &str,
    attachments: &[PublicAttachment],
) -> Result<String, String> {
    let mut hash = 0xcbf29ce484222325u64;
    mix(&mut hash, b"worldline-reader-export-v1");
    mix(&mut hash, baseline.as_bytes());
    mix(
        &mut hash,
        &serde_json::to_vec(selection).expect("reader selection serializes"),
    );
    for attachment in attachments {
        mix(&mut hash, attachment.id.as_bytes());
        mix(&mut hash, attachment.display.as_bytes());
        mix(
            &mut hash,
            super::portable_output_path(&attachment.output_path)?.as_bytes(),
        );
        mix(&mut hash, &attachment.bytes);
    }
    Ok(format!("reader-v1-{hash:016x}"))
}

fn mix(hash: &mut u64, bytes: &[u8]) {
    for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

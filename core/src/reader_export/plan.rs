use super::render::render_object_body;
use super::site::{html_escape, relative_url, render_package, validate_output_path};
use super::*;
use crate::catalog::AssetInfo;
use crate::manuscript::{ManuscriptEntryKind, ManuscriptReferenceStatus};
use std::path::Path;

impl Project {
    /// 只读预览当前缓冲中的显式公开选择，不刷新或修改 Project。
    pub fn preview_reader_export(
        &self,
        selection: &ReaderExportSelection,
    ) -> Result<ReaderExportPreview, String> {
        Ok(prepare(self, selection)?.preview)
    }

    /// 按预览摘要生成站点文件；路径、正文和索引都只来自显式授权内容。
    pub fn build_reader_export(
        &self,
        selection: &ReaderExportSelection,
        expected_plan_digest: &str,
    ) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
        let prepared = prepare(self, selection)?;
        if prepared.preview.plan_digest != expected_plan_digest {
            return Err("阅读包预览已过期，请重新预览并核对选择".into());
        }
        render_package(prepared)
    }

    /// 原子发布到一个新目录；失败时不覆盖已存在的目标。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn export_reader_site(
        &self,
        selection: &ReaderExportSelection,
        expected_plan_digest: &str,
        destination: &Path,
    ) -> Result<(), String> {
        let files = self.build_reader_export(selection, expected_plan_digest)?;
        let destination = crate::compiler::source_path(destination);
        if output_entry_exists(&destination)? {
            return Err("导出目标已存在，请选择新的文件夹名称".into());
        }
        if destination.starts_with(crate::compiler::source_path(&self.root)) {
            return Err("阅读包目标必须位于当前工作区之外".into());
        }
        let parent = destination.parent().ok_or("导出目录缺少父目录")?;
        if !parent.is_dir() {
            return Err("阅读包目标的父目录必须已存在".into());
        }

        static NEXT_STAGE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let mut staging = None;
        for _ in 0..100 {
            let sequence = NEXT_STAGE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let candidate = parent.join(format!(
                ".worldline-reader-export-{}-{sequence}",
                std::process::id()
            ));
            match std::fs::create_dir(&candidate) {
                Ok(()) => {
                    staging = Some(candidate);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(format!("无法建立阅读包暂存目录：{error}")),
            }
        }
        let staging = staging.ok_or("无法分配唯一的阅读包暂存目录")?;
        let write_result = (|| -> std::io::Result<()> {
            for (relative, bytes) in files {
                validate_output_path(&relative).map_err(std::io::Error::other)?;
                let target = staging.join(relative);
                std::fs::create_dir_all(
                    target.parent().expect("validated output path has parent"),
                )?;
                std::fs::write(target, bytes)?;
            }
            match std::fs::symlink_metadata(&destination) {
                Ok(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "导出目标已存在",
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            std::fs::rename(&staging, &destination)
        })();
        if let Err(error) = write_result {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("阅读包导出失败：{error}"));
        }
        Ok(())
    }
}

fn prepare(project: &Project, selection: &ReaderExportSelection) -> Result<PreparedExport, String> {
    validate_selection(selection)?;
    #[cfg(not(target_arch = "wasm32"))]
    project.ensure_storage_ready()?;

    // Compile a clone so include loading never mutates the caller's buffers.
    let mut compile_project = project.clone();
    let compiled = compile_project.compile();
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
            output_path: PathBuf::from(format!("assets/a{:04}.{extension}", index + 1)),
            bytes,
        });
    }

    let mut routes = BTreeMap::<TargetRef, String>::new();
    let mut object_order = selection.objects.clone();
    object_order.sort();
    for (index, target) in object_order.iter().enumerate() {
        routes.insert(target.clone(), format!("objects/o{:04}.html", index + 1));
    }
    for attachment in &attachments {
        routes.insert(
            TargetRef::new("asset", &attachment.id),
            attachment.output_path.to_string_lossy().into_owned(),
        );
        included.push(ReaderExportIncluded {
            target: Some(TargetRef::new("asset", &attachment.id)),
            manuscript_id: None,
            chapter_id: None,
            title: attachment.display.clone(),
            output_path: attachment.output_path.to_string_lossy().into_owned(),
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

    let mut pages = Vec::new();
    for target in object_order {
        let route = routes.get(&target).expect("selected object route").clone();
        let object = compiled
            .analysis
            .catalog
            .object(&target)
            .expect("selection was validated");
        let (body_html, searchable_text) =
            render_object_body(&compiled, &target, &routes, &file_routes)?;
        pages.push(PublicPage {
            title: object.display.clone(),
            output_path: PathBuf::from(&route),
            body_html,
            searchable_text,
        });
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
            let route = format!(
                "manuscripts/m{:04}-c{:04}.html",
                book_index + 1,
                chapter_index + 1
            );
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
                } else {
                    body_html.push_str("<p class=\"unavailable\">未公开内容</p>");
                    searchable_text.push_str(" 未公开内容");
                }
            }
            pages.push(PublicPage {
                title: chapter.title.clone(),
                output_path: PathBuf::from(&route),
                body_html,
                searchable_text,
            });
            included.push(ReaderExportIncluded {
                target: None,
                manuscript_id: Some(book_selection.id.clone()),
                chapter_id: Some(chapter_id.clone()),
                title: chapter.title.clone(),
                output_path: route,
            });
        }
    }

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
    let plan_digest = digest_plan(selection, &content_baseline, &attachments);
    let preview = ReaderExportPreview {
        schema_version: READER_EXPORT_SCHEMA_VERSION,
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
    })
}

fn validate_selection(selection: &ReaderExportSelection) -> Result<(), String> {
    if selection.schema_version != READER_EXPORT_SCHEMA_VERSION {
        return Err("不支持的阅读包选择版本".into());
    }
    if selection.site_title.trim().is_empty() || selection.site_title.chars().count() > 160 {
        return Err("站点标题必须为 1 至 160 个字符".into());
    }
    if selection.objects.len() > MAX_OBJECTS
        || selection.manuscripts.len() > MAX_MANUSCRIPTS
        || selection.attachments.len() > MAX_ATTACHMENTS
    {
        return Err("阅读包选择超过数量限制".into());
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

fn public_object_kind(kind: &str) -> bool {
    matches!(
        kind,
        "event"
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

fn read_public_asset(
    project: &Project,
    asset: &AssetInfo,
    workspace_paths: &[PathBuf],
) -> Result<(String, Vec<u8>, PathBuf), String> {
    if !asset.available {
        return Err(format!("附件不可用：{}", asset.id));
    }
    let source = Path::new(&asset.resolved_path);
    let source = crate::file_access::within(&project.root, source)
        .map_err(|_| format!("附件必须位于当前工作区内：{}", asset.id))?;
    if !workspace_paths.contains(&source) {
        return Err(format!("附件不在当前工作区快照中：{}", asset.id));
    }
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    // No HTML, SVG, script, CSS, or other active-content formats are copied.
    if !matches!(
        extension.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "gif"
            | "bmp"
            | "wav"
            | "mp3"
            | "ogg"
            | "flac"
            | "m4a"
            | "aac"
            | "mp4"
            | "webm"
    ) {
        return Err(format!("附件格式不在静态阅读包白名单中：{}", asset.id));
    }
    let bytes = crate::file_access::read_limited(&source, MAX_ATTACHMENT_BYTES)
        .map_err(|error| format!("附件不可读或超过 16 MiB 限制：{} ({error})", asset.id))?;
    Ok((extension, bytes, source))
}

#[cfg(not(target_arch = "wasm32"))]
fn output_entry_exists(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("无法检查导出目标：{error}")),
    }
}

fn digest_plan(
    selection: &ReaderExportSelection,
    baseline: &str,
    attachments: &[PublicAttachment],
) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    mix(&mut hash, b"worldline-reader-export-v1");
    mix(&mut hash, baseline.as_bytes());
    mix(
        &mut hash,
        &serde_json::to_vec(selection).expect("reader selection serializes"),
    );
    for attachment in attachments {
        mix(&mut hash, attachment.id.as_bytes());
        mix(&mut hash, &attachment.bytes);
    }
    format!("reader-v1-{hash:016x}")
}

fn mix(hash: &mut u64, bytes: &[u8]) {
    for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn build_exclusions(input: ExclusionInput<'_>) -> Vec<ReaderExportExclusion> {
    let mut exclusions = Vec::new();
    for object in &input.compiled.analysis.catalog.objects {
        if !input.selected_objects.contains(&object.target) {
            exclusions.push(ReaderExportExclusion {
                target: Some(object.target.clone()),
                manuscript_id: None,
                chapter_id: None,
                source_path: Some(object.file.clone()),
                reason_code: "target_not_selected".into(),
            });
        }
    }
    for (id, asset) in &input.compiled.analysis.catalog.assets {
        if !input.selected_assets.contains(id) {
            exclusions.push(ReaderExportExclusion {
                target: Some(TargetRef::new("asset", id)),
                manuscript_id: None,
                chapter_id: None,
                source_path: Some(asset.resolved_path.clone()),
                reason_code: "attachment_not_selected".into(),
            });
        }
    }
    for (book_id, index) in input.indexes {
        for entry in &index.entries {
            if entry.kind == ManuscriptEntryKind::Chapter
                && !input
                    .selected_chapters
                    .contains(&(book_id.clone(), entry.id.clone()))
            {
                exclusions.push(ReaderExportExclusion {
                    target: None,
                    manuscript_id: Some(book_id.clone()),
                    chapter_id: Some(entry.id.clone()),
                    source_path: None,
                    reason_code: "chapter_not_selected".into(),
                });
            }
        }
    }
    for path in input.workspace_paths {
        let canonical = crate::compiler::source_path(path);
        if input.selected_asset_paths.contains(&canonical) {
            continue;
        }
        let relative = path
            .strip_prefix(&input.project.root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        exclusions.push(ReaderExportExclusion {
            target: None,
            manuscript_id: None,
            chapter_id: None,
            source_path: Some(relative),
            reason_code: "workspace_file_not_selected".into(),
        });
    }
    exclusions.sort_by(|left, right| {
        left.reason_code
            .cmp(&right.reason_code)
            .then(left.source_path.cmp(&right.source_path))
            .then(left.target.cmp(&right.target))
            .then(left.manuscript_id.cmp(&right.manuscript_id))
            .then(left.chapter_id.cmp(&right.chapter_id))
    });
    exclusions
}

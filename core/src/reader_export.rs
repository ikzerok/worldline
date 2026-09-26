//! 显式授权的静态阅读包；与完整工程备份保持独立。

use crate::ast::{Event, Stmt, TextPart};
use crate::catalog::{AssetInfo, TargetRef};
use crate::manuscript::{ManuscriptEntryKind, ManuscriptIndex, ManuscriptReferenceStatus};
use crate::project::Project;
use crate::CompileResult;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

pub const READER_EXPORT_SCHEMA_VERSION: u32 = 1;
const MAX_OBJECTS: usize = 500;
const MAX_MANUSCRIPTS: usize = 100;
const MAX_CHAPTERS: usize = 5_000;
const MAX_ATTACHMENTS: usize = 128;
const MAX_ATTACHMENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_ATTACHMENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_PACKAGE_BYTES: usize = 128 * 1024 * 1024;

/// 单册显式选择；章节数组顺序决定阅读导航顺序。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderManuscriptSelection {
    pub id: String,
    pub chapters: Vec<String>,
}

/// 阅读包的唯一公开边界。引用不会自动扩大选择范围。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderExportSelection {
    pub schema_version: u32,
    pub site_title: String,
    pub objects: Vec<TargetRef>,
    pub manuscripts: Vec<ReaderManuscriptSelection>,
    pub attachments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderExportIncluded {
    pub target: Option<TargetRef>,
    pub manuscript_id: Option<String>,
    pub chapter_id: Option<String>,
    pub title: String,
    pub output_path: String,
}

/// 作者预览中的排除报告；该结构只从 preview API 返回，绝不写入站点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderExportExclusion {
    pub target: Option<TargetRef>,
    pub manuscript_id: Option<String>,
    pub chapter_id: Option<String>,
    pub source_path: Option<String>,
    pub reason_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderExportPreview {
    pub schema_version: u32,
    pub plan_digest: String,
    pub content_baseline: String,
    pub included: Vec<ReaderExportIncluded>,
    pub exclusions: Vec<ReaderExportExclusion>,
}

#[derive(Debug, Clone)]
struct PublicPage {
    title: String,
    output_path: PathBuf,
    body_html: String,
    searchable_text: String,
}

#[derive(Debug, Clone)]
struct PublicAttachment {
    id: String,
    display: String,
    source_path: PathBuf,
    output_path: PathBuf,
    bytes: Vec<u8>,
}

struct PreparedExport {
    preview: ReaderExportPreview,
    pages: Vec<PublicPage>,
    attachments: Vec<PublicAttachment>,
    site_title: String,
}

struct ExclusionInput<'a> {
    compiled: &'a CompileResult,
    indexes: &'a BTreeMap<String, ManuscriptIndex>,
    selected_objects: &'a BTreeSet<TargetRef>,
    selected_chapters: &'a BTreeSet<(String, String)>,
    selected_assets: &'a BTreeSet<String>,
    selected_asset_paths: &'a BTreeSet<PathBuf>,
    workspace_paths: &'a [PathBuf],
    project: &'a Project,
}

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
    let chapter_count: usize = selection
        .manuscripts
        .iter()
        .map(|book| book.chapters.len())
        .sum();
    if chapter_count > MAX_CHAPTERS {
        return Err("公开章节数量超出 5000 限制".into());
    }
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

fn render_object_body(
    compiled: &CompileResult,
    target: &TargetRef,
    routes: &BTreeMap<TargetRef, String>,
    file_routes: &BTreeMap<String, String>,
) -> Result<(String, String), String> {
    let mut html = String::new();
    let mut plain = String::new();
    match target.kind.as_str() {
        "event" => {
            let event = compiled
                .program
                .events
                .iter()
                .find(|event| event.name == target.id)
                .ok_or_else(|| "选中的事件无法读取".to_string())?;
            let (body_html, body_text) = render_statements(
                &event.body,
                &event.name,
                &routes[target],
                compiled,
                routes,
                file_routes,
            );
            html.push_str(&body_html);
            plain.push_str(&body_text);
        }
        "scene" => {
            let (event, body) = find_scene(&compiled.program.events, &target.id)
                .ok_or_else(|| "选中的场景无法读取".to_string())?;
            let (body_html, body_text) = render_statements(
                body,
                &event.name,
                &routes[target],
                compiled,
                routes,
                file_routes,
            );
            html.push_str(&body_html);
            plain.push_str(&body_text);
        }
        "entity" => {
            if let Some(entity) = compiled.analysis.catalog.entities.get(&target.id) {
                append_description(&mut html, &mut plain, &entity.description);
            }
        }
        "world" => {
            if let Some(world) = compiled
                .program
                .worlds
                .iter()
                .find(|item| item.name == target.id)
            {
                append_description(&mut html, &mut plain, &world.description);
            }
        }
        "anchor" => {
            if let Some(anchor) = compiled.analysis.catalog.anchors.get(&target.id) {
                append_description(&mut html, &mut plain, &anchor.description);
            }
        }
        "tag" => {
            if let Some(tag) = compiled.analysis.catalog.tags.get(&target.id) {
                append_description(&mut html, &mut plain, &tag.description);
            }
        }
        "relation" => {
            if let Some(relation) = compiled.analysis.catalog.relations.get(&target.id) {
                append_description(&mut html, &mut plain, &relation.description);
            }
        }
        _ => {}
    }
    if html.is_empty() {
        html.push_str("<p>该对象没有静态阅读正文。</p>");
    }
    Ok((html, plain))
}

fn append_description(html: &mut String, plain: &mut String, description: &str) {
    if !description.trim().is_empty() {
        html.push_str(&format!("<p>{}</p>", html_escape(description)));
        plain.push_str(description);
    }
}

fn find_scene<'a>(events: &'a [Event], target_id: &str) -> Option<(&'a Event, &'a [Stmt])> {
    for event in events {
        if let Some(body) = find_scene_in(&event.body, &event.name, target_id) {
            return Some((event, body));
        }
    }
    None
}

fn find_scene_in<'a>(statements: &'a [Stmt], prefix: &str, target_id: &str) -> Option<&'a [Stmt]> {
    for statement in statements {
        let Stmt::Scene(scene) = statement else {
            continue;
        };
        let full_name = format!("{prefix}.{}", scene.name);
        if full_name == target_id {
            return Some(&scene.body);
        }
        if let Some(body) = find_scene_in(&scene.body, &full_name, target_id) {
            return Some(body);
        }
    }
    None
}

fn render_statements(
    statements: &[Stmt],
    current_event: &str,
    current_path: &str,
    compiled: &CompileResult,
    routes: &BTreeMap<TargetRef, String>,
    file_routes: &BTreeMap<String, String>,
) -> (String, String) {
    let mut html = String::new();
    let mut plain = String::new();
    for statement in statements {
        match statement {
            Stmt::Text(text) => {
                let (text_html, text_plain) =
                    render_parts(&text.parts, current_path, routes, file_routes);
                if !text_html.is_empty() {
                    html.push_str(&format!("<p>{text_html}</p>"));
                    plain.push_str(&text_plain);
                    plain.push('\n');
                }
            }
            Stmt::Choice(choice) => {
                let (label_html, label_plain) =
                    render_parts(&choice.label, current_path, routes, file_routes);
                html.push_str(&format!("<div class=\"choice\">{label_html}</div>"));
                plain.push_str(&label_plain);
                plain.push('\n');
                let (body_html, body_text) = render_statements(
                    &choice.body,
                    current_event,
                    current_path,
                    compiled,
                    routes,
                    file_routes,
                );
                html.push_str(&body_html);
                plain.push_str(&body_text);
            }
            Stmt::If(condition) => {
                // Conditions are intentionally not evaluated or disclosed in a reader package.
                for (_, branch) in &condition.branches {
                    let (branch_html, branch_text) = render_statements(
                        branch,
                        current_event,
                        current_path,
                        compiled,
                        routes,
                        file_routes,
                    );
                    html.push_str(&branch_html);
                    plain.push_str(&branch_text);
                }
            }
            Stmt::Divert(divert) => {
                if let crate::ast::DivertTarget::Node(name) = &divert.target {
                    if let Some(node) = compiled
                        .analysis
                        .symbols
                        .resolve_target(name, Some(current_event))
                    {
                        let event = &compiled.program.events[node.event];
                        let target = TargetRef::new(
                            if node.scenes.is_empty() {
                                "event"
                            } else {
                                "scene"
                            },
                            &node.full_name(&event.name),
                        );
                        if let Some(destination) = routes.get(&target) {
                            let href = relative_url(current_path, destination);
                            html.push_str(&format!(
                                "<p><a href=\"{}\">继续阅读</a></p>",
                                html_escape(&href)
                            ));
                            plain.push_str("继续阅读\n");
                        } else {
                            html.push_str("<p class=\"unavailable\">未公开内容</p>");
                            plain.push_str("未公开内容\n");
                        }
                    } else {
                        html.push_str("<p class=\"unavailable\">未公开内容</p>");
                        plain.push_str("未公开内容\n");
                    }
                }
            }
            Stmt::Scene(scene) => {
                let (body_html, body_text) = render_statements(
                    &scene.body,
                    current_event,
                    current_path,
                    compiled,
                    routes,
                    file_routes,
                );
                html.push_str(&body_html);
                plain.push_str(&body_text);
            }
            Stmt::Let(_) | Stmt::Set(_) | Stmt::Change(_) | Stmt::Anchor(_) | Stmt::Effect(_) => {}
        }
    }
    (html, plain)
}

fn render_parts(
    parts: &[TextPart],
    current_path: &str,
    routes: &BTreeMap<TargetRef, String>,
    file_routes: &BTreeMap<String, String>,
) -> (String, String) {
    let mut html = String::new();
    let mut plain = String::new();
    for part in parts {
        match part {
            TextPart::Str(text) => {
                html.push_str(&html_escape(text));
                plain.push_str(text);
            }
            TextPart::Expr(_) => {
                html.push_str("（动态内容略）");
                plain.push_str("（动态内容略）");
            }
            TextPart::Link(link) => {
                let route = if link.target.kind == "file" {
                    let path = crate::compiler::source_path(Path::new(&link.target.id));
                    file_routes.get(&path.to_string_lossy().into_owned())
                } else {
                    routes.get(&link.target)
                };
                if let Some(route) = route {
                    let href = relative_url(current_path, route);
                    html.push_str(&format!(
                        "<a href=\"{}\">{}</a>",
                        html_escape(&href),
                        html_escape(&link.label)
                    ));
                    plain.push_str(&link.label);
                } else {
                    html.push_str("<span class=\"unavailable\">未公开内容</span>");
                    plain.push_str("未公开内容");
                }
            }
        }
    }
    (html, plain)
}

fn render_package(prepared: PreparedExport) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    #[derive(Serialize)]
    struct PublicSearchEntry<'a> {
        title: &'a str,
        url: String,
        text: &'a str,
    }
    #[derive(Serialize)]
    struct PublicManifestEntry<'a> {
        title: &'a str,
        url: String,
    }
    #[derive(Serialize)]
    struct PublicManifest<'a> {
        schema_version: u32,
        title: &'a str,
        pages: Vec<PublicManifestEntry<'a>>,
        attachments: Vec<PublicManifestEntry<'a>>,
    }

    let mut files = BTreeMap::new();
    let public_pages: Vec<_> = prepared
        .pages
        .iter()
        .map(|page| PublicManifestEntry {
            title: &page.title,
            url: page.output_path.to_string_lossy().into_owned(),
        })
        .collect();
    let public_attachments: Vec<_> = prepared
        .attachments
        .iter()
        .map(|attachment| PublicManifestEntry {
            title: &attachment.display,
            url: attachment.output_path.to_string_lossy().into_owned(),
        })
        .collect();
    let manifest = PublicManifest {
        schema_version: READER_EXPORT_SCHEMA_VERSION,
        title: &prepared.site_title,
        pages: public_pages,
        attachments: public_attachments,
    };
    insert_output(
        &mut files,
        "reader-manifest.json",
        json_for_script(&manifest)?.into_bytes(),
    )?;

    let mut search_entries = Vec::new();
    for page in &prepared.pages {
        search_entries.push(PublicSearchEntry {
            title: &page.title,
            url: page.output_path.to_string_lossy().into_owned(),
            text: &page.searchable_text,
        });
    }
    let search_json = json_for_script(&search_entries)?;
    insert_output(
        &mut files,
        "search-index.json",
        search_json.clone().into_bytes(),
    )?;
    let js_data = json_for_script(&search_entries)?;
    insert_output(
        &mut files,
        "search-data.js",
        format!("window.READER_SEARCH_DATA={js_data};\n").into_bytes(),
    )?;
    insert_output(&mut files, "reader.js", READER_JS.as_bytes().to_vec())?;
    insert_output(&mut files, "style.css", READER_CSS.as_bytes().to_vec())?;

    for page in &prepared.pages {
        let path = page.output_path.to_string_lossy();
        let stylesheet = relative_url(path.as_ref(), "style.css");
        let home = relative_url(path.as_ref(), "index.html");
        let search = relative_url(path.as_ref(), "search.html");
        let html = format!(
            "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><link rel=\"stylesheet\" href=\"{}\"></head><body><header><a href=\"{}\">{}</a> · <a href=\"{}\">搜索</a></header><main><h1>{}</h1>{}</main></body></html>",
            html_escape(&page.title),
            html_escape(&stylesheet),
            html_escape(&home),
            html_escape(&prepared.site_title),
            html_escape(&search),
            html_escape(&page.title),
            page.body_html
        );
        insert_output(&mut files, &page.output_path, html.into_bytes())?;
    }

    let object_pages: Vec<_> = prepared
        .pages
        .iter()
        .filter(|page| page.output_path.starts_with("objects"))
        .collect();
    let manuscript_pages: Vec<_> = prepared
        .pages
        .iter()
        .filter(|page| page.output_path.starts_with("manuscripts"))
        .collect();
    let attachments_html = prepared
        .attachments
        .iter()
        .map(|attachment| {
            let path = attachment.output_path.to_string_lossy();
            format!(
                "<li><a href=\"{}\">{}</a></li>",
                html_escape(&path),
                html_escape(&attachment.display)
            )
        })
        .collect::<String>();
    let index_html = format!(
        "{}<h1>{}</h1>{}<h2>资料对象</h2>{}<h2>书稿章节</h2>{}<h2>附件</h2><ul>{}</ul>",
        page_start(&prepared.site_title, "index.html", false),
        html_escape(&prepared.site_title),
        search_link(),
        page_links(&object_pages, "index.html"),
        page_links(&manuscript_pages, "index.html"),
        attachments_html
    );
    insert_output(&mut files, "index.html", page_end(index_html).into_bytes())?;
    insert_output(
        &mut files,
        "objects/index.html",
        page_end(format!(
            "{}<h1>资料对象</h1>{}",
            page_start(&prepared.site_title, "objects/index.html", true),
            page_links(&object_pages, "objects/index.html")
        ))
        .into_bytes(),
    )?;
    insert_output(
        &mut files,
        "manuscripts/index.html",
        page_end(format!(
            "{}<h1>书稿章节</h1>{}",
            page_start(&prepared.site_title, "manuscripts/index.html", true),
            page_links(&manuscript_pages, "manuscripts/index.html")
        ))
        .into_bytes(),
    )?;
    insert_output(
        &mut files,
        "search.html",
        page_end(format!(
            "{}<h1>搜索公开内容</h1><label>搜索 <input id=\"query\" type=\"search\" autocomplete=\"off\"></label><ul id=\"results\"></ul><script src=\"search-data.js\"></script><script src=\"reader.js\"></script>",
            page_start(&prepared.site_title, "search.html", false)
        ))
        .into_bytes(),
    )?;

    for attachment in prepared.attachments {
        insert_output(&mut files, attachment.output_path, attachment.bytes)?;
    }
    let total_bytes: usize = files.values().map(Vec::len).sum();
    if total_bytes > MAX_PACKAGE_BYTES {
        return Err("阅读包超过 128 MiB 限制".into());
    }
    Ok(files)
}

fn page_start(site_title: &str, current_path: &str, nested_index: bool) -> String {
    let css = if nested_index {
        "../style.css"
    } else {
        "style.css"
    };
    let home = if nested_index {
        "../index.html"
    } else {
        "index.html"
    };
    format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><link rel=\"stylesheet\" href=\"{}\"></head><body><header><a href=\"{}\">{}</a> · <a href=\"{}\">搜索</a></header><main>",
        html_escape(site_title),
        css,
        home,
        html_escape(site_title),
        relative_url(current_path, "search.html")
    )
}

fn page_end(body: String) -> String {
    format!("{body}</main></body></html>")
}

fn search_link() -> &'static str {
    "<p><a href=\"search.html\">搜索公开内容</a></p>"
}

fn page_links(pages: &[&PublicPage], from_path: &str) -> String {
    let items = pages
        .iter()
        .map(|page| {
            format!(
                "<li><a href=\"{}\">{}</a></li>",
                html_escape(&relative_url(from_path, &page.output_path,)),
                html_escape(&page.title)
            )
        })
        .collect::<String>();
    format!("<ul>{items}</ul>")
}

fn json_for_script<T: Serialize>(value: &T) -> Result<String, String> {
    let json = serde_json::to_string(value).map_err(|error| error.to_string())?;
    Ok(json
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029"))
}

fn insert_output(
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
    path: impl Into<PathBuf>,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let path = path.into();
    validate_output_path(&path)?;
    if files.insert(path.clone(), bytes).is_some() {
        return Err(format!("阅读包内部输出路径冲突：{}", path.display()));
    }
    Ok(())
}

fn validate_output_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!("阅读包输出路径无效：{}", path.display()));
    }
    Ok(())
}

fn relative_url(from_file: impl AsRef<Path>, to_file: impl AsRef<Path>) -> String {
    let from_parent: Vec<_> = from_file
        .as_ref()
        .parent()
        .unwrap_or(Path::new(""))
        .components()
        .filter_map(|part| match part {
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let target: Vec<_> = to_file
        .as_ref()
        .components()
        .filter_map(|part| match part {
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let common = from_parent
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = vec!["..".to_string(); from_parent.len() - common];
    parts.extend(target.into_iter().skip(common));
    parts.join("/")
}

fn html_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

const READER_CSS: &str = "body{font:1rem/1.7 system-ui,sans-serif;max-width:68rem;margin:0 auto;padding:1rem;color:#222}header{border-bottom:1px solid #ddd;padding:.5rem 0}main{padding:1rem 0}.unavailable{color:#666;font-style:italic}.choice{margin:.5rem 0;padding:.5rem;border-left:3px solid #bbb}a{color:#174ea6}";

const READER_JS: &str = r#"(() => {
const input = document.getElementById('query');
const list = document.getElementById('results');
if (!input || !list) return;
const entries = window.READER_SEARCH_DATA || [];
input.addEventListener('input', () => {
  const query = input.value.trim().toLocaleLowerCase();
  list.replaceChildren();
  if (!query) return;
  for (const entry of entries) {
    if (!(entry.title + ' ' + entry.text).toLocaleLowerCase().includes(query)) continue;
    const item = document.createElement('li');
    const link = document.createElement('a');
    link.href = entry.url;
    link.textContent = entry.title;
    const excerpt = document.createElement('p');
    excerpt.textContent = entry.text;
    item.append(link, excerpt);
    list.append(item);
  }
});
})();
"#;

use super::*;
use crate::catalog::AssetInfo;
use crate::manuscript::ManuscriptEntryKind;
use std::path::Path;

pub(super) fn allowed_asset_extension(extension: &str) -> bool {
    matches!(
        extension,
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
    )
}

pub(super) fn read_public_asset(
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
    if !allowed_asset_extension(&extension) {
        return Err(format!("附件格式不在静态阅读包白名单中：{}", asset.id));
    }
    let bytes = crate::file_access::read_limited(&source, MAX_ATTACHMENT_BYTES)
        .map_err(|error| format!("附件不可读或超过 16 MiB 限制：{} ({error})", asset.id))?;
    Ok((extension, bytes, source))
}

pub(super) fn build_exclusions(input: ExclusionInput<'_>) -> Vec<ReaderExportExclusion> {
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

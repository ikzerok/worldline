use super::*;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
fn native_path_is_read_only(path: &Path) -> Result<bool, String> {
    let mut current = path;
    loop {
        match fs::symlink_metadata(current) {
            Ok(metadata) => return Ok(metadata.permissions().readonly()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                current = current
                    .parent()
                    .ok_or_else(|| format!("无法检查恢复路径权限：{}", path.display()))?;
            }
            Err(error) => {
                return Err(format!(
                    "无法检查恢复路径权限 {}：{error}",
                    current.display()
                ));
            }
        }
    }
}
#[derive(Clone, Debug)]
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct RestoreFile {
    pub(super) relative: PathBuf,
    pub(super) before: Option<Vec<u8>>,
    pub(super) after: Option<Vec<u8>>,
}
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn files_to_pending(current: &Files, target: &Files) -> Vec<RestoreFile> {
    differing_paths(current, target)
        .into_iter()
        .map(|relative| RestoreFile {
            before: current.get(&relative).cloned(),
            after: target.get(&relative).cloned(),
            relative,
        })
        .collect()
}
pub(super) fn differing_paths(current: &Files, target: &Files) -> BTreeSet<PathBuf> {
    current
        .keys()
        .chain(target.keys())
        .filter(|relative| current.get(*relative) != target.get(*relative))
        .cloned()
        .collect()
}
pub(super) fn ensure_restore_targets_writable(
    project: &Project,
    checkpoint: &Files,
    changes: &[CheckpointFileChange],
    disk_changes: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    let manifest = checkpoint.get(Path::new(".world/project.json"));
    let target_registry = manifest
        .map(|bytes| crate::workspace_documents::parse_registry(&project.root, bytes))
        .unwrap_or_default();
    let mut paths = disk_changes.clone();
    paths.extend(changes.iter().map(|change| change.path.clone()));
    for relative in paths {
        let absolute = crate::compiler::source_path(&project.root.join(&relative));
        if project
            .authoring_documents
            .get(&absolute)
            .is_some_and(crate::workspace_documents::AuthoringDocument::is_read_only)
            || target_registry.read_only(&absolute)
            || checkpoint
                .get(&relative)
                .is_some_and(|bytes| crate::workspace_documents::document_read_only(bytes, false))
        {
            return Err(format!(
                "只读文件不能通过检查点恢复：{}",
                relative.display()
            ));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if native_path_is_read_only(&project.root.join(&relative))? {
            return Err(format!(
                "只读文件或目录不能通过检查点恢复：{}",
                relative.display()
            ));
        }
    }
    Ok(())
}

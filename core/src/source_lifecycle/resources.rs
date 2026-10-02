use crate::project::Project;
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
mod open;
const MAX_RESOURCE_BYTES: usize = 64 * 1024 * 1024;

/// 素材解析优先当前已应用的两类缓冲；删除墓碑不能回读旧磁盘内容。
pub(super) fn resource_bytes(project: &Project, path: &Path) -> Result<Vec<u8>, String> {
    if let Some(document) = project.documents.get(path) {
        if document.is_deleted() {
            return Err("正式路径指向待删除源码，工程未修改".into());
        }
        return bounded_copy(document.text.as_bytes());
    }
    if let Some(document) = project.authoring_documents.get(path) {
        if document.is_deleted() {
            return Err("素材指向待删除展示文档，工程未修改".into());
        }
        return bounded_copy(document.bytes());
    }
    disk_bytes(project, path)
}

pub(crate) fn disk_bytes(_project: &Project, path: &Path) -> Result<Vec<u8>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        read_regular(_project, path)
            .map_err(|error| format!("路径资源无法安全读取：{}：{error}", path.display()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        crate::file_access::read_limited(path, MAX_RESOURCE_BYTES)
            .map_err(|error| format!("导入资源无法读取：{error}"))
    }
}

fn bounded_copy(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > MAX_RESOURCE_BYTES {
        return Err("路径资源超过单资源 64 MiB 预算".into());
    }
    Ok(bytes.to_vec())
}

#[cfg(not(target_arch = "wasm32"))]
fn read_regular(project: &Project, path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let rejected = |message: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, message);
    let mut pinned = open::open(&project.root, path, MAX_RESOURCE_BYTES as u64)?;
    let before = pinned.file.metadata()?;
    let original = same_file::Handle::from_file(pinned.file.try_clone()?)?;
    let mut bytes = Vec::new();
    (&mut pinned.file)
        .take(MAX_RESOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_RESOURCE_BYTES {
        return Err(rejected("素材读取时超过预算"));
    }
    let current_pinned = open::open(&project.root, path, MAX_RESOURCE_BYTES as u64)?;
    let current = current_pinned.file.metadata()?;
    let current_identity = same_file::Handle::from_file(current_pinned.file.try_clone()?)?;
    if original != current_identity
        || before.len() != current.len()
        || before.modified()? != current.modified()?
    {
        return Err(rejected("素材身份或内容在读取时变化"));
    }
    Ok(bytes)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn held_directory_walk_rejects_external_parent_before_content_read() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("wl-resource-parent-{}", std::process::id()));
        let external =
            std::env::temp_dir().join(format!("wl-resource-external-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&external).unwrap();
        std::fs::write(external.join("canary.txt"), "synthetic outside canary").unwrap();
        let project = Project::new(&root);
        symlink(&external, root.join("swapped")).unwrap();
        assert!(disk_bytes(&project, &root.join("swapped/canary.txt")).is_err());
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(external).unwrap();
    }
}

//! 源文件与素材读取；浏览器只读取用户显式导入的文件，桌面仍读取磁盘。
use std::path::Path;

/// 枚举整个工作区；链接不属于可移植工程，拒绝跟随。
pub fn workspace_files(root: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut files = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory)? {
                let path = entry?.path();
                let metadata = std::fs::symlink_metadata(&path)?;
                #[cfg(windows)]
                let linked = {
                    use std::os::windows::fs::MetadataExt;
                    metadata.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let linked = metadata.file_type().is_symlink();
                if linked || !crate::compiler::source_path(&path).starts_with(root) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!("工作区不支持链接或目录外文件：{}", path.display()),
                    ));
                }
                if metadata.is_dir() {
                    pending.push(path);
                } else if metadata.is_file() {
                    files.push(path);
                }
            }
        }
        files.sort();
        Ok(files)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Ok(FILES.with(|files| {
            files
                .borrow()
                .keys()
                .filter(|p| p.starts_with(root))
                .cloned()
                .collect()
        }))
    }
}

pub fn within(root: &Path, path: &Path) -> Result<std::path::PathBuf, String> {
    let path = crate::compiler::source_path(path);
    if !path.starts_with(crate::compiler::source_path(root)) || path == root {
        return Err(format!("文件必须位于工作区目录内：{}", path.display()));
    }
    Ok(path)
}

#[cfg(not(target_arch = "wasm32"))]
pub use std::fs::{read, read_to_string};

pub fn readable(path: &Path) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        path.is_file() && std::fs::File::open(path).is_ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        FILES.with(|files| {
            files
                .borrow()
                .contains_key(&crate::compiler::source_path(path))
        })
    }
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static FILES: std::cell::RefCell<std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>> = Default::default();
}

/// 替换浏览器已授权的工程文件集合，不访问网络或宿主磁盘。
#[cfg(target_arch = "wasm32")]
pub fn mount(files: std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>) {
    FILES.with(|current| *current.borrow_mut() = files);
}

#[cfg(target_arch = "wasm32")]
pub fn read(path: impl AsRef<Path>) -> std::io::Result<Vec<u8>> {
    FILES
        .with(|files| {
            files
                .borrow()
                .get(&crate::compiler::source_path(path.as_ref()))
                .cloned()
        })
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "文件尚未导入浏览器"))
}

#[cfg(target_arch = "wasm32")]
pub fn read_to_string(path: impl AsRef<Path>) -> std::io::Result<String> {
    String::from_utf8(read(path)?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

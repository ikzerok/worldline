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
                if is_link_or_junction(&metadata)
                    || !crate::compiler::source_path(&path).starts_with(root)
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!("工作区不支持链接或目录外文件：{}", path.display()),
                    ));
                }
                let relative = path.strip_prefix(root).unwrap_or(&path);
                if is_workspace_state_path(relative, ".transactions")
                    || is_workspace_state_path(relative, ".checkpoints")
                {
                    if relative.components().count() == 2 && !metadata.is_dir() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("工程本地存储路径不是目录：{}", path.display()),
                        ));
                    }
                    // 保存事务与本地检查点是受控存储，不属于作者文件或导出；
                    // 链接形态仍在上面的边界检查中拒绝。
                    continue;
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
                .filter(|p| {
                    p.strip_prefix(root).is_ok_and(|relative| {
                        !is_workspace_state_path(relative, ".transactions")
                            && !is_workspace_state_path(relative, ".checkpoints")
                    })
                })
                .cloned()
                .collect()
        }))
    }
}

fn is_workspace_state_path(path: &Path, store: &str) -> bool {
    let mut components = path.components();
    let (Some(std::path::Component::Normal(world)), Some(std::path::Component::Normal(directory))) =
        (components.next(), components.next())
    else {
        return false;
    };
    fn matches(name: &std::ffi::OsStr, expected: &str) -> bool {
        #[cfg(windows)]
        {
            name.to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(expected))
        }
        #[cfg(not(windows))]
        {
            name == std::ffi::OsStr::new(expected)
        }
    }
    matches(world, ".world") && matches(directory, store)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn is_link_or_junction(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
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
pub(crate) fn replace_workspace_files(
    root: &Path,
    files: &std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
) -> Result<(), String> {
    let root = crate::compiler::source_path(root);
    for relative in files.keys() {
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
            || relative.starts_with(Path::new(".world/.checkpoints"))
            || relative.starts_with(Path::new(".world/.transactions"))
        {
            return Err(format!("恢复工作区文件路径无效：{}", relative.display()));
        }
    }
    FILES.with(|current| {
        let mut current = current.borrow_mut();
        current.retain(|path, _| !path.starts_with(&root));
        for (relative, bytes) in files {
            current.insert(
                crate::compiler::source_path(&root.join(relative)),
                bytes.clone(),
            );
        }
        Ok(())
    })
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

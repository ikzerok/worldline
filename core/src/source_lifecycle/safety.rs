use crate::project::Project;
use std::path::{Component, Path, PathBuf};

pub(crate) fn relative(path: &Path) -> Result<(), String> {
    if path.to_string_lossy().contains('\\') {
        return Err("源码请求路径须使用 /，不能含反斜杠".into());
    }
    native_relative(path)
}

/// Rust 调用方的 Path::join 使用平台分隔符；请求字符串另由 relative 检查可移植格式。
pub(crate) fn native_relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || path.extension().is_none_or(|extension| extension != "wl")
        || path.to_str().is_none()
        || path.to_string_lossy().contains(':')
        || (!cfg!(windows) && path.to_string_lossy().contains('\\'))
        || path.to_string_lossy().chars().any(char::is_control)
    {
        return Err("源码路径须为工作区内相对 .wl，不能含上级跳转、非法分隔符或控制字符".into());
    }
    let portable = portable_identity(path);
    if portable.starts_with(".world/.transactions/") || portable.starts_with(".world/.checkpoints/")
    {
        return Err("源码不能使用保存事务或本地检查点保留目录".into());
    }
    Ok(())
}

fn portable_identity(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn inventory(project: &Project) -> Result<Vec<PathBuf>, String> {
    let files = baseline_inventory(project)?;
    if let Some(path) = files.iter().find(|path| {
        path.extension().is_some_and(|extension| extension == "wl")
            && !project.documents.contains_key(*path)
    }) {
        return Err(format!(
            "磁盘新增源码尚未载入，请刷新后重试：{}",
            path.display()
        ));
    }
    Ok(files)
}

pub(crate) fn baseline_inventory(project: &Project) -> Result<Vec<PathBuf>, String> {
    let files = match disk_inventory(&project.root) {
        Ok(files) => files,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(format!("无法检查源码工作区边界：{error}")),
    };
    buffer_budget(project)?;
    Ok(files)
}

pub(crate) fn buffer_budget(project: &Project) -> Result<(), String> {
    if project
        .documents
        .len()
        .saturating_add(project.authoring_documents.len())
        > 4096
    {
        return Err("源码组织工作区超过 4096 文件预算，工程未修改".into());
    }
    let bytes: usize = project
        .documents
        .values()
        .map(|document| document.text.len())
        .sum();
    if bytes > 64 * 1024 * 1024 {
        return Err("源码组织超过 64 MiB 源码预算，工程未修改".into());
    }
    let json_bytes = project
        .authoring_documents
        .values()
        .fold(0usize, |sum, document| {
            sum.saturating_add(document.bytes().len())
        });
    if json_bytes > 64 * 1024 * 1024 {
        return Err("源码组织超过 64 MiB 已登记文档预算，工程未修改".into());
    }
    Ok(())
}

pub(crate) fn destination(project: &Project, relative_path: &Path) -> Result<PathBuf, String> {
    relative(relative_path)?;
    native_destination(project, relative_path)
}

pub(crate) fn native_destination(
    project: &Project,
    relative_path: &Path,
) -> Result<PathBuf, String> {
    native_relative(relative_path)?;
    let files = inventory(project)?;
    // within 与 compiler 使用同一原生身份，不能丢弃它并将请求的混合分隔符存入缓冲。
    let path = crate::file_access::within(&project.root, &project.root.join(relative_path))?;
    let folded = portable_identity(&path);
    for existing in files
        .iter()
        .chain(project.documents.keys())
        .chain(project.authoring_documents.keys())
    {
        let identity = portable_identity(existing);
        if identity == folded
            || folded.starts_with(&format!("{identity}/"))
            || identity.starts_with(&format!("{folded}/"))
        {
            return Err(format!(
                "目标或父路径已存在/大小写冲突：{}",
                existing.display()
            ));
        }
        // 即使父目录尚未落盘，缓冲树也不能建立 Dir/… 与 dir/… 两个可移植身份。
        for (left, right) in existing.components().zip(path.components()) {
            let left = left.as_os_str();
            let right = right.as_os_str();
            if left == right {
                continue;
            }
            if left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase() {
                return Err("目标与当前缓冲树的父路径存在大小写别名".into());
            }
            break;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut parent = project.root.clone();
        for component in relative_path.components() {
            if parent.is_dir() {
                for entry in std::fs::read_dir(&parent).map_err(|error| error.to_string())? {
                    let name = entry.map_err(|error| error.to_string())?.file_name();
                    if name != component.as_os_str()
                        && name.to_string_lossy().to_lowercase()
                            == component.as_os_str().to_string_lossy().to_lowercase()
                    {
                        return Err("目标父目录存在大小写别名，无法跨平台安全移动".into());
                    }
                }
            }
            parent.push(component.as_os_str());
            match std::fs::symlink_metadata(&parent) {
                Ok(metadata) if crate::file_access::is_link_or_junction(&metadata) => {
                    return Err("目标路径不允许链接或目录联接".into());
                }
                Ok(metadata) if parent == path || !metadata.is_dir() => {
                    return Err("目标已经存在或父级不是目录".into());
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("无法检查目标路径：{error}")),
            }
        }
    }
    Ok(path)
}

/// 提交前最后一次校验；取消回调或同步调用者不得在检查间隙偷换资源/目标。
pub(super) fn revalidate(
    project: &Project,
    plan: &super::SourceLifecyclePlan,
) -> Result<(), String> {
    project.source_lifecycle_disk_baselines_match()?;
    writable_paths(&plan.changes)?;
    inventory(project)?;
    match &plan.request {
        super::SourceLifecycleRequest::Create { path } => {
            destination(project, path)?;
        }
        super::SourceLifecycleRequest::Move { to, .. } => {
            destination(project, to)?;
        }
        super::SourceLifecycleRequest::Include { .. } => {}
    }
    let mut checked = std::collections::BTreeSet::new();
    for resource in &plan.resources {
        if !checked.insert(&resource.resolved_before) {
            continue;
        }
        let bytes = super::resource_bytes(project, &resource.resolved_before)?;
        if super::digest(&bytes) != resource.content_digest {
            return Err("提交前资源已变化，整批未提交，请重新预览".into());
        }
    }
    Ok(())
}

pub(super) fn writable_paths(changes: &[super::SourceLifecycleChange]) -> Result<(), String> {
    for path in changes
        .iter()
        .flat_map(|change| [&change.path, &change.after_path])
    {
        writable_path(path)?;
    }
    Ok(())
}

/// 浏览器仅检查导入快照/能力；不伪造浏览器对宿主文件系统的权限访问。
pub(crate) fn writable_path(path: &Path) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if crate::file_access::is_link_or_junction(&metadata)
                    || metadata.permissions().readonly()
                {
                    return Err(format!(
                        "源码组织目标或父级为只读/链接：{}",
                        ancestor.display()
                    ));
                }
                if ancestor != path {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("无法检查源码组织写入路径：{error}")),
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = path;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn disk_inventory(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    crate::file_access::workspace_files_limited(root, 4096)
}

#[cfg(not(target_arch = "wasm32"))]
fn disk_inventory(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let invalid = |message: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, message);
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut entries = 0usize;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            entries += 1;
            if entries > 4096 {
                return Err(invalid("源码组织超过 4096 文件/目录扫描预算"));
            }
            let path = entry?.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if crate::file_access::is_link_or_junction(&metadata) {
                return Err(invalid("源码组织不支持链接或目录联接"));
            }
            crate::file_access::within(root, &path).map_err(|message| invalid(&message))?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid("源码组织路径越界"))?;
            let text = relative.to_string_lossy().replace('\\', "/").to_lowercase();
            if text == ".world/.transactions" || text == ".world/.checkpoints" {
                if !metadata.is_dir() {
                    return Err(invalid("本地存储路径不是目录"));
                }
                continue;
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                files.push(path);
            } else {
                return Err(invalid("源码组织工作区含有非普通文件"));
            }
        }
    }
    files.sort();
    Ok(files)
}

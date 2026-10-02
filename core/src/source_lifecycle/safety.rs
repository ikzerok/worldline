use crate::project::Project;
use std::path::{Component, Path, PathBuf};

pub(crate) fn relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || path.extension().is_none_or(|extension| extension != "wl")
        || path.to_str().is_none()
        || path
            .to_string_lossy()
            .contains(['\\', ':', '\0', '\n', '\r'])
    {
        return Err("源码路径须为工作区内相对 .wl，不能含上级跳转、反斜杠或控制字符".into());
    }
    let portable = path.to_string_lossy().to_lowercase();
    if portable.starts_with(".world/.transactions/") || portable.starts_with(".world/.checkpoints/")
    {
        return Err("源码不能使用保存事务或本地检查点保留目录".into());
    }
    Ok(())
}

pub(crate) fn inventory(project: &Project) -> Result<Vec<PathBuf>, String> {
    let files = match crate::file_access::workspace_files_limited(&project.root, 4096) {
        Ok(files) => files,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(format!("无法检查源码工作区边界：{error}")),
    };
    if let Some(path) = files.iter().find(|path| {
        path.extension().is_some_and(|extension| extension == "wl")
            && !project.documents.contains_key(*path)
    }) {
        return Err(format!(
            "磁盘新增源码尚未载入，请刷新后重试：{}",
            path.display()
        ));
    }
    if files.len() > 4096 || project.documents.len() > 4096 {
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
    Ok(files)
}

pub(crate) fn destination(project: &Project, relative_path: &Path) -> Result<PathBuf, String> {
    relative(relative_path)?;
    let files = inventory(project)?;
    let path = project.root.join(relative_path);
    crate::file_access::within(&project.root, &path)?;
    let folded = path.to_string_lossy().to_lowercase();
    for existing in files
        .iter()
        .chain(project.documents.keys())
        .chain(project.authoring_documents.keys())
    {
        let identity = existing.to_string_lossy().to_lowercase();
        if identity == folded || folded.starts_with(&format!("{identity}/")) {
            return Err(format!(
                "目标或父路径已存在/大小写冲突：{}",
                existing.display()
            ));
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
    project.checkpoint_disk_baselines_match()?;
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
        let bytes = if let Some(document) = project
            .documents
            .get(&resource.resolved_before)
            .filter(|document| !document.is_deleted())
        {
            document.text.as_bytes().to_vec()
        } else {
            crate::file_access::read_limited(&resource.resolved_before, 64 * 1024 * 1024)
                .map_err(|error| format!("提交前资源不可读：{error}"))?
        };
        if super::digest(&bytes) != resource.content_digest {
            return Err("提交前资源已变化，整批未提交，请重新预览".into());
        }
    }
    Ok(())
}

pub(super) fn writable_paths(changes: &[super::SourceLifecycleChange]) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    for path in changes
        .iter()
        .flat_map(|change| [&change.path, &change.after_path])
    {
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
    }
    #[cfg(target_arch = "wasm32")]
    let _ = changes;
    Ok(())
}

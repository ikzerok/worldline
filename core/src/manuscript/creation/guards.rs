use super::*;
use crate::source_lifecycle::{safety, SourceLifecycleFailure, SourceLifecycleFailureKind};
use crate::workspace_documents::manifest_path;
use std::path::Path;

pub(super) fn failure(error: SourceLifecycleFailure) -> Failure {
    let code = match error.kind {
        SourceLifecycleFailureKind::IllegalPath => Code::InvalidDestination,
        SourceLifecycleFailureKind::SourceChanged => Code::ExternalConflict,
        _ => Code::SourceUnavailable,
    };
    Failure::new(code, error.message)
}

pub(super) fn workspace(project: &Project) -> Result<String, Failure> {
    project
        .ensure_workspace_writable()
        .map_err(|m| Failure::new(Code::ReadOnly, m))?;
    budget(project)?;
    disk_budget(project)?;
    let inventory = safety::inventory(project).map_err(failure)?;
    project
        .source_disk_baselines_match_inventory(&inventory)
        .map_err(failure)?;
    project.source_lifecycle_guard(&inventory).map_err(failure)
}

pub(super) fn budget(project: &Project) -> Result<(), Failure> {
    if project
        .documents
        .len()
        .saturating_add(project.authoring_documents.len())
        > 4096
        || project
            .documents
            .values()
            .fold(0usize, |n, d| n.saturating_add(d.text.len()))
            > 64 * 1024 * 1024
        || project
            .authoring_documents
            .values()
            .fold(0usize, |n, d| n.saturating_add(d.bytes().len()))
            > 64 * 1024 * 1024
    {
        return Err(Failure::new(
            Code::BudgetExceeded,
            "新章工作区超过 4096 文件或 64 MiB 缓冲预算，整笔未提交",
        ));
    }
    Ok(())
}

pub(super) fn request(request: &ManuscriptChapterCreateRequest) -> Result<(), Failure> {
    if request.schema_version != 1 {
        return Err(Failure::new(Code::InvalidChapter, "不支持的新章请求版本"));
    }
    let mut ids = vec![request.chapter.id.as_str()];
    let mut titles = vec![request.chapter.title.as_str()];
    match &request.book {
        ManuscriptBookDestination::Existing { id } => ids.push(id),
        ManuscriptBookDestination::New { id, title } => {
            ids.push(id);
            titles.push(title);
        }
    }
    ids.extend(request.chapter.parent_section_id.as_deref());
    ids.extend(request.chapter.after_sibling_id.as_deref());
    if ids.iter().any(|s| s.len() > 256)
        || titles.iter().any(|s| s.len() > 4096)
        || request.expected_baseline.len() > 1024
    {
        return Err(Failure::new(
            Code::BudgetExceeded,
            "新章身份或标题超过字节预算",
        ));
    }
    if ids.iter().any(|id| !super::super::index::valid_id(id))
        || titles.iter().any(|title| title.trim().is_empty())
    {
        return Err(Failure::new(
            Code::InvalidChapter,
            "书稿/章节 ID 无效或标题为空",
        ));
    }
    match &request.source {
        ManuscriptChapterSource::Existing { target } => {
            if target.kind.len() > 256 || target.id.len() > 256 {
                return Err(Failure::new(
                    Code::BudgetExceeded,
                    "正文目标身份超过 256 字节预算",
                ));
            }
        }
        ManuscriptChapterSource::NewEvent {
            id,
            storyline,
            destination,
        } => {
            let (ManuscriptSourceDestination::ExistingActiveSource { relative_path }
            | ManuscriptSourceDestination::NewActiveSource { relative_path }) = destination;
            if id.len() > 256 || storyline.len() > 256 || relative_path.as_os_str().len() > 1024 {
                return Err(Failure::new(
                    Code::BudgetExceeded,
                    "新事件身份或目标路径超过字节预算",
                ));
            }
            if id
                .split('.')
                .any(|part| crate::authoring::identifier(part).is_err())
                || crate::authoring::identifier(storyline).is_err()
            {
                return Err(Failure::new(
                    Code::InvalidChapter,
                    "新事件 ID 或故事线 ID 不符合正式语言规则",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn destinations(
    project: &Project,
    request: &ManuscriptChapterCreateRequest,
) -> Result<(), Failure> {
    let manifest = manifest_path(&project.root);
    if !project.authoring_documents.contains_key(&manifest) {
        missing_path(project, &manifest)?;
    }
    if let ManuscriptBookDestination::New { id, .. } = &request.book {
        missing_path(
            project,
            &project
                .root
                .join(".world/manuscripts")
                .join(format!("{id}.json")),
        )?;
    }
    if let ManuscriptChapterSource::NewEvent { destination, .. } = &request.source {
        let (ManuscriptSourceDestination::ExistingActiveSource { relative_path }
        | ManuscriptSourceDestination::NewActiveSource { relative_path }) = destination;
        safety::relative(relative_path).map_err(|m| Failure::new(Code::InvalidDestination, m))?;
        match destination {
            ManuscriptSourceDestination::NewActiveSource { .. } => {
                safety::destination(project, relative_path).map_err(failure)?;
            }
            ManuscriptSourceDestination::ExistingActiveSource { .. } => {
                let path =
                    crate::file_access::within(&project.root, &project.root.join(relative_path))
                        .map_err(|m| Failure::new(Code::InvalidDestination, m))?;
                if !project.sources().contains_key(&path) {
                    return Err(Failure::new(
                        Code::SourceUnavailable,
                        "新事件目标必须是已载入的活动源码；归档、非活动和删除稿不能使用",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// 展示 JSON 的缺失目标规则与源码相同，但不把 .wl 扩展规则套在 JSON 上。
fn missing_path(project: &Project, path: &Path) -> Result<(), Failure> {
    let path = crate::file_access::within(&project.root, path)
        .map_err(|m| Failure::new(Code::InvalidDestination, m))?;
    let inventory = safety::inventory(project).map_err(failure)?;
    let key = portable(&path);
    for existing in inventory
        .iter()
        .chain(project.documents.keys())
        .chain(project.authoring_documents.keys())
    {
        let other = portable(existing);
        if key == other
            || key.starts_with(&format!("{other}/"))
            || other.starts_with(&format!("{key}/"))
        {
            return Err(Failure::new(
                Code::InvalidDestination,
                format!("书稿目标或父路径已存在/大小写冲突：{}", existing.display()),
            ));
        }
        for (left, right) in existing.components().zip(path.components()) {
            if left == right {
                continue;
            }
            if left.as_os_str().to_string_lossy().to_lowercase()
                == right.as_os_str().to_string_lossy().to_lowercase()
            {
                return Err(Failure::new(
                    Code::InvalidDestination,
                    "书稿目标父路径存在大小写别名",
                ));
            }
            break;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut parent = project.root.clone();
        for part in path
            .strip_prefix(&project.root)
            .map_err(|_| Failure::new(Code::InvalidDestination, "书稿路径越界"))?
            .components()
        {
            if parent.is_dir() {
                for entry in std::fs::read_dir(&parent)
                    .map_err(|e| Failure::new(Code::ExternalConflict, e.to_string()))?
                {
                    let name = entry
                        .map_err(|e| Failure::new(Code::ExternalConflict, e.to_string()))?
                        .file_name();
                    if name != part.as_os_str()
                        && name.to_string_lossy().to_lowercase()
                            == part.as_os_str().to_string_lossy().to_lowercase()
                    {
                        return Err(Failure::new(
                            Code::InvalidDestination,
                            "书稿目标父目录存在大小写别名",
                        ));
                    }
                }
            }
            parent.push(part.as_os_str());
            match std::fs::symlink_metadata(&parent) {
                Ok(m)
                    if crate::file_access::is_link_or_junction(&m)
                        || parent == path
                        || !m.is_dir() =>
                {
                    return Err(Failure::new(
                        Code::InvalidDestination,
                        "书稿目标已存在、含链接或父级不是目录",
                    ));
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Failure::new(Code::ExternalConflict, e.to_string())),
            }
        }
    }
    safety::writable_path(&path).map_err(|m| Failure::new(Code::ReadOnly, m))
}

fn portable(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect::<Vec<_>>()
        .join("/")
}

/// 用结构化计数分类预算错误；不从既有中文错误文本猜测失败类别。
fn disk_budget(project: &Project) -> Result<(), Failure> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut pending = vec![project.root.clone()];
        let mut count = 0usize;
        let mut bytes = 0u64;
        while let Some(directory) = pending.pop() {
            if directory != project.root {
                crate::file_access::within(&project.root, &directory)
                    .map_err(|m| Failure::new(Code::InvalidDestination, m))?;
                let metadata = std::fs::symlink_metadata(&directory)
                    .map_err(|e| Failure::new(Code::ExternalConflict, e.to_string()))?;
                if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
                    return Err(Failure::new(
                        Code::InvalidDestination,
                        "工作区库存目录不能为链接或非目录",
                    ));
                }
            }
            let entries = match std::fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(e) if directory == project.root && e.kind() == std::io::ErrorKind::NotFound => {
                    continue
                }
                Err(e) => {
                    return Err(Failure::new(
                        Code::ExternalConflict,
                        format!("无法检查新章工作区库存：{e}"),
                    ))
                }
            };
            for entry in entries {
                let entry =
                    entry.map_err(|e| Failure::new(Code::ExternalConflict, e.to_string()))?;
                count = count.saturating_add(1);
                if count > 4096 {
                    return Err(Failure::new(
                        Code::BudgetExceeded,
                        "新章工作区超过 4096 文件/目录扫描预算",
                    ));
                }
                let path = entry.path();
                crate::file_access::within(&project.root, &path)
                    .map_err(|m| Failure::new(Code::InvalidDestination, m))?;
                let metadata = std::fs::symlink_metadata(&path)
                    .map_err(|e| Failure::new(Code::ExternalConflict, e.to_string()))?;
                if crate::file_access::is_link_or_junction(&metadata) {
                    return Err(Failure::new(
                        Code::InvalidDestination,
                        "工作区库存不支持链接或目录联接",
                    ));
                }
                let relative = path
                    .strip_prefix(&project.root)
                    .map_err(|_| Failure::new(Code::InvalidDestination, "工作区库存路径越界"))?;
                let key = portable(relative);
                if key == ".world/.transactions" || key == ".world/.checkpoints" {
                    continue;
                }
                if metadata.is_dir() {
                    pending.push(path);
                } else if metadata.is_file() {
                    bytes = bytes.saturating_add(metadata.len());
                    if metadata.len() > 64 * 1024 * 1024 || bytes > 256 * 1024 * 1024 {
                        return Err(Failure::new(
                            Code::BudgetExceeded,
                            "新章库存超过单文件 64 MiB 或总计 256 MiB 验证预算",
                        ));
                    }
                }
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = project;
    Ok(())
}

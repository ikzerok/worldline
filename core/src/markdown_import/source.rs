use super::{
    InputFile, InputFileSource, Project, MAX_IMPORT_ENTRIES, MAX_IMPORT_FILES,
    MAX_IMPORT_PATH_BYTES,
};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Component, Path, PathBuf};

fn validate_import_relative_path(relative: &str) -> Result<(), String> {
    if relative.len() > MAX_IMPORT_PATH_BYTES {
        return Err(format!("Markdown 来源相对路径过长：{relative}"));
    }
    for component in relative.split('/') {
        let stem = component
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let reserved_device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|suffix| {
                    suffix.len() == 1
                        && suffix.as_bytes()[0].is_ascii_digit()
                        && suffix.as_bytes()[0] != b'0'
                })
            });
        if matches!(component, "" | "." | "..")
            || component.len() > 200
            || component.ends_with(['.', ' '])
            || component.chars().any(|character| {
                character.is_control()
                    || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\\')
            })
            || reserved_device
        {
            return Err(format!(
                "Markdown 来源路径不是可安全迁移的相对路径：{relative}"
            ));
        }
    }
    Ok(())
}

pub(super) fn check_tracked_disk_baselines(project: &Project) -> Result<(), String> {
    for path in project
        .documents
        .keys()
        .chain(project.authoring_documents.keys())
    {
        let state = project
            .tracked_file_state(path)
            .ok_or_else(|| format!("无法读取 Project 基线：{}", path.display()))?;
        let disk = match crate::file_access::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("无法核对 Project 基线：{error}")),
        };
        if disk != state.baseline {
            return Err(format!("工程文件在打开后发生外部变化：{}", path.display()));
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn checked_source_root(path: &Path) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("无法读取 Markdown 来源目录：{error}"))?;
    if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
        return Err("Markdown 来源必须是普通目录，不能是符号链接或联接".into());
    }
    std::fs::canonicalize(path).map_err(|error| format!("无法规范化 Markdown 来源目录：{error}"))
}

pub(super) fn validate_snapshot_label(label: &str) -> Result<(), String> {
    if label.trim().is_empty()
        || label.len() > MAX_IMPORT_PATH_BYTES
        || label.chars().any(char::is_control)
    {
        return Err("Markdown 快照标签不能为空、不能含控制字符，且最多 512 字节".into());
    }
    Ok(())
}

pub(super) fn read_input_file(input: &InputFile<'_>) -> Result<Vec<u8>, String> {
    match &input.source {
        InputFileSource::Snapshot(bytes) => Ok(bytes.to_vec()),
        #[cfg(not(target_arch = "wasm32"))]
        InputFileSource::Disk { root, absolute } => {
            let metadata = std::fs::symlink_metadata(absolute)
                .map_err(|error| format!("无法检查来源文件 {}：{error}", input.relative))?;
            if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_file() {
                return Err(format!(
                    "来源文件在预览期间变为链接或特殊文件：{}",
                    input.relative
                ));
            }
            let canonical = std::fs::canonicalize(absolute)
                .map_err(|error| format!("无法规范化来源文件 {}：{error}", input.relative))?;
            if !canonical.starts_with(root) {
                return Err(format!("来源文件越过所选目录：{}", input.relative));
            }
            if metadata.len() != input.length {
                return Err(format!("来源文件在扫描后发生变化：{}", input.relative));
            }
            let bytes = std::fs::read(&canonical)
                .map_err(|error| format!("无法读取来源文件 {}：{error}", input.relative))?;
            if bytes.len() as u64 != input.length {
                return Err(format!("来源文件在读取期间发生变化：{}", input.relative));
            }
            Ok(bytes)
        }
    }
}

pub(super) fn enumerate_snapshot_files(
    files: &crate::workspace_snapshot::Files,
) -> Result<Vec<InputFile<'_>>, String> {
    if files.len() > MAX_IMPORT_FILES {
        return Err("Markdown 来源文件数超过单次迁移预算".into());
    }
    let mut entries_seen = files.len();
    let mut portable_paths = BTreeMap::<String, String>::new();
    let mut file_paths = BTreeSet::new();
    let mut directories = BTreeSet::new();
    let mut inputs = Vec::with_capacity(files.len());
    for (path, bytes) in files {
        let raw = path
            .to_str()
            .ok_or_else(|| format!("Markdown 来源路径不是 UTF-8：{}", path.display()))?;
        let relative = raw.replace('\\', "/");
        if path.is_absolute()
            || relative.starts_with('/')
            || relative
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(format!("Markdown 来源路径不安全：{relative}"));
        }
        validate_import_relative_path(&relative)?;
        let components = relative.split('/').collect::<Vec<_>>();
        for end in 1..=components.len() {
            let joined = components[..end].join("/");
            let folded = joined.to_lowercase();
            if portable_paths
                .insert(folded, joined.clone())
                .is_some_and(|previous| previous != joined)
            {
                return Err(format!("Markdown 来源包含大小写折叠后重名的路径：{joined}"));
            }
            if end < components.len() {
                directories.insert(joined);
            }
        }
        file_paths.insert(relative.clone());
        inputs.push(InputFile {
            relative,
            length: bytes.len() as u64,
            source: InputFileSource::Snapshot(bytes),
        });
    }
    if file_paths.iter().any(|file| directories.contains(file)) {
        return Err("Markdown 来源同一路径不能同时作为文件和目录".into());
    }
    entries_seen = entries_seen.saturating_add(directories.len());
    if entries_seen > MAX_IMPORT_ENTRIES {
        return Err("Markdown 来源目录项超过单次迁移预算".into());
    }
    inputs.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(inputs)
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn enumerate_source_files(root: &Path) -> Result<Vec<InputFile<'static>>, String> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut entries_seen = 0usize;
    let mut portable_paths = BTreeMap::<String, String>::new();
    while let Some(directory) = pending.pop() {
        let metadata = std::fs::symlink_metadata(&directory)
            .map_err(|error| format!("无法检查 Markdown 来源目录：{error}"))?;
        let canonical = std::fs::canonicalize(&directory)
            .map_err(|error| format!("无法规范化 Markdown 来源目录：{error}"))?;
        if crate::file_access::is_link_or_junction(&metadata)
            || !metadata.is_dir()
            || !canonical.starts_with(root)
        {
            return Err("Markdown 来源目录在扫描期间变为链接或越界目录".into());
        }
        let entries = std::fs::read_dir(&directory)
            .map_err(|error| format!("无法枚举 Markdown 来源目录：{error}"))?;
        for entry in entries {
            entries_seen = entries_seen.saturating_add(1);
            if entries_seen > MAX_IMPORT_ENTRIES {
                return Err("Markdown 来源目录项超过单次迁移预算".into());
            }
            let entry = entry.map_err(|error| format!("无法枚举 Markdown 来源目录：{error}"))?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| format!("无法检查 Markdown 来源路径：{error}"))?;
            if crate::file_access::is_link_or_junction(&metadata) {
                return Err(format!(
                    "Markdown 来源不允许符号链接或联接：{}",
                    path.display()
                ));
            }
            let relative_path = path
                .strip_prefix(root)
                .map_err(|_| "Markdown 来源路径越界")?;
            if relative_path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err(format!("Markdown 来源路径不安全：{}", path.display()));
            }
            let relative = relative_path
                .to_str()
                .ok_or_else(|| format!("Markdown 来源路径不是 UTF-8：{}", path.display()))?
                .replace('\\', "/");
            validate_import_relative_path(&relative)?;
            let folded = relative.to_lowercase();
            if portable_paths
                .insert(folded, relative.clone())
                .is_some_and(|previous| previous != relative)
            {
                return Err(format!(
                    "Markdown 来源包含大小写折叠后重名的路径：{relative}"
                ));
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                files.push(InputFile {
                    relative,
                    length: metadata.len(),
                    source: InputFileSource::Disk {
                        root: root.to_path_buf(),
                        absolute: path,
                    },
                });
            } else {
                return Err(format!("Markdown 来源包含特殊文件：{}", path.display()));
            }
            if files.len() > MAX_IMPORT_FILES {
                return Err("Markdown 来源文件数超过单次迁移预算".into());
            }
        }
    }
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn output_path_exists(project: &Project, relative: &str) -> Result<bool, String> {
    let relative = Path::new(relative);
    let target = crate::file_access::within(&project.root, &project.root.join(relative))?;
    let mut current = project.root.clone();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(part) = component else {
            return Err("导入目标路径包含非普通路径段".into());
        };
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if crate::file_access::is_link_or_junction(&metadata) {
                    return Err(format!("导入目标路径包含链接或联接：{}", current.display()));
                }
                if index + 1 < components.len() && !metadata.is_dir() {
                    return Ok(true);
                }
                if index + 1 == components.len() {
                    return Ok(true);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(format!("无法检查导入目标 {}：{error}", target.display())),
        }
    }
    Ok(false)
}

#[cfg(target_arch = "wasm32")]
pub(super) fn output_path_exists(project: &Project, relative: &str) -> Result<bool, String> {
    let target = relative.to_lowercase();
    let paths = crate::file_access::workspace_files(&project.root)
        .map_err(|error| format!("无法检查导入目标路径：{error}"))?;
    for path in paths
        .iter()
        .chain(project.documents.keys())
        .chain(project.authoring_documents.keys())
    {
        let Ok(path) = path.strip_prefix(&project.root) else {
            continue;
        };
        let Some(path) = path.to_str() else {
            return Err("工程包含非 UTF-8 路径，不能安全检查 Markdown 导入目标".into());
        };
        let existing = path.replace('\\', "/").to_lowercase();
        if existing == target
            || existing.starts_with(&format!("{target}/"))
            || target.starts_with(&format!("{existing}/"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

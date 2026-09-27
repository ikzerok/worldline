use super::*;
use std::collections::HashSet;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};
use web_time::{SystemTime, UNIX_EPOCH};
pub(super) fn validate_snapshot_files(files: &Files) -> Result<(), String> {
    if files.len() > MAX_CHECKPOINT_FILES {
        return Err("工作区文件数超过检查点上限".into());
    }
    for (path, bytes) in files {
        validate_relative_file(path)?;
        if path.extension().is_some_and(|extension| extension == "wl")
            && std::str::from_utf8(bytes).is_err()
        {
            return Err(format!("检查点源码不是有效 UTF-8：{}", path.display()));
        }
    }
    Ok(())
}

pub(super) fn validate_relative_file(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.components().any(|component| match component {
            Component::Normal(name) => name
                .to_str()
                .is_none_or(|name| name.is_empty() || name.contains(['\\', '/', ':'])),
            _ => true,
        })
        || is_reserved_store_path(path, ".checkpoints")
        || is_reserved_store_path(path, ".transactions")
    {
        return Err(format!("检查点文件路径无效：{}", path.display()));
    }
    Ok(())
}

fn is_reserved_store_path(path: &Path, store: &str) -> bool {
    let mut components = path.components();
    let (Some(Component::Normal(world)), Some(Component::Normal(directory))) =
        (components.next(), components.next())
    else {
        return false;
    };
    store_name_matches(world, ".world") && store_name_matches(directory, store)
}

fn store_name_matches(name: &std::ffi::OsStr, expected: &str) -> bool {
    #[cfg(windows)]
    {
        name.to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(expected))
    }
    #[cfg(not(windows))]
    {
        name == expected
    }
}

pub(super) fn ensure_workspace_snapshot_limits(
    project: &Project,
    max_bytes: u64,
) -> Result<(), String> {
    let root = crate::compiler::source_path(&project.root);
    let disk_paths = match crate::file_access::workspace_files(&root) {
        Ok(paths) => paths,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(format!("无法读取检查点工作区：{error}")),
    };
    let mut managed = HashSet::new();
    let mut count = 0usize;
    let mut bytes = 0u64;
    for (path, document) in &project.documents {
        managed.insert(crate::compiler::source_path(path));
        if !document.is_deleted() {
            count += 1;
            bytes = bytes
                .checked_add(document.text.len() as u64)
                .ok_or("检查点字节数超出可表示范围")?;
        }
    }
    for (path, document) in &project.authoring_documents {
        managed.insert(crate::compiler::source_path(path));
        if !document.is_deleted() {
            count += 1;
            bytes = bytes
                .checked_add(document.bytes().len() as u64)
                .ok_or("检查点字节数超出可表示范围")?;
        }
    }
    for path in disk_paths {
        if managed.contains(&crate::compiler::source_path(&path)) {
            continue;
        }
        count += 1;
        let size = disk_file_size(&path)?;
        bytes = bytes
            .checked_add(size)
            .ok_or("检查点字节数超出可表示范围")?;
        if count > MAX_CHECKPOINT_FILES || bytes > max_bytes {
            return Err("工作区超过检查点文件数或字节上限".into());
        }
    }
    if count > MAX_CHECKPOINT_FILES || bytes > max_bytes {
        return Err("工作区超过检查点文件数或字节上限".into());
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn disk_file_size(path: &Path) -> Result<u64, String> {
    std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("无法读取工作区文件大小 {}：{error}", path.display()))
}

pub(super) fn ensure_disk_file_limits(paths: &[PathBuf], max_bytes: u64) -> Result<(), String> {
    if paths.len() > MAX_CHECKPOINT_FILES {
        return Err("工作区超过检查点文件数上限".into());
    }
    let mut total = 0u64;
    for path in paths {
        total = total
            .checked_add(disk_file_size(path)?)
            .ok_or("检查点字节数超出可表示范围")?;
        if total > max_bytes {
            return Err("工作区超过检查点字节上限".into());
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn disk_file_size(path: &Path) -> Result<u64, String> {
    crate::file_access::read(path)
        .map(|bytes| bytes.len() as u64)
        .map_err(|error| format!("无法读取工作区文件大小 {}：{error}", path.display()))
}

pub(super) fn files_digest(files: &Files) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    mix(&mut hash, b"worldline-checkpoint-files-v1");
    for (path, bytes) in files {
        let path = path.to_string_lossy().replace('\\', "/");
        mix(&mut hash, path.as_bytes());
        mix(&mut hash, bytes);
    }
    format!("{hash:016x}")
}

fn mix(hash: &mut u64, bytes: &[u8]) {
    for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

pub(super) fn file_checksum(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub(super) fn saved_source_baselines(project: &Project) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let root = crate::compiler::source_path(&project.root);
    project
        .documents
        .keys()
        .filter_map(|path| {
            let path = crate::compiler::source_path(path);
            if !path.extension().is_some_and(|extension| extension == "wl") {
                return None;
            }
            let relative = path.strip_prefix(&root).ok()?.to_path_buf();
            let state = project.tracked_file_state(&path)?;
            Some((relative, state.baseline))
        })
        .collect()
}

pub(super) fn checkpoint_payload_bytes(
    files: &Files,
    text_base: &BTreeMap<PathBuf, Option<Vec<u8>>>,
) -> Result<u64, String> {
    files
        .values()
        .chain(text_base.iter().filter_map(|(path, bytes)| {
            bytes
                .as_ref()
                .filter(|bytes| files.get(path) != Some(*bytes))
        }))
        .try_fold(0u64, |total, bytes| total.checked_add(bytes.len() as u64))
        .ok_or_else(|| "检查点字节数超出可表示范围".into())
}

pub(super) fn text_base_digest(
    text_base: Option<&BTreeMap<PathBuf, Option<Vec<u8>>>>,
) -> Option<String> {
    let text_base = text_base?;
    let mut hash = 0xcbf29ce484222325u64;
    mix(&mut hash, b"worldline-checkpoint-text-base-v1");
    for (path, bytes) in text_base {
        let path = path.to_string_lossy().replace('\\', "/");
        mix(&mut hash, path.as_bytes());
        match bytes {
            Some(bytes) => {
                mix(&mut hash, b"present");
                mix(&mut hash, bytes);
            }
            None => mix(&mut hash, b"absent"),
        }
    }
    Some(format!("{hash:016x}"))
}

pub(super) fn checkpoint_record_digest(manifest: &CheckpointManifest) -> String {
    if manifest.version == LEGACY_CHECKPOINT_FORMAT_VERSION {
        return manifest.snapshot_digest.clone();
    }
    let mut hash = 0xcbf29ce484222325u64;
    mix(&mut hash, b"worldline-checkpoint-record-v2");
    mix(&mut hash, manifest.snapshot_digest.as_bytes());
    mix(
        &mut hash,
        manifest
            .text_base_digest
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );
    format!("{hash:016x}")
}

pub(super) fn make_manifest(
    label: Option<String>,
    files: &Files,
    text_base: &BTreeMap<PathBuf, Option<Vec<u8>>>,
    payload_bytes: u64,
) -> Result<CheckpointManifest, String> {
    let id = next_checkpoint_id();
    let created_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64;
    let entries = files
        .iter()
        .enumerate()
        .map(|(index, (path, bytes))| {
            validate_relative_file(path)?;
            Ok(CheckpointFileEntry {
                path: path.to_string_lossy().replace('\\', "/"),
                payload: format!("files/{index:08}.bin"),
                bytes: bytes.len() as u64,
                checksum: file_checksum(bytes),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let base_entries = text_base
        .iter()
        .enumerate()
        .map(|(index, (path, bytes))| {
            validate_relative_file(path)?;
            if !path.extension().is_some_and(|extension| extension == "wl") {
                return Err("检查点文本基线只能引用 .wl 文件".into());
            }
            let source = match bytes {
                None => CheckpointTextBaseSource::Absent,
                Some(bytes) if files.get(path) == Some(bytes) => CheckpointTextBaseSource::Snapshot,
                Some(bytes) => CheckpointTextBaseSource::Stored {
                    payload: format!("files/base-{index:08}.bin"),
                    bytes: bytes.len() as u64,
                    checksum: file_checksum(bytes),
                },
            };
            Ok(CheckpointTextBaseEntry {
                path: path.to_string_lossy().replace('\\', "/"),
                source,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(CheckpointManifest {
        version: CHECKPOINT_FORMAT_VERSION,
        id,
        label,
        created_at_unix_ms,
        payload_bytes,
        snapshot_digest: files_digest(files),
        files: entries,
        text_base: Some(base_entries),
        text_base_digest: text_base_digest(Some(text_base)),
    })
}

pub(super) fn summary_for_manifest(manifest: &CheckpointManifest) -> CheckpointSummary {
    CheckpointSummary {
        id: manifest.id.clone(),
        label: manifest.label.clone(),
        created_at_unix_ms: manifest.created_at_unix_ms,
        file_count: manifest.files.len(),
        payload_bytes: manifest.payload_bytes,
        available: true,
        unavailable_reason: None,
    }
}

pub(super) fn validate_checkpoint_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("检查点 ID 无效".into());
    }
    Ok(())
}
static NEXT_CHECKPOINT: AtomicU64 = AtomicU64::new(0);

pub(super) fn next_checkpoint_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = NEXT_CHECKPOINT.fetch_add(1, Ordering::Relaxed);
    format!("cp-{millis:016x}-{sequence:08x}")
}

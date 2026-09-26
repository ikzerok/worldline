use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

const CHECKPOINT_ROOT: &str = ".world/.checkpoints/v1";
const MANIFEST_NAME: &str = "checkpoint.json";
const STAGING_PREFIX: &str = ".staging-";
static CHECKPOINT_LOCK: Mutex<()> = Mutex::new(());

fn checkpoint_root(root: &Path) -> PathBuf {
    root.join(CHECKPOINT_ROOT)
}

fn checkpoint_path(root: &Path, id: &str) -> PathBuf {
    checkpoint_root(root).join(id)
}

fn lock_checkpoints() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    CHECKPOINT_LOCK
        .lock()
        .map_err(|_| "检查点存储锁不可用".to_string())
}

fn publish_checkpoint(
    root: &Path,
    manifest: CheckpointManifest,
    files: &Files,
    text_base: &BTreeMap<PathBuf, Option<Vec<u8>>>,
    limits: &CheckpointLimits,
) -> Result<(), String> {
    let _lock = lock_checkpoints()?;
    let records = read_checkpoint_records(root)?;
    if records.len() >= limits.max_count {
        return Err("检查点数量配额已满，请显式删除旧记录".into());
    }
    let used_bytes = records.iter().try_fold(0u64, |total, record| {
        total.checked_add(record.summary.payload_bytes)
    }).ok_or("检查点历史字节数超出可表示范围")?;
    if used_bytes.saturating_add(manifest.payload_bytes) > limits.max_total_bytes as u64 {
        return Err("检查点历史字节配额已满，请显式删除旧记录".into());
    }

    validate_relative_id(&manifest.id)?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    if manifest_bytes.len() as u64 > MAX_CHECKPOINT_MANIFEST_BYTES {
        return Err("检查点清单超过格式上限".into());
    }
    let history = checkpoint_root(root);
    ensure_checkpoint_directory(root, &history)?;
    remove_staging_directories(root, &history)?;
    let staging = history.join(format!("{STAGING_PREFIX}{}", manifest.id));
    let final_path = history.join(&manifest.id);
    if fs::symlink_metadata(&final_path).is_ok() || fs::symlink_metadata(&staging).is_ok() {
        return Err("检查点 ID 冲突，请重试".into());
    }
    fs::create_dir(&staging).map_err(|error| format!("无法建立检查点暂存目录：{error}"))?;
    let result = (|| {
        let payload_directory = staging.join("files");
        fs::create_dir(&payload_directory).map_err(|error| error.to_string())?;
        for entry in &manifest.files {
            let bytes = files
                .get(Path::new(&entry.path))
                .ok_or_else(|| format!("检查点清单引用了缺失文件：{}", entry.path))?;
            if failure_requested("payload") {
                return Err("检查点故障注入：payload".into());
            }
            write_checkpoint_file(&staging.join(&entry.payload), bytes)?;
        }
        for entry in manifest.text_base.as_deref().unwrap_or_default() {
            if let CheckpointTextBaseSource::Stored { payload, .. } = &entry.source {
                let bytes = text_base
                    .get(Path::new(&entry.path))
                    .and_then(Option::as_ref)
                    .ok_or_else(|| format!("检查点文本基线引用了缺失文件：{}", entry.path))?;
                if failure_requested("payload") {
                    return Err("检查点故障注入：payload".into());
                }
                write_checkpoint_file(&staging.join(payload), bytes)?;
            }
        }
        if failure_requested("manifest") {
            return Err("检查点故障注入：manifest".into());
        }
        write_checkpoint_file(&staging.join(MANIFEST_NAME), &manifest_bytes)?;
        sync_directory(&payload_directory)?;
        sync_directory(&staging)?;
        if failure_requested("publish") {
            return Err("检查点故障注入：publish".into());
        }
        fs::rename(&staging, &final_path).map_err(|error| error.to_string())?;
        sync_directory(&history)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn list_checkpoint_records(root: &Path) -> Result<Vec<CheckpointListing>, String> {
    let _lock = lock_checkpoints()?;
    read_checkpoint_records(root)
}

fn read_checkpoint_records(root: &Path) -> Result<Vec<CheckpointListing>, String> {
    let history = checkpoint_root(root);
    let metadata = match fs::symlink_metadata(&history) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("无法读取检查点目录：{error}")),
    };
    if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
        return Err("检查点存储路径不能是链接或普通文件".into());
    }
    validate_checkpoint_directory(root, &history)?;
    let mut records = Vec::new();
    let mut directory_count = 0usize;
    for entry in fs::read_dir(&history).map_err(|error| error.to_string())? {
        directory_count += 1;
        if directory_count > MAX_CHECKPOINT_RECORDS_ON_DISK {
            return Err("检查点存储目录超过安全条目上限".into());
        }
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if crate::file_access::is_link_or_junction(&metadata) {
            return Err(format!("检查点目录包含链接：{}", path.display()));
        }
        let id = entry
            .file_name()
            .to_str()
            .ok_or("检查点目录名不是有效 UTF-8")?
            .to_owned();
        if id.starts_with(STAGING_PREFIX) {
            if !metadata.is_dir() {
                return Err(format!("检查点暂存项不是目录：{}", path.display()));
            }
            continue;
        }
        if !metadata.is_dir() {
            return Err(format!("检查点记录不是目录：{}", path.display()));
        }
        validate_checkpoint_id(&id)?;
        match read_checkpoint_bundle(root, &id) {
            Ok(bundle) => records.push(CheckpointListing {
                summary: summary_for_manifest(&bundle.manifest),
            }),
            Err(error) => records.push(CheckpointListing {
                summary: unavailable_summary(&id, &error, unavailable_payload_bytes(&path)?),
            }),
        }
    }
    Ok(records)
}

fn load_checkpoint(root: &Path, id: &str) -> Result<CheckpointBundle, String> {
    let _lock = lock_checkpoints()?;
    read_checkpoint_bundle(root, id)
}

fn read_checkpoint_bundle(root: &Path, id: &str) -> Result<CheckpointBundle, String> {
    validate_checkpoint_id(id)?;
    let directory = checkpoint_path(root, id);
    validate_checkpoint_directory(root, &directory)?;
    let manifest_path = directory.join(MANIFEST_NAME);
    let manifest_metadata = fs::symlink_metadata(&manifest_path)
        .map_err(|error| format!("检查点清单无法读取：{error}"))?;
    if crate::file_access::is_link_or_junction(&manifest_metadata)
        || !manifest_metadata.is_file()
        || manifest_metadata.len() > MAX_CHECKPOINT_MANIFEST_BYTES
    {
        return Err("检查点清单类型或大小无效".into());
    }
    let manifest: CheckpointManifest = serde_json::from_slice(
        &fs::read(&manifest_path).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("检查点清单损坏：{error}"))?;
    let format_is_valid = match manifest.version {
        LEGACY_CHECKPOINT_FORMAT_VERSION => {
            manifest.text_base.is_none() && manifest.text_base_digest.is_none()
        }
        CHECKPOINT_FORMAT_VERSION => {
            manifest.text_base.is_some() && manifest.text_base_digest.is_some()
        }
        _ => false,
    };
    if !format_is_valid
        || manifest.id != id
        || manifest.files.len() > MAX_CHECKPOINT_FILES
        || manifest
            .text_base
            .as_ref()
            .is_some_and(|entries| entries.len() > MAX_CHECKPOINT_FILES)
        || manifest.label.as_ref().is_some_and(|label| label.chars().count() > 120)
    {
        return Err("检查点清单版本或字段无效".into());
    }
    let payload_directory = directory.join("files");
    let payload_metadata = fs::symlink_metadata(&payload_directory)
        .map_err(|error| format!("检查点负载目录无法读取：{error}"))?;
    if crate::file_access::is_link_or_junction(&payload_metadata) || !payload_metadata.is_dir() {
        return Err("检查点负载目录无效".into());
    }
    let mut files = Files::new();
    let mut text_base = manifest.text_base.as_ref().map(|_| BTreeMap::new());
    let mut payloads = BTreeSet::new();
    let mut total = 0u64;
    for file in &manifest.files {
        let path = parse_relative_file(&file.path)?;
        let payload = validate_payload_path(&file.payload)?;
        if !payloads.insert(file.payload.clone()) {
            return Err("检查点包含重复负载路径".into());
        }
        total = total
            .checked_add(file.bytes)
            .ok_or("检查点文件大小无效")?;
        if total > DEFAULT_MAX_CHECKPOINT_BYTES as u64 {
            return Err("检查点负载超过格式上限".into());
        }
        let payload_path = directory.join(payload);
        let metadata = fs::symlink_metadata(&payload_path)
            .map_err(|error| format!("检查点文件负载缺失：{error}"))?;
        if crate::file_access::is_link_or_junction(&metadata)
            || !metadata.is_file()
            || metadata.len() != file.bytes
        {
            return Err(format!("检查点文件负载大小或类型无效：{}", file.path));
        }
        let bytes = fs::read(&payload_path).map_err(|error| error.to_string())?;
        if file.checksum != file_checksum(&bytes) {
            return Err(format!("检查点文件校验失败：{}", file.path));
        }
        if files.insert(path, bytes).is_some() {
            return Err("检查点包含重复文件路径".into());
        }
    }
    if let (Some(entries), Some(bases)) = (&manifest.text_base, &mut text_base) {
        for entry in entries {
            let path = parse_relative_file(&entry.path)?;
            if !path.extension().is_some_and(|extension| extension == "wl")
                || bases.contains_key(&path)
            {
                return Err("检查点文本基线路径无效或重复".into());
            }
            let bytes = match &entry.source {
                CheckpointTextBaseSource::Snapshot => files
                    .get(&path)
                    .cloned()
                    .ok_or_else(|| format!("检查点文本基线引用了不存在的快照文件：{}", entry.path))?,
                CheckpointTextBaseSource::Absent => {
                    bases.insert(path, None);
                    continue;
                }
                CheckpointTextBaseSource::Stored {
                    payload,
                    bytes,
                    checksum,
                } => {
                    let payload_path = validate_payload_path(payload)?;
                    if !payloads.insert(payload.clone()) {
                        return Err("检查点包含重复负载路径".into());
                    }
                    total = total.checked_add(*bytes).ok_or("检查点文件大小无效")?;
                    if total > DEFAULT_MAX_CHECKPOINT_BYTES as u64 {
                        return Err("检查点负载超过格式上限".into());
                    }
                    let path_on_disk = directory.join(payload_path);
                    let metadata = fs::symlink_metadata(&path_on_disk)
                        .map_err(|error| format!("检查点文本基线负载缺失：{error}"))?;
                    if crate::file_access::is_link_or_junction(&metadata)
                        || !metadata.is_file()
                        || metadata.len() != *bytes
                    {
                        return Err(format!("检查点文本基线负载大小或类型无效：{}", entry.path));
                    }
                    let data = fs::read(&path_on_disk).map_err(|error| error.to_string())?;
                    if *checksum != file_checksum(&data) {
                        return Err(format!("检查点文本基线校验失败：{}", entry.path));
                    }
                    data
                }
            };
            bases.insert(path, Some(bytes));
        }
    }
    let mut actual_payloads = BTreeSet::new();
    for entry in fs::read_dir(&payload_directory).map_err(|error| error.to_string())? {
        if actual_payloads.len() >= MAX_CHECKPOINT_PAYLOADS {
            return Err("检查点负载文件数超过格式上限".into());
        }
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_file() {
            return Err("检查点负载目录包含无效项".into());
        }
        actual_payloads.insert(entry.file_name().to_string_lossy().into_owned());
    }
    let actual_payloads = actual_payloads
        .into_iter()
        .map(|name| format!("files/{name}"))
        .collect::<BTreeSet<_>>();
    if actual_payloads != payloads || total != manifest.payload_bytes {
        return Err("检查点清单与负载集合不一致".into());
    }
    if files_digest(&files) != manifest.snapshot_digest {
        return Err("检查点整体校验失败".into());
    }
    if manifest.version == CHECKPOINT_FORMAT_VERSION
        && (manifest.text_base_digest.as_deref() != text_base_digest(text_base.as_ref()).as_deref()
            || manifest.text_base_digest.is_none())
    {
        return Err("检查点文本基线整体校验失败".into());
    }
    let mut actual_root_entries = BTreeSet::new();
    for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
        if actual_root_entries.len() >= 2 {
            return Err("检查点记录含有未登记文件".into());
        }
        let entry = entry.map_err(|error| error.to_string())?;
        actual_root_entries.insert(entry.file_name().to_string_lossy().into_owned());
    }
    if actual_root_entries != BTreeSet::from([MANIFEST_NAME.into(), "files".into()]) {
        return Err("检查点记录含有未登记文件".into());
    }
    Ok(CheckpointBundle {
        manifest,
        files,
        text_base,
    })
}

fn delete_checkpoint_record(root: &Path, id: &str) -> Result<(), String> {
    let _lock = lock_checkpoints()?;
    let directory = checkpoint_path(root, id);
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err("检查点不存在".into())
        }
        Err(error) => return Err(error.to_string()),
    };
    if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
        return Err("不能删除无效的检查点存储路径".into());
    }
    validate_checkpoint_directory(root, &directory)?;
    fs::remove_dir_all(&directory).map_err(|error| format!("无法删除检查点：{error}"))?;
    sync_directory(directory.parent().unwrap_or(root))
}

fn parse_relative_file(value: &str) -> Result<PathBuf, String> {
    let normalized = value.replace('\\', "/");
    let path = Path::new(&normalized);
    validate_relative_file(path)?;
    if normalized != value {
        return Err("检查点路径分隔符无效".into());
    }
    Ok(path.to_path_buf())
}

fn validate_payload_path(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if path.components().count() != 2
        || !matches!(path.components().next(), Some(Component::Normal(files)) if files == "files")
        || !matches!(path.components().nth(1), Some(Component::Normal(name)) if name.to_string_lossy().ends_with(".bin"))
    {
        return Err("检查点负载路径无效".into());
    }
    Ok(path.to_path_buf())
}

fn validate_relative_id(id: &str) -> Result<(), String> {
    validate_checkpoint_id(id)
}

fn unavailable_summary(id: &str, reason: &str, bytes: u64) -> CheckpointSummary {
    CheckpointSummary {
        id: id.into(),
        label: None,
        created_at_unix_ms: 0,
        file_count: 0,
        payload_bytes: bytes,
        available: false,
        unavailable_reason: Some(reason.into()),
    }
}

fn unavailable_payload_bytes(path: &Path) -> Result<u64, String> {
    let payload_directory = path.join("files");
    let metadata = match fs::symlink_metadata(&payload_directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.to_string()),
    };
    if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
        return Ok(DEFAULT_MAX_CHECKPOINT_BYTES as u64 + 1);
    }
    let mut total = 0u64;
    let mut count = 0;
    for entry in fs::read_dir(&payload_directory).map_err(|error| error.to_string())? {
        count += 1;
        if count > MAX_CHECKPOINT_PAYLOADS {
            return Ok(DEFAULT_MAX_CHECKPOINT_BYTES as u64 + 1);
        }
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_file() {
            return Ok(DEFAULT_MAX_CHECKPOINT_BYTES as u64 + 1);
        }
        total = total.saturating_add(metadata.len());
        if total > DEFAULT_MAX_CHECKPOINT_BYTES as u64 {
            return Ok(DEFAULT_MAX_CHECKPOINT_BYTES as u64 + 1);
        }
    }
    Ok(total)
}

fn ensure_checkpoint_directory(root: &Path, directory: &Path) -> Result<(), String> {
    let root = crate::compiler::source_path(root);
    if !directory.starts_with(&root) {
        return Err("检查点目录越出工作区边界".into());
    }
    if !root.exists() {
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    }
    validate_record_path_components(&root, directory, true)?;
    Ok(())
}

fn validate_checkpoint_directory(root: &Path, directory: &Path) -> Result<(), String> {
    let root = crate::compiler::source_path(root);
    validate_record_path_components(&root, directory, false)
}

fn validate_record_path_components(root: &Path, directory: &Path, create: bool) -> Result<(), String> {
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| "检查点目录越出工作区边界".to_string())?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("检查点目录路径无效".into());
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
                    return Err(format!("检查点路径不能经过链接或普通文件：{}", current.display()));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                fs::create_dir(&current).map_err(|error| error.to_string())?;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn remove_staging_directories(root: &Path, history: &Path) -> Result<(), String> {
    let mut count = 0usize;
    for entry in fs::read_dir(history).map_err(|error| error.to_string())? {
        count += 1;
        if count > MAX_CHECKPOINT_RECORDS_ON_DISK {
            return Err("检查点存储目录超过安全条目上限".into());
        }
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !entry.file_name().to_string_lossy().starts_with(STAGING_PREFIX) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
            return Err(format!("检查点暂存项无效：{}", path.display()));
        }
        validate_checkpoint_directory(root, &path)?;
        fs::remove_dir_all(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn write_checkpoint_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

fn persist_restored_files(
    root: &Path,
    pending: &[RestoreFile],
    _target: &Files,
) -> Result<(), String> {
    if pending.is_empty() {
        return Ok(());
    }
    let pending = pending
        .iter()
        .map(|file| crate::storage::PendingFile {
            relative: file.relative.clone(),
            before: file.before.clone(),
            after: file.after.clone(),
        })
        .collect::<Vec<_>>();
    crate::storage::save(root, &pending)
}

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

fn failure_requested(phase: &str) -> bool {
    let Some(value) = std::env::var("WORLDLINE_CHECKPOINT_FAIL_PHASE").ok() else {
        return false;
    };
    if let Ok(thread) = std::env::var("WORLDLINE_CHECKPOINT_FAIL_THREAD") {
        if thread != format!("{:?}", std::thread::current().id()) {
            return false;
        }
    }
    value == phase
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> Result<(), String> {
    fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("无法同步检查点目录：{} ({error})", directory.display()))
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> Result<(), String> {
    Ok(())
}

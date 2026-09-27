use super::capture::*;
use super::*;
use std::collections::HashSet;
use std::path::Path;
#[cfg(any(target_arch = "wasm32", test))]
pub(super) struct CheckpointSnapshotWriter {
    bytes: Vec<u8>,
    max_bytes: usize,
}

#[cfg(any(target_arch = "wasm32", test))]
impl CheckpointSnapshotWriter {
    pub(super) fn new(max_bytes: usize) -> Self {
        Self {
            bytes: Vec::new(),
            max_bytes,
        }
    }

    pub(super) fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > self.max_bytes.saturating_sub(self.bytes.len()) {
            return Err("检查点快照超过浏览器存储上限".into());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    pub(super) fn write_u8(&mut self, value: u8) -> Result<(), String> {
        self.write(&[value])
    }

    pub(super) fn write_u32(&mut self, value: usize) -> Result<(), String> {
        let value = u32::try_from(value).map_err(|_| "检查点快照字段超出格式上限")?;
        self.write(&value.to_le_bytes())
    }

    pub(super) fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        let len = u32::try_from(bytes.len()).map_err(|_| "检查点快照字段超出格式上限")?;
        self.write(&len.to_le_bytes())?;
        self.write(bytes)
    }

    pub(super) fn write_string(&mut self, value: &str) -> Result<(), String> {
        self.write_bytes(value.as_bytes())
    }

    pub(super) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(any(target_arch = "wasm32", test))]
struct CheckpointSnapshotReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

#[cfg(any(target_arch = "wasm32", test))]
impl<'a> CheckpointSnapshotReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self.offset.checked_add(len).ok_or("检查点快照长度无效")?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or("检查点快照内容不完整")?;
        self.offset = end;
        Ok(value)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        Ok(self.read(1)?[0])
    }

    fn read_u32(&mut self) -> Result<usize, String> {
        let value = u32::from_le_bytes(self.read(4)?.try_into().unwrap());
        Ok(value as usize)
    }

    fn read_bytes(&mut self, max_len: u64) -> Result<Vec<u8>, String> {
        let len = self.read_u32()? as u64;
        if len > max_len {
            return Err("检查点快照字段超过格式上限".into());
        }
        Ok(self.read(len as usize)?.to_vec())
    }

    fn read_string(&mut self, max_len: u64) -> Result<String, String> {
        String::from_utf8(self.read_bytes(max_len)?)
            .map_err(|_| "检查点快照路径不是有效 UTF-8".into())
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn decode_checkpoint_snapshot(bytes: &[u8]) -> Result<Vec<CheckpointBundle>, String> {
    if bytes.len() > MAX_CHECKPOINT_SNAPSHOT_BYTES {
        return Err("检查点快照超过格式上限".into());
    }
    let mut reader = CheckpointSnapshotReader::new(bytes);
    if reader.read(CHECKPOINT_SNAPSHOT_MAGIC.len())? != CHECKPOINT_SNAPSHOT_MAGIC {
        return Err("检查点快照格式或版本无效".into());
    }
    let count = reader.read_u32()?;
    if count > DEFAULT_MAX_CHECKPOINTS {
        return Err("检查点快照记录数超过格式上限".into());
    }
    let mut records = Vec::with_capacity(count);
    let mut ids = HashSet::new();
    let mut total_payload_bytes = 0u64;
    for _ in 0..count {
        let manifest_bytes = reader.read_bytes(MAX_CHECKPOINT_MANIFEST_BYTES)?;
        let manifest: CheckpointManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|error| format!("检查点快照清单无效：{error}"))?;
        let file_count = reader.read_u32()?;
        if file_count > MAX_CHECKPOINT_FILES {
            return Err("检查点快照文件数超过格式上限".into());
        }
        let mut files = Files::new();
        for _ in 0..file_count {
            let path = PathBuf::from(reader.read_string(32 * 1024)?);
            let bytes = reader.read_bytes(DEFAULT_MAX_CHECKPOINT_BYTES as u64)?;
            if files.insert(path, bytes).is_some() {
                return Err("检查点快照包含重复文件路径".into());
            }
        }
        let text_base = match reader.read_u8()? {
            0 => None,
            1 => {
                let base_count = reader.read_u32()?;
                if base_count > MAX_CHECKPOINT_FILES {
                    return Err("检查点快照文本基线数超过格式上限".into());
                }
                let mut text_base = BTreeMap::new();
                for _ in 0..base_count {
                    let path = PathBuf::from(reader.read_string(32 * 1024)?);
                    let entry = manifest
                        .text_base
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .find(|entry| Path::new(&entry.path) == path)
                        .ok_or("检查点快照清单缺少文本基线")?;
                    let value = match (&entry.source, reader.read_u8()?) {
                        (CheckpointTextBaseSource::Absent, 0) => None,
                        (CheckpointTextBaseSource::Snapshot, 1) => Some(
                            files
                                .get(&path)
                                .cloned()
                                .ok_or("检查点快照文本基线引用了缺失文件")?,
                        ),
                        (CheckpointTextBaseSource::Stored { .. }, 2) => {
                            Some(reader.read_bytes(DEFAULT_MAX_CHECKPOINT_BYTES as u64)?)
                        }
                        _ => return Err("检查点快照文本基线状态无效".into()),
                    };
                    if text_base.insert(path, value).is_some() {
                        return Err("检查点快照包含重复文本基线".into());
                    }
                }
                Some(text_base)
            }
            _ => return Err("检查点快照文本基线状态无效".into()),
        };
        let bundle = CheckpointBundle {
            manifest,
            files,
            text_base,
        };
        validate_transferred_bundle(&bundle)?;
        if !ids.insert(bundle.manifest.id.clone()) {
            return Err("检查点快照包含重复记录 ID".into());
        }
        total_payload_bytes = total_payload_bytes
            .checked_add(bundle.manifest.payload_bytes)
            .ok_or("检查点历史字节数超出可表示范围")?;
        if total_payload_bytes > DEFAULT_MAX_CHECKPOINT_HISTORY_BYTES as u64 {
            return Err("检查点快照历史字节数超过格式上限".into());
        }
        records.push(bundle);
    }
    if reader.offset != bytes.len() {
        return Err("检查点快照包含尾随数据".into());
    }
    Ok(records)
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn validate_transferred_bundle(bundle: &CheckpointBundle) -> Result<(), String> {
    let manifest = &bundle.manifest;
    validate_checkpoint_id(&manifest.id)?;
    if manifest
        .label
        .as_ref()
        .is_some_and(|label| label.chars().count() > 120)
        || manifest.files.len() > MAX_CHECKPOINT_FILES
    {
        return Err("检查点快照清单字段超过格式上限".into());
    }
    match manifest.version {
        LEGACY_CHECKPOINT_FORMAT_VERSION
            if manifest.text_base.is_none()
                && manifest.text_base_digest.is_none()
                && bundle.text_base.is_none() => {}
        CHECKPOINT_FORMAT_VERSION
            if manifest.text_base.is_some()
                && manifest.text_base_digest.is_some()
                && manifest
                    .text_base
                    .as_ref()
                    .is_some_and(|entries| entries.len() <= MAX_CHECKPOINT_FILES)
                && bundle.text_base.is_some() => {}
        _ => return Err("检查点快照记录版本或基线字段无效".into()),
    }
    validate_snapshot_files(&bundle.files)?;
    if bundle.files.len() != manifest.files.len() {
        return Err("检查点快照文件清单与负载数量不一致".into());
    }
    let mut payloads = HashSet::new();
    let mut file_paths = HashSet::new();
    for entry in &manifest.files {
        let path = parse_transferred_path(&entry.path)?;
        if !file_paths.insert(path.clone()) {
            return Err("检查点快照清单包含重复文件路径".into());
        }
        validate_checkpoint_payload_name(&entry.payload)?;
        if !payloads.insert(entry.payload.as_str()) {
            return Err("检查点快照包含重复负载".into());
        }
        let bytes = bundle.files.get(&path).ok_or("检查点快照缺少清单文件")?;
        if bytes.len() as u64 != entry.bytes || file_checksum(bytes) != entry.checksum {
            return Err("检查点快照文件校验失败".into());
        }
    }
    if file_paths.len() != bundle.files.len() {
        return Err("检查点快照包含清单外文件".into());
    }
    if let (Some(entries), Some(text_base)) = (&manifest.text_base, &bundle.text_base) {
        if entries.len() > MAX_CHECKPOINT_FILES || entries.len() != text_base.len() {
            return Err("检查点快照文本基线数量无效".into());
        }
        let mut base_paths = HashSet::new();
        for entry in entries {
            let path = parse_transferred_path(&entry.path)?;
            if !path.extension().is_some_and(|extension| extension == "wl") {
                return Err("检查点快照文本基线只能引用 .wl 文件".into());
            }
            if !base_paths.insert(path.clone()) {
                return Err("检查点快照包含重复文本基线路径".into());
            }
            let stored = text_base.get(&path).ok_or("检查点快照缺少文本基线")?;
            match (&entry.source, stored) {
                (CheckpointTextBaseSource::Absent, None) => {}
                (CheckpointTextBaseSource::Snapshot, Some(bytes))
                    if bundle.files.get(&path) == Some(bytes) => {}
                (
                    CheckpointTextBaseSource::Stored {
                        payload,
                        bytes: expected_bytes,
                        checksum,
                    },
                    Some(bytes),
                ) if bytes.len() as u64 == *expected_bytes && file_checksum(bytes) == *checksum => {
                    validate_checkpoint_payload_name(payload)?;
                    if !payloads.insert(payload.as_str()) {
                        return Err("检查点快照包含重复负载".into());
                    }
                }
                _ => return Err("检查点快照文本基线校验失败".into()),
            }
        }
        if base_paths.len() != text_base.len() {
            return Err("检查点快照包含清单外文本基线".into());
        }
    }
    let empty_text_base = BTreeMap::new();
    let payload_bytes = checkpoint_payload_bytes(
        &bundle.files,
        bundle.text_base.as_ref().unwrap_or(&empty_text_base),
    )?;
    if payload_bytes != manifest.payload_bytes
        || payload_bytes > DEFAULT_MAX_CHECKPOINT_BYTES as u64
        || files_digest(&bundle.files) != manifest.snapshot_digest
        || text_base_digest(bundle.text_base.as_ref()).as_deref()
            != manifest.text_base_digest.as_deref()
    {
        return Err("检查点快照摘要或字节数校验失败".into());
    }
    Ok(())
}

#[cfg(any(target_arch = "wasm32", test))]
fn parse_transferred_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    validate_relative_file(&path)?;
    Ok(path)
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn portable_path(path: &Path) -> Result<String, String> {
    let path = path
        .to_str()
        .ok_or_else(|| format!("检查点路径不是有效 UTF-8：{}", path.display()))?
        .replace('\\', "/");
    parse_transferred_path(&path)?;
    Ok(path)
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn validate_checkpoint_payload_name(payload: &str) -> Result<(), String> {
    let Some(name) = payload.strip_prefix("files/") else {
        return Err("检查点负载路径无效".into());
    };
    let number = name.strip_prefix("base-").unwrap_or(name);
    let bytes = number.as_bytes();
    if name.contains('/')
        || bytes.len() != 12
        || !bytes[..8].iter().all(u8::is_ascii_digit)
        || &bytes[8..] != b".bin"
    {
        return Err(format!("检查点负载路径无效：{payload}"));
    }
    Ok(())
}

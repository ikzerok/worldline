use super::capture::*;
use super::snapshot::*;
use super::*;
use std::collections::HashSet;
use std::path::Path;
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CheckpointScopeKey {
    root: PathBuf,
    session_id: String,
}

#[cfg(any(target_arch = "wasm32", test))]
impl CheckpointScopeKey {
    pub(super) fn new(root: &Path, session_id: &str) -> Self {
        Self {
            root: crate::compiler::source_path(root),
            session_id: session_id.to_owned(),
        }
    }
}

/// The browser production backend and its native regression test share this
/// exact store implementation; session identity is part of every lookup key.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default)]
pub(super) struct InMemoryCheckpointStore {
    pub(super) records: BTreeMap<CheckpointScopeKey, BTreeMap<String, CheckpointBundle>>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl InMemoryCheckpointStore {
    pub(super) fn publish(
        &mut self,
        scope: CheckpointScopeKey,
        manifest: CheckpointManifest,
        files: &Files,
        text_base: &BTreeMap<PathBuf, Option<Vec<u8>>>,
        limits: &CheckpointLimits,
    ) -> Result<(), String> {
        validate_checkpoint_id(&manifest.id)?;
        let records = self.records.entry(scope).or_default();
        if records.len() >= limits.max_count {
            return Err("检查点数量配额已满，请显式删除旧记录".into());
        }
        let used_bytes = records
            .values()
            .try_fold(0u64, |total, checkpoint| {
                total.checked_add(checkpoint.manifest.payload_bytes)
            })
            .ok_or("检查点历史字节数超出可表示范围")?;
        if used_bytes.saturating_add(manifest.payload_bytes) > limits.max_total_bytes as u64 {
            return Err("检查点历史字节配额已满，请显式删除旧记录".into());
        }
        if files_digest(files) != manifest.snapshot_digest
            || manifest.text_base_digest.as_deref() != text_base_digest(Some(text_base)).as_deref()
        {
            return Err("检查点内存记录摘要不一致".into());
        }
        records.insert(
            manifest.id.clone(),
            CheckpointBundle {
                manifest,
                files: files.clone(),
                text_base: Some(text_base.clone()),
            },
        );
        Ok(())
    }

    pub(super) fn list(&self, scope: &CheckpointScopeKey) -> Vec<CheckpointListing> {
        self.records
            .get(scope)
            .into_iter()
            .flat_map(|records| records.values())
            .map(|bundle| CheckpointListing {
                summary: summary_for_manifest(&bundle.manifest),
            })
            .collect()
    }

    pub(super) fn load(
        &self,
        scope: &CheckpointScopeKey,
        id: &str,
    ) -> Result<CheckpointBundle, String> {
        validate_checkpoint_id(id)?;
        self.records
            .get(scope)
            .and_then(|records| records.get(id))
            .cloned()
            .ok_or_else(|| "检查点不存在或已损坏".into())
    }

    pub(super) fn delete(&mut self, scope: &CheckpointScopeKey, id: &str) -> Result<(), String> {
        validate_checkpoint_id(id)?;
        let Some(records) = self.records.get_mut(scope) else {
            return Err("检查点不存在".into());
        };
        if records.remove(id).is_none() {
            return Err("检查点不存在".into());
        }
        if records.is_empty() {
            self.records.remove(scope);
        }
        Ok(())
    }

    pub(super) fn export_snapshot(
        &self,
        scope: &CheckpointScopeKey,
        max_bytes: usize,
    ) -> Result<Vec<u8>, String> {
        if max_bytes == 0 || max_bytes > MAX_CHECKPOINT_SNAPSHOT_BYTES {
            return Err("检查点快照上限无效".into());
        }
        let records = self.records.get(scope);
        let count = records.map_or(0, BTreeMap::len);
        if count > DEFAULT_MAX_CHECKPOINTS {
            return Err("检查点快照记录数超过格式上限".into());
        }
        let mut writer = CheckpointSnapshotWriter::new(max_bytes);
        writer.write(CHECKPOINT_SNAPSHOT_MAGIC)?;
        writer.write_u32(count)?;
        if let Some(records) = records {
            for bundle in records.values() {
                validate_transferred_bundle(bundle)?;
                let manifest = serde_json::to_vec(&bundle.manifest)
                    .map_err(|error| format!("无法编码检查点清单：{error}"))?;
                if manifest.len() as u64 > MAX_CHECKPOINT_MANIFEST_BYTES {
                    return Err("检查点清单超过格式上限".into());
                }
                writer.write_bytes(&manifest)?;
                writer.write_u32(bundle.files.len())?;
                for (path, bytes) in &bundle.files {
                    writer.write_string(&portable_path(path)?)?;
                    writer.write_bytes(bytes)?;
                }
                match &bundle.text_base {
                    None => writer.write_u8(0)?,
                    Some(text_base) => {
                        writer.write_u8(1)?;
                        writer.write_u32(text_base.len())?;
                        for (path, bytes) in text_base {
                            writer.write_string(&portable_path(path)?)?;
                            let entry = bundle
                                .manifest
                                .text_base
                                .as_deref()
                                .unwrap_or_default()
                                .iter()
                                .find(|entry| Path::new(&entry.path) == path)
                                .ok_or("检查点快照清单缺少文本基线")?;
                            match (&entry.source, bytes) {
                                (CheckpointTextBaseSource::Absent, None) => writer.write_u8(0)?,
                                (CheckpointTextBaseSource::Snapshot, Some(_)) => {
                                    writer.write_u8(1)?
                                }
                                (CheckpointTextBaseSource::Stored { .. }, Some(bytes)) => {
                                    writer.write_u8(2)?;
                                    writer.write_bytes(bytes)?;
                                }
                                _ => return Err("检查点快照文本基线校验失败".into()),
                            }
                        }
                    }
                }
            }
        }
        Ok(writer.finish())
    }

    pub(super) fn import_snapshot(
        &mut self,
        scope: CheckpointScopeKey,
        bytes: &[u8],
    ) -> Result<(), String> {
        let incoming = decode_checkpoint_snapshot(bytes)?;
        let existing = self.records.get(&scope);
        let mut used_bytes = existing
            .into_iter()
            .flat_map(BTreeMap::values)
            .try_fold(0u64, |total, bundle| {
                total.checked_add(bundle.manifest.payload_bytes)
            })
            .ok_or("检查点历史字节数超出可表示范围")?;
        let mut imported_ids = HashSet::new();
        let mut new_bundles = Vec::new();

        for bundle in incoming {
            validate_transferred_bundle(&bundle)?;
            if !imported_ids.insert(bundle.manifest.id.clone()) {
                return Err("检查点快照包含重复记录 ID".into());
            }
            if let Some(previous) = existing.and_then(|records| records.get(&bundle.manifest.id)) {
                if previous.manifest.snapshot_digest != bundle.manifest.snapshot_digest
                    || previous.manifest.text_base_digest != bundle.manifest.text_base_digest
                    || previous.files != bundle.files
                    || previous.text_base != bundle.text_base
                {
                    return Err("检查点快照与已有记录 ID 冲突".into());
                }
                continue;
            }
            if existing.map_or(0, BTreeMap::len) + new_bundles.len() >= DEFAULT_MAX_CHECKPOINTS {
                return Err("检查点数量配额已满，未导入快照".into());
            }
            used_bytes = used_bytes
                .checked_add(bundle.manifest.payload_bytes)
                .ok_or("检查点历史字节数超出可表示范围")?;
            if used_bytes > DEFAULT_MAX_CHECKPOINT_HISTORY_BYTES as u64 {
                return Err("检查点历史字节配额已满，未导入快照".into());
            }
            new_bundles.push(bundle);
        }

        if !new_bundles.is_empty() {
            self.records.entry(scope).or_default().extend(
                new_bundles
                    .into_iter()
                    .map(|bundle| (bundle.manifest.id.clone(), bundle)),
            );
        }
        Ok(())
    }
}

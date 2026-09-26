use std::cell::RefCell;

thread_local! {
    static WEB_CHECKPOINTS: RefCell<BTreeMap<PathBuf, BTreeMap<String, CheckpointBundle>>> =
        RefCell::new(BTreeMap::new());
}

fn publish_checkpoint(
    root: &Path,
    manifest: CheckpointManifest,
    files: &Files,
    limits: &CheckpointLimits,
) -> Result<(), String> {
    validate_checkpoint_id(&manifest.id)?;
    let root = crate::compiler::source_path(root);
    WEB_CHECKPOINTS.with(|storage| {
        let mut storage = storage.borrow_mut();
        let records = storage.entry(root).or_default();
        if records.len() >= limits.max_count {
            return Err("检查点数量配额已满，请显式删除旧记录".into());
        }
        let used_bytes = records.values().try_fold(0u64, |total, checkpoint| {
            total.checked_add(checkpoint.manifest.payload_bytes)
        }).ok_or("检查点历史字节数超出可表示范围")?;
        if used_bytes.saturating_add(manifest.payload_bytes) > limits.max_total_bytes as u64 {
            return Err("检查点历史字节配额已满，请显式删除旧记录".into());
        }
        if files_digest(files) != manifest.snapshot_digest {
            return Err("检查点内存记录摘要不一致".into());
        }
        records.insert(
            manifest.id.clone(),
            CheckpointBundle {
                manifest,
                files: files.clone(),
            },
        );
        Ok(())
    })
}

fn list_checkpoint_records(root: &Path) -> Result<Vec<CheckpointListing>, String> {
    let root = crate::compiler::source_path(root);
    WEB_CHECKPOINTS.with(|storage| {
        Ok(storage
            .borrow()
            .get(&root)
            .into_iter()
            .flat_map(|records| records.values())
            .map(|bundle| CheckpointListing {
                summary: summary_for_manifest(&bundle.manifest),
            })
            .collect())
    })
}

fn load_checkpoint(root: &Path, id: &str) -> Result<CheckpointBundle, String> {
    validate_checkpoint_id(id)?;
    let root = crate::compiler::source_path(root);
    WEB_CHECKPOINTS.with(|storage| {
        storage
            .borrow()
            .get(&root)
            .and_then(|records| records.get(id))
            .cloned()
            .ok_or_else(|| "检查点不存在或已损坏".into())
    })
}

fn delete_checkpoint_record(root: &Path, id: &str) -> Result<(), String> {
    validate_checkpoint_id(id)?;
    let root = crate::compiler::source_path(root);
    WEB_CHECKPOINTS.with(|storage| {
        let mut storage = storage.borrow_mut();
        let Some(records) = storage.get_mut(&root) else {
            return Err("检查点不存在".into());
        };
        if records.remove(id).is_none() {
            return Err("检查点不存在".into());
        }
        if records.is_empty() {
            storage.remove(&root);
        }
        Ok(())
    })
}

fn persist_restored_files(
    root: &Path,
    target: &Files,
) -> Result<(), String> {
    crate::file_access::replace_workspace_files(root, target)
}

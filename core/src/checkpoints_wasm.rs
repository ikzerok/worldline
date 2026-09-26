use std::cell::RefCell;

thread_local! {
    static WEB_CHECKPOINTS: RefCell<InMemoryCheckpointStore> =
        const { RefCell::new(InMemoryCheckpointStore { records: BTreeMap::new() }) };
}

fn publish_checkpoint(
    root: &Path,
    session_id: &str,
    manifest: CheckpointManifest,
    files: &Files,
    limits: &CheckpointLimits,
) -> Result<(), String> {
    let scope = CheckpointScopeKey::new(root, session_id);
    WEB_CHECKPOINTS.with(|storage| {
        storage
            .borrow_mut()
            .publish(scope, manifest, files, limits)
    })
}

fn list_checkpoint_records(
    root: &Path,
    session_id: &str,
) -> Result<Vec<CheckpointListing>, String> {
    let scope = CheckpointScopeKey::new(root, session_id);
    WEB_CHECKPOINTS.with(|storage| {
        Ok(storage.borrow().list(&scope))
    })
}

fn load_checkpoint(root: &Path, session_id: &str, id: &str) -> Result<CheckpointBundle, String> {
    let scope = CheckpointScopeKey::new(root, session_id);
    WEB_CHECKPOINTS.with(|storage| {
        storage.borrow().load(&scope, id)
    })
}

fn delete_checkpoint_record(root: &Path, session_id: &str, id: &str) -> Result<(), String> {
    let scope = CheckpointScopeKey::new(root, session_id);
    WEB_CHECKPOINTS.with(|storage| {
        storage.borrow_mut().delete(&scope, id)
    })
}

fn persist_restored_files(
    root: &Path,
    target: &Files,
) -> Result<(), String> {
    crate::file_access::replace_workspace_files(root, target)
}

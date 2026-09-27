// Checkpoint unit tests are grouped by browser storage, clock, and WASM session behavior.
#[cfg(test)]
mod browser_checkpoint_scope_tests {
    use crate::checkpoints::capture::{checkpoint_payload_bytes, make_manifest};
    use crate::checkpoints::snapshot::validate_checkpoint_payload_name;
    use crate::checkpoints::store::{CheckpointScopeKey, InMemoryCheckpointStore};
    use crate::checkpoints::{
        CheckpointBundle, CheckpointLimits, LEGACY_CHECKPOINT_FORMAT_VERSION,
        MAX_CHECKPOINT_SNAPSHOT_BYTES,
    };
    use crate::workspace_snapshot::Files;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    #[test]
    fn browser_checkpoint_history_isolated_by_session_at_a_shared_mount_root() {
        let files = Files::from([(PathBuf::from("world.wl"), b"event start\n".to_vec())]);
        let text_base = BTreeMap::new();
        let manifest = make_manifest(None, &files, &text_base, 12).unwrap();
        let checkpoint_id = manifest.id.clone();
        let limits = CheckpointLimits::default();
        let root = Path::new("/world");
        let session_a = CheckpointScopeKey::new(root, "browser-session-a");
        let session_b = CheckpointScopeKey::new(root, "browser-session-b");
        let same_session_after_reopen = CheckpointScopeKey::new(root, "browser-session-a");
        let mut store = InMemoryCheckpointStore::default();

        store
            .publish(session_a.clone(), manifest, &files, &text_base, &limits)
            .unwrap();

        assert_eq!(store.list(&session_a).len(), 1);
        assert!(store.list(&session_b).is_empty());
        assert!(store.load(&session_b, &checkpoint_id).is_err());
        assert_eq!(
            store
                .load(&same_session_after_reopen, &checkpoint_id)
                .unwrap()
                .files,
            files,
            "the same persisted browser session can reopen its own checkpoint"
        );
        assert!(store.delete(&session_b, &checkpoint_id).is_err());
        assert_eq!(store.list(&session_a).len(), 1);
        store.delete(&session_a, &checkpoint_id).unwrap();
        assert!(store.list(&session_a).is_empty());
    }

    #[test]
    fn browser_checkpoint_snapshot_roundtrips_content_and_rejects_corruption() {
        let files = Files::from([
            (PathBuf::from("world.wl"), b"event start\n".to_vec()),
            (PathBuf::from("notes.bin"), vec![0, 255, 7]),
        ]);
        let text_base =
            BTreeMap::from([(PathBuf::from("world.wl"), Some(b"event old\n".to_vec()))]);
        let payload_bytes = checkpoint_payload_bytes(&files, &text_base).unwrap();
        let manifest = make_manifest(
            Some("refresh recovery".into()),
            &files,
            &text_base,
            payload_bytes,
        )
        .unwrap();
        let checkpoint_id = manifest.id.clone();
        let scope = CheckpointScopeKey::new(Path::new("/world"), "browser-session-a");
        let mut source = InMemoryCheckpointStore::default();
        source
            .publish(
                scope.clone(),
                manifest,
                &files,
                &text_base,
                &CheckpointLimits::default(),
            )
            .unwrap();

        let encoded = source
            .export_snapshot(&scope, MAX_CHECKPOINT_SNAPSHOT_BYTES)
            .unwrap();
        let mut reopened = InMemoryCheckpointStore::default();
        reopened.import_snapshot(scope.clone(), &encoded).unwrap();
        let restored = reopened.load(&scope, &checkpoint_id).unwrap();
        assert_eq!(restored.files, files);
        assert_eq!(restored.text_base, Some(text_base));
        assert_eq!(reopened.list(&scope).len(), 1);

        let other_scope = CheckpointScopeKey::new(Path::new("/world"), "browser-session-b");
        assert!(reopened.list(&other_scope).is_empty());

        let mut corrupt = encoded;
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        assert!(reopened
            .import_snapshot(other_scope.clone(), &corrupt)
            .is_err());
        assert!(reopened.list(&other_scope).is_empty());
    }

    #[test]
    fn browser_checkpoint_snapshot_preserves_legacy_records_and_imports_atomically() {
        let files = Files::from([(PathBuf::from("world.wl"), b"event start\n".to_vec())]);
        let empty_text_base = BTreeMap::new();
        let payload_bytes = checkpoint_payload_bytes(&files, &empty_text_base).unwrap();
        let mut legacy_manifest =
            make_manifest(None, &files, &empty_text_base, payload_bytes).unwrap();
        legacy_manifest.version = LEGACY_CHECKPOINT_FORMAT_VERSION;
        legacy_manifest.text_base = None;
        legacy_manifest.text_base_digest = None;
        let legacy_id = legacy_manifest.id.clone();
        let scope = CheckpointScopeKey::new(Path::new("/world"), "browser-session-legacy");
        let mut source = InMemoryCheckpointStore::default();
        source.records.entry(scope.clone()).or_default().insert(
            legacy_id.clone(),
            CheckpointBundle {
                manifest: legacy_manifest,
                files: files.clone(),
                text_base: None,
            },
        );
        let modern_manifest = make_manifest(None, &files, &empty_text_base, payload_bytes).unwrap();
        source
            .publish(
                scope.clone(),
                modern_manifest,
                &files,
                &empty_text_base,
                &CheckpointLimits::default(),
            )
            .unwrap();

        let encoded = source
            .export_snapshot(&scope, MAX_CHECKPOINT_SNAPSHOT_BYTES)
            .unwrap();
        assert!(source.export_snapshot(&scope, 1).is_err());

        let mut reopened = InMemoryCheckpointStore::default();
        reopened.import_snapshot(scope.clone(), &encoded).unwrap();
        let legacy = reopened.load(&scope, &legacy_id).unwrap();
        assert_eq!(legacy.manifest.version, LEGACY_CHECKPOINT_FORMAT_VERSION);
        assert_eq!(legacy.files, files);
        assert_eq!(legacy.text_base, None);
        assert_eq!(reopened.list(&scope).len(), 2);

        // Corrupt the final record after a valid first one. Import validates the
        // complete snapshot before publishing any of its records.
        let mut corrupt = encoded;
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        let another_scope = CheckpointScopeKey::new(Path::new("/world"), "browser-session-atomic");
        assert!(reopened
            .import_snapshot(another_scope.clone(), &corrupt)
            .is_err());
        assert!(reopened.list(&another_scope).is_empty());
    }

    #[test]
    fn malformed_checkpoint_payload_names_never_panic_on_utf8_boundaries() {
        assert!(validate_checkpoint_payload_name("files/1234567ébin").is_err());
    }
}
#[cfg(test)]
mod checkpoint_clock_tests {
    use crate::checkpoints::capture::{make_manifest, next_checkpoint_id};
    use crate::workspace_snapshot::Files;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn checkpoint_ids_are_unique_and_manifest_times_are_available() {
        let files = Files::from([(PathBuf::from("world.wl"), b"event start\n".to_vec())]);
        let text_base = BTreeMap::new();
        let first = make_manifest(None, &files, &text_base, 12).unwrap();
        let second = make_manifest(None, &files, &text_base, 12).unwrap();

        assert_ne!(next_checkpoint_id(), next_checkpoint_id());
        assert_ne!(first.id, second.id);
        assert!(first.created_at_unix_ms > 0);
        assert!(second.created_at_unix_ms > 0);
    }
}
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_project_checkpoint_session_tests {
    use crate::checkpoints::CheckpointLimits;
    use crate::project::Project;
    use crate::workspace_snapshot::{self, Files};
    use std::path::{Path, PathBuf};

    fn mount(files: &Files) {
        crate::file_access::mount(
            files
                .iter()
                .map(|(path, bytes)| (Path::new("/world").join(path), bytes.clone()))
                .collect(),
        );
    }

    #[test]
    fn same_saved_session_keeps_history_but_a_new_project_at_world_cannot_restore_it() {
        let files = Files::from([(
            PathBuf::from("world.wl"),
            "event start\n  Original.\n  -> END\n".as_bytes().to_vec(),
        )]);
        mount(&files);
        let project = Project::open(Path::new("/world/world.wl")).unwrap();
        let session_id = project.checkpoint_session_id().to_owned();
        let checkpoint = project
            .create_checkpoint(None, CheckpointLimits::default())
            .unwrap();
        assert!(checkpoint.created_at_unix_ms > 0);
        let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();

        // Browser save/reopen restores the session identity alongside the package.
        mount(&files);
        let mut reopened = Project::open(Path::new("/world/world.wl")).unwrap();
        reopened
            .set_checkpoint_session_id(session_id.clone())
            .unwrap();
        assert_eq!(reopened.list_checkpoints().unwrap().len(), 1);
        reopened.restore_checkpoint(&plan).unwrap();

        // A second work can have the exact same mounted root and bytes; its fresh
        // Project identity still hides the old history and invalidates the plan.
        mount(&files);
        let mut other = Project::open(Path::new("/world/world.wl")).unwrap();
        assert_ne!(other.checkpoint_session_id(), session_id);
        assert!(other.list_checkpoints().unwrap().is_empty());
        let before = workspace_snapshot::snapshot_files(&other).unwrap();
        assert!(other.restore_checkpoint(&plan).is_err());
        assert_eq!(workspace_snapshot::snapshot_files(&other).unwrap(), before);
    }
}

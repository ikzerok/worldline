use super::*;

#[test]
fn second_disk_change_between_preview_and_apply_is_rejected() {
    let fixture = Fixture::new();
    let mut project = fixture.conflict();
    let plan = plan(&project, ReconciliationChoice::Local);
    fs::write(&project.entry, "event newest\n  -> END\n").unwrap();
    let before = project.content_baseline();
    assert!(project.apply_reconciliation(&plan).is_err());
    assert_eq!(project.content_baseline(), before);
    assert_eq!(
        project
            .tracked_file_state(&project.entry)
            .unwrap()
            .baseline
            .as_deref(),
        Some(BASE.as_bytes())
    );
}

#[test]
fn before_commit_race_and_prepared_delivery_race_are_rejected() {
    let fixture = Fixture::new();
    let mut project = fixture.conflict();
    let plan = plan(&project, ReconciliationChoice::Local);
    let path = project.entry.clone();
    let before = project.content_baseline();
    assert!(project
        .apply_reconciliation_with_progress(&plan, &mut |stage| {
            if stage == ReconciliationStage::BeforeCommit {
                fs::write(&path, MERGED).unwrap();
            }
            true
        })
        .is_err());
    assert_eq!(project.content_baseline(), before);
    fs::write(&path, DISK).unwrap();
    let prepared = project
        .prepare_reconciliation_with_progress(&plan, &mut |_| true)
        .unwrap();
    fs::write(fixture.root.join("new-attachment.bin"), [9]).unwrap();
    assert!(project.commit_prepared_reconciliation(prepared).is_err());
    assert_eq!(project.content_baseline(), before);
}

#[test]
fn changed_unrelated_bytes_and_local_baseline_invalidate_exact_evidence() {
    let fixture = Fixture::new();
    let asset = fixture.root.join("asset.bin");
    fs::write(&asset, [1, 2]).unwrap();
    let mut project = fixture.conflict();
    let plan = plan(&project, ReconciliationChoice::Disk);
    fs::write(&asset, [3, 4]).unwrap();
    assert!(project.apply_reconciliation(&plan).is_err());
    fs::write(&asset, [1, 2]).unwrap();
    project.mark_saved();
    assert!(project.apply_reconciliation(&plan).is_err());
}

#[test]
fn cancellation_never_changes_buffers_or_advances_baseline() {
    let fixture = Fixture::new();
    let mut project = fixture.conflict();
    let plan = plan(&project, ReconciliationChoice::Disk);
    let before = project.content_baseline();
    for cancelled in [
        ReconciliationStage::Capture,
        ReconciliationStage::Candidate,
        ReconciliationStage::Validate,
        ReconciliationStage::Revalidate,
        ReconciliationStage::BeforeCommit,
    ] {
        assert!(project
            .apply_reconciliation_with_progress(&plan, &mut |stage| stage != cancelled)
            .is_err());
        assert_eq!(project.content_baseline(), before);
        assert_eq!(fs::read(&project.entry).unwrap(), DISK.as_bytes());
    }
}

#[test]
fn forged_plan_candidate_and_can_apply_do_not_authorize_changes() {
    let fixture = Fixture::new();
    let mut project = fixture.conflict();
    let original = plan(&project, ReconciliationChoice::Local);
    let mut modified = original.clone();
    modified.files[0].result = Some(MERGED.as_bytes().to_vec());
    assert!(project.apply_reconciliation(&modified).is_err());
    let mut modified = original;
    modified.problems.entries.clear();
    modified.plan_digest = "fake".into();
    assert!(project.apply_reconciliation(&modified).is_err());
    let session = project.capture_reconciliation().unwrap();
    let mut unresolved = project
        .preview_reconciliation(&session, &ReconciliationRequest::default())
        .unwrap();
    unresolved.can_apply = true;
    assert!(project.apply_reconciliation(&unresolved).is_err());
    assert_eq!(project.document(&project.entry).unwrap(), LOCAL);
}

#[test]
fn invalid_utf8_and_unknown_manifest_are_protected_even_for_local_choice() {
    let fixture = Fixture::new();
    let mut project = fixture.conflict();
    fs::write(&project.entry, [0xff, 0x00]).unwrap();
    let session = project.capture_reconciliation().unwrap();
    assert!(session.files[0]
        .protected_reason
        .as_ref()
        .unwrap()
        .contains("UTF-8"));
    let plan = project
        .preview_reconciliation(&session, &request(ReconciliationChoice::Local))
        .unwrap();
    assert!(!plan.can_apply);
    assert!(project.apply_reconciliation(&plan).is_err());
    assert_eq!(fs::read(&project.entry).unwrap(), [0xff, 0]);
    fs::write(&project.entry, DISK).unwrap();
    fs::create_dir_all(fixture.root.join(".world")).unwrap();
    fs::write(
        fixture.root.join(".world/project.json"),
        br#"{"schema_version":1,"required_features":["future.magic.v99"]}"#,
    )
    .unwrap();
    let session = project.capture_reconciliation().unwrap();
    assert!(!session.blockers.is_empty());
    let plan = project
        .preview_reconciliation(&session, &request(ReconciliationChoice::Local))
        .unwrap();
    assert!(!plan.can_apply);
}

#[test]
fn unresolved_transactions_never_become_ordinary_reconciliation() {
    let fixture = Fixture::new();
    let mut project = fixture.conflict();
    let plan = plan(&project, ReconciliationChoice::Local);
    fs::create_dir_all(fixture.root.join(".world/.transactions/unfinished")).unwrap();
    let journal = fixture
        .root
        .join(".world/.transactions/unfinished/journal.json");
    fs::write(&journal, b"{}").unwrap();
    assert!(project.apply_reconciliation(&plan).is_err());
    assert_eq!(fs::read(&journal).unwrap(), b"{}");
    assert_eq!(fs::read(&project.entry).unwrap(), DISK.as_bytes());
    assert!(!project
        .capture_reconciliation()
        .unwrap()
        .blockers
        .is_empty());
}

#[test]
fn two_files_are_all_or_nothing_and_no_non_conflict_paths_are_accepted() {
    let fixture = Fixture::new();
    let extra = fixture.root.join("extra.wl");
    fs::write(&extra, "event extra\n  -> END\n").unwrap();
    let mut project = fixture.conflict();
    project
        .set_text(&extra, "event extra\n  本地\n  -> END\n".into())
        .unwrap();
    fs::write(&extra, "event extra\n  磁盘\n  -> END\n").unwrap();
    let session = project.capture_reconciliation().unwrap();
    assert_eq!(session.files.len(), 2);
    let plan = project
        .preview_reconciliation(&session, &request(ReconciliationChoice::Local))
        .unwrap();
    assert!(!plan.can_apply);
    assert!(project.apply_reconciliation(&plan).is_err());
    let mut bad = request(ReconciliationChoice::Local);
    bad.choices.push(ReconciliationDecision {
        path: "../outside.wl".into(),
        choice: ReconciliationChoice::Delete,
    });
    assert!(project.preview_reconciliation(&session, &bad).is_err());
}

#[test]
fn save_after_new_external_change_fails_without_reverting_adopted_candidate() {
    let fixture = Fixture::new();
    let mut project = fixture.conflict();
    let plan = plan(
        &project,
        ReconciliationChoice::Manual {
            text: MERGED.into(),
        },
    );
    project.apply_reconciliation(&plan).unwrap();
    fs::write(&project.entry, BASE).unwrap();
    assert!(project.save().is_err());
    assert_eq!(project.document(&project.entry).unwrap(), MERGED);
    assert_eq!(fs::read(&project.entry).unwrap(), BASE.as_bytes());
}

#[cfg(unix)]
#[test]
fn readonly_file_and_hardlink_alias_are_protected() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let project = fixture.conflict();
    fs::set_permissions(&project.entry, fs::Permissions::from_mode(0o444)).unwrap();
    let session = project.capture_reconciliation().unwrap();
    assert!(session.files[0].protected_reason.is_some());
    fs::set_permissions(&project.entry, fs::Permissions::from_mode(0o644)).unwrap();
    fs::hard_link(&project.entry, fixture.root.join("alias.wl")).unwrap();
    assert!(project.capture_reconciliation().is_err());
}

#[test]
fn missing_nested_scan_is_not_treated_as_missing_root() {
    let fixture = Fixture::new();
    let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "扫描期间子目录消失");
    assert!(!capture::root_is_missing(&fixture.root, &missing));
    assert!(capture::root_is_missing(
        &fixture.root.join("missing-root"),
        &missing
    ));
    let denied = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "拒绝读取");
    assert!(!capture::root_is_missing(
        &fixture.root.join("missing-root"),
        &denied
    ));
}

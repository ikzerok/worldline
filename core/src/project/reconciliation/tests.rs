use super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
mod guards;
mod performance;
mod protocol;

const BASE: &str = "event start\n  基线\n  -> END\n";
const LOCAL: &str = "event start\n  本地 🌧️\n  -> END\n";
const DISK: &str = "event start\n  外部 🌬️\n  -> END\n";
const MERGED: &str = "event start\n  手工核对的雨与风 🌧️🌬️\n  -> END\n";

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "worldline-reconciliation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("world.wl"), BASE).unwrap();
        Self { root }
    }
    fn conflict(&self) -> Project {
        let mut project = Project::open_read_only(&self.root).unwrap();
        project
            .set_text(&project.entry.clone(), LOCAL.into())
            .unwrap();
        fs::write(&project.entry, DISK).unwrap();
        project
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn request(choice: ReconciliationChoice) -> ReconciliationRequest {
    ReconciliationRequest {
        choices: vec![ReconciliationDecision {
            path: PathBuf::from("world.wl"),
            choice,
        }],
        allow_incomplete_source: false,
    }
}
fn plan(project: &Project, choice: ReconciliationChoice) -> ReconciliationPlan {
    let session = project.capture_reconciliation().unwrap();
    project
        .preview_reconciliation(&session, &request(choice))
        .unwrap()
}

#[test]
fn manual_preview_and_apply_are_exact_and_never_save() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("untouched.bin"), [0, 255, 3]).unwrap();
    let mut project = fixture.conflict();
    let before = project.content_baseline();
    let session = project.capture_reconciliation().unwrap();
    assert_eq!(session.files.len(), 1);
    assert_eq!(session.files[0].baseline.as_deref(), Some(BASE.as_bytes()));
    assert_eq!(session.files[0].local.as_deref(), Some(LOCAL.as_bytes()));
    assert_eq!(session.files[0].disk.as_deref(), Some(DISK.as_bytes()));
    let plan = project
        .preview_reconciliation(
            &session,
            &request(ReconciliationChoice::Manual {
                text: MERGED.into(),
            }),
        )
        .unwrap();
    assert!(plan.can_apply, "{:?}", plan.blockers);
    assert_eq!(project.content_baseline(), before);
    assert_eq!(fs::read(&project.entry).unwrap(), DISK.as_bytes());
    let applied = project.apply_reconciliation(&plan).unwrap();
    assert_eq!(project.document(&project.entry).unwrap(), MERGED);
    assert_eq!(
        project
            .tracked_file_state(&project.entry)
            .unwrap()
            .baseline
            .as_deref(),
        Some(DISK.as_bytes())
    );
    assert!(project.is_dirty());
    assert_eq!(fs::read(&project.entry).unwrap(), DISK.as_bytes());
    assert_eq!(
        fs::read(fixture.root.join("untouched.bin")).unwrap(),
        [0, 255, 3]
    );
    assert!(project.apply_reconciliation(&plan).is_err());
    assert!(project.restore(applied.undo));
    assert_eq!(project.document(&project.entry).unwrap(), LOCAL);
    assert_eq!(
        project
            .tracked_file_state(&project.entry)
            .unwrap()
            .baseline
            .as_deref(),
        Some(DISK.as_bytes())
    );
}

#[test]
fn save_then_undo_redo_keep_latest_disk_baseline_and_reopen() {
    let fixture = Fixture::new();
    let mut project = fixture.conflict();
    let old_history = project.clone();
    let plan = plan(
        &project,
        ReconciliationChoice::Manual {
            text: MERGED.into(),
        },
    );
    let applied = project.apply_reconciliation(&plan).unwrap();
    assert!(!project.restore(old_history));
    let redo = project.clone();
    project.save().unwrap();
    assert_eq!(fs::read(&project.entry).unwrap(), MERGED.as_bytes());
    assert!(project.restore(applied.undo));
    assert_eq!(project.document(&project.entry).unwrap(), LOCAL);
    assert_eq!(
        project
            .tracked_file_state(&project.entry)
            .unwrap()
            .baseline
            .as_deref(),
        Some(MERGED.as_bytes())
    );
    assert!(project.is_dirty());
    assert!(project.restore(redo));
    assert!(!project.is_dirty());
    assert_eq!(
        Project::open_read_only(&fixture.root)
            .unwrap()
            .document(&project.entry)
            .unwrap(),
        MERGED
    );
}

#[test]
fn accepting_disk_can_be_clean_but_undo_is_unsaved_local() {
    let fixture = Fixture::new();
    let mut project = fixture.conflict();
    let applied = project
        .apply_reconciliation(&plan(&project, ReconciliationChoice::Disk))
        .unwrap();
    assert!(!project.is_dirty());
    assert!(project.restore(applied.undo));
    assert!(project.is_dirty());
    project.save().unwrap();
    assert_eq!(fs::read(&project.entry).unwrap(), LOCAL.as_bytes());
}

#[test]
fn missing_and_empty_remain_distinct_in_delete_modify_conflict() {
    let fixture = Fixture::new();
    let extra = fixture.root.join("extra.wl");
    fs::write(&extra, "event extra\n  -> END\n").unwrap();
    let mut project = Project::open_read_only(&fixture.root).unwrap();
    project.delete_document(&extra).unwrap();
    fs::write(&extra, []).unwrap();
    let session = project.capture_reconciliation().unwrap();
    assert_eq!(session.files[0].local, None);
    assert_eq!(session.files[0].disk, Some(Vec::new()));
    let request = ReconciliationRequest {
        choices: vec![ReconciliationDecision {
            path: "extra.wl".into(),
            choice: ReconciliationChoice::Delete,
        }],
        allow_incomplete_source: false,
    };
    let plan = project.preview_reconciliation(&session, &request).unwrap();
    assert!(plan.can_apply, "{:?}", plan.blockers);
    let applied = project.apply_reconciliation(&plan).unwrap();
    assert_eq!(
        project.tracked_file_state(&extra).unwrap().baseline,
        Some(Vec::new())
    );
    assert_eq!(fs::read(&extra).unwrap(), Vec::<u8>::new());
    project.save().unwrap();
    assert!(!extra.exists());
    assert!(project.restore(applied.undo));
    assert_eq!(project.tracked_file_state(&extra).unwrap().baseline, None);
}

#[test]
fn external_deletion_local_recreation_is_explicit_and_guarded() {
    let fixture = Fixture::new();
    let mut project = fixture.conflict();
    fs::remove_file(&project.entry).unwrap();
    let plan = plan(&project, ReconciliationChoice::Local);
    assert_eq!(plan.session.files[0].disk, None);
    project.apply_reconciliation(&plan).unwrap();
    assert!(!project.entry.exists());
    assert_eq!(
        project.tracked_file_state(&project.entry).unwrap().baseline,
        None
    );
    project.save().unwrap();
    assert_eq!(fs::read(&project.entry).unwrap(), LOCAL.as_bytes());
}

#[test]
fn no_default_side_and_bad_source_requires_explicit_unfinished_choice() {
    let fixture = Fixture::new();
    let mut project = fixture.conflict();
    let session = project.capture_reconciliation().unwrap();
    let empty = project
        .preview_reconciliation(&session, &ReconciliationRequest::default())
        .unwrap();
    assert_eq!(empty.unresolved, 1);
    assert!(!empty.can_apply);
    let mut input = request(ReconciliationChoice::Manual {
        text: "event start\n  -> missing\n".into(),
    });
    let blocked = project.preview_reconciliation(&session, &input).unwrap();
    assert!(blocked.source_has_errors);
    assert!(!blocked.can_apply);
    input.allow_incomplete_source = true;
    let allowed = project.preview_reconciliation(&session, &input).unwrap();
    assert!(allowed.can_apply, "{:?}", allowed.blockers);
    assert!(allowed.source_has_errors);
    project.apply_reconciliation(&allowed).unwrap();
    assert_eq!(fs::read(&project.entry).unwrap(), DISK.as_bytes());
}

#[test]
fn unrelated_preexisting_source_errors_can_be_kept_explicitly() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("unfinished.wl"),
        "event unfinished\n  -> unknown\n",
    )
    .unwrap();
    let project = fixture.conflict();
    let session = project.capture_reconciliation().unwrap();
    let mut input = request(ReconciliationChoice::Disk);
    input.allow_incomplete_source = true;
    let plan = project.preview_reconciliation(&session, &input).unwrap();
    assert!(plan.source_has_errors);
    assert!(plan.can_apply, "{:?}", plan.blockers);
}

#[test]
fn json_side_preserves_unknown_fields_spacing_and_registered_identity() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join(".world")).unwrap();
    let manifest = fixture.root.join(".world/project.json");
    let base = br#"{"schema_version":1,"extension":{"base":true}}"#;
    let local = br#"{ "schema_version":1, "extension":{"local":true,"unknown":[1,2,3]} }"#;
    let disk = br#"{"schema_version":1,"extension":{"disk":true}}"#;
    fs::write(&manifest, base).unwrap();
    let mut project = Project::open_read_only(&fixture.root).unwrap();
    project
        .set_authoring_document(&manifest, local.to_vec())
        .unwrap();
    fs::write(&manifest, disk).unwrap();
    let session = project.capture_reconciliation().unwrap();
    let request = ReconciliationRequest {
        choices: vec![ReconciliationDecision {
            path: ".world/project.json".into(),
            choice: ReconciliationChoice::Local,
        }],
        allow_incomplete_source: false,
    };
    let plan = project.preview_reconciliation(&session, &request).unwrap();
    assert!(plan.can_apply, "{:?}", plan.blockers);
    project.apply_reconciliation(&plan).unwrap();
    assert_eq!(
        project.authoring_document(&manifest).unwrap().bytes(),
        local
    );
    assert_eq!(fs::read(&manifest).unwrap(), disk);
    project.save().unwrap();
    assert_eq!(fs::read(&manifest).unwrap(), local);
}

#[test]
fn manifest_new_registration_undo_never_deletes_external_file() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join(".world/maps")).unwrap();
    let manifest = fixture.root.join(".world/project.json");
    let map = fixture.root.join(".world/maps/external.json");
    let base = br#"{"schema_version":1,"required_features":["presentation.maps.v1"],"maps":{},"extension":"base"}"#;
    let local = br#"{"schema_version":1,"required_features":["presentation.maps.v1"],"maps":{},"extension":"local"}"#;
    let disk = br#"{"schema_version":1,"required_features":["presentation.maps.v1"],"maps":{"external":".world/maps/external.json"},"extension":"disk"}"#;
    let external = br#"{"schema_version":1,"id":"external","title":"External map","canvas":{"width":100,"height":100,"unit":"normalized"},"layers":{},"layer_order":[],"placements":{},"unknown":{"preserve":true}}"#;
    fs::write(&manifest, base).unwrap();
    fs::write(&map, external).unwrap();
    let mut project = Project::open_read_only(&fixture.root).unwrap();
    assert!(!project.authoring_documents.contains_key(&map));
    project
        .set_authoring_document(&manifest, local.to_vec())
        .unwrap();
    fs::write(&manifest, disk).unwrap();
    let session = project.capture_reconciliation().unwrap();
    let request = ReconciliationRequest {
        choices: vec![ReconciliationDecision {
            path: ".world/project.json".into(),
            choice: ReconciliationChoice::Disk,
        }],
        allow_incomplete_source: false,
    };
    let plan = project.preview_reconciliation(&session, &request).unwrap();
    assert!(
        plan.can_apply,
        "{:?} {:?}",
        plan.blockers, plan.problems.entries
    );
    let applied = project.apply_reconciliation(&plan).unwrap();
    let redo = project.clone();
    assert!(project.restore(applied.undo));
    assert_eq!(
        project.authoring_document(&manifest).unwrap().bytes(),
        local
    );
    assert!(!project.authoring_document(&map).unwrap().is_dirty());
    assert!(!project.authoring_document(&map).unwrap().is_deleted());
    project.save().unwrap();
    assert_eq!(fs::read(&map).unwrap(), external);
    assert_eq!(fs::read(&manifest).unwrap(), local);
    assert!(project.restore(redo));
    project.save().unwrap();
    assert_eq!(fs::read(&manifest).unwrap(), disk);
    assert_eq!(fs::read(&map).unwrap(), external);
}

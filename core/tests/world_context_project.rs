use worldline_core::{project::Project, TargetRef, WorldContextError, WorldContextOptions};
#[path = "../../cli/tests/support/world_context_fixture.rs"]
mod fixture;

#[test]
fn refresh_changes_snapshot_and_rejects_old_context_and_time_evidence() {
    let fixture = fixture::Fixture::new();
    let mut project = Project::open(&fixture.root).unwrap();
    let target = TargetRef::new("character", "b");
    let old = project
        .query_world_context(&target, Default::default())
        .unwrap();
    let baseline = project.content_baseline();
    assert_eq!(old.content_baseline.as_deref(), Some(baseline.as_str()));
    std::fs::write(
        fixture.root.join("world.wl"),
        format!("{}// external\n", fixture::SOURCE),
    )
    .unwrap();
    assert!(project.refresh().unwrap().is_empty());
    assert_eq!(
        project
            .query_world_context(
                &target,
                WorldContextOptions {
                    expected_snapshot: Some(old.snapshot),
                    ..Default::default()
                }
            )
            .unwrap_err(),
        WorldContextError::StaleSnapshot
    );
    assert!(project
        .compare_temporal_events("first", "second", Some(&baseline))
        .unwrap_err()
        .starts_with("STALE_BASELINE"));
    assert!(project
        .compare_temporal_events("first", "second", Some(&project.content_baseline()))
        .is_ok());
}
#[test]
fn dirty_external_conflict_cannot_reaffirm_time_evidence_on_unchanged_buffer_baseline() {
    let fixture = fixture::Fixture::new();
    let mut project = Project::open(&fixture.root).unwrap();
    let path = project.entry.clone();
    project
        .set_text(&path, format!("{}// local\n", fixture::SOURCE))
        .unwrap();
    let baseline = project.content_baseline();
    std::fs::write(&path, format!("{}// external\n", fixture::SOURCE)).unwrap();
    assert!(!project.refresh().unwrap().is_empty());
    assert_eq!(project.content_baseline(), baseline);
    let target = TargetRef::new("character", "b");
    let immutable = project
        .compile()
        .query_world_context(&target, Default::default())
        .unwrap();
    assert!(immutable.complete);
    let projection = project
        .query_world_context(&target, Default::default())
        .unwrap();
    assert!(!projection.complete);
    assert!(!projection.truncated);
    assert!(projection
        .reasons
        .contains(&worldline_core::WorldContextLimit::SourceConflict));
    assert_eq!(projection.returned, immutable.returned);
    assert_eq!(projection.snapshot, immutable.snapshot);
    assert_eq!(project.content_baseline(), baseline);
    assert!(project
        .compare_temporal_events("first", "second", Some(&baseline))
        .unwrap_err()
        .starts_with("CONFLICT"));
    assert!(project.documents[&path].text.contains("// local"));
    assert!(std::fs::read_to_string(path)
        .unwrap()
        .contains("// external"));
}

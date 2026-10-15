use super::*;
use crate::draft_rehearsal::{run_draft_rehearsal, DraftRehearsal, DraftRehearsalRunRequest};
use crate::{OwnedStory, ReplayBudget, ReplayCancellation, Value};
use worldline_core::{draft_rehearsal::DraftRehearsalRequest, project::Project, TargetRef};

const APPLIED: &str = include_str!("../../tests/fixtures/object_state_inspection_v036/applied.wl");
const DRAFT: &str = include_str!("../../tests/fixtures/object_state_inspection_v036/draft.wl");

#[test]
fn object_inspection_normal_and_draft_runs_keep_their_own_catalogs_and_sources() {
    let root = std::env::temp_dir().join(format!(
        "runtime-object-inspection-v036-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let mut project = Project::new(&root);
    project
        .set_text(&project.entry.clone(), APPLIED.into())
        .unwrap();
    let compiled = project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let mut ordinary = OwnedStory::new_with_seed(compiled.program, compiled.analysis, 3).unwrap();
    let mut buffer = project.open_source_writing_buffer(&project.entry).unwrap();
    buffer.replace_source(DRAFT.into());
    let request = DraftRehearsalRequest::from_writing_buffers(
        &project,
        std::slice::from_ref(&buffer),
        vec![],
        false,
    )
    .unwrap();
    let mut draft =
        DraftRehearsal::new(project.compile_draft_rehearsal(&request).unwrap(), 3).unwrap();
    let q = StateInspectionQuery {
        owner: Some(TargetRef::new("world", "room")),
        ..Default::default()
    };
    ordinary.continue_story().unwrap();
    draft
        .continue_bounded(ReplayBudget::new(100, 1000), &ReplayCancellation::new())
        .unwrap();
    let baseline = project.content_baseline();
    let normal = ordinary.inspect_state(&q).unwrap();
    let rehearsed = draft.inspect_state(&q).unwrap();
    assert_eq!(normal.owner.as_ref().unwrap().display, "已应用地点");
    assert_eq!(rehearsed.owner.as_ref().unwrap().display, "草稿地点");
    assert_eq!(
        normal.items[0]
            .state_definition
            .as_ref()
            .unwrap()
            .initial
            .value,
        Some(Value::TagSet(vec!["calm".into()]))
    );
    assert_eq!(
        rehearsed.items[0]
            .state_definition
            .as_ref()
            .unwrap()
            .initial
            .value,
        Some(Value::TagSet(vec!["alert".into()]))
    );
    assert_eq!(normal.items[0].current, rehearsed.items[0].current);
    assert_ne!(normal.stamp.run_id, rehearsed.stamp.run_id);
    let mut stale = q.clone();
    stale.expected_stamp = Some(normal.stamp);
    assert_eq!(
        draft.inspect_state(&stale).unwrap_err().code,
        "STALE_INSPECTION"
    );
    let mut machine_request = DraftRehearsalRunRequest {
        presentation: None,
        input: request,
        seed: 3,
        budget: ReplayBudget::new(100, 1000),
        choice_ids: vec![],
        inspection: q.clone(),
    };
    let mut machine = run_draft_rehearsal(&project, &machine_request, &ReplayCancellation::new())
        .unwrap()
        .inspection
        .unwrap();
    assert_ne!(machine.stamp.run_id, rehearsed.stamp.run_id);
    machine.stamp = rehearsed.stamp;
    assert_eq!(machine, rehearsed);
    machine_request.inspection.group = Some(InspectionGroup::Global);
    assert!(run_draft_rehearsal(&project, &machine_request, &ReplayCancellation::new()).is_err());
    assert_eq!(project.content_baseline(), baseline);
    buffer.replace_source("event start\n  修改中的草稿\n  -> END\n".into());
    assert!(draft
        .snapshot()
        .verify_current(&project, std::slice::from_ref(&buffer), false)
        .is_err());
    assert_eq!(draft.inspect_state(&q).unwrap(), rehearsed);
    project
        .set_text(
            &project.entry.clone(),
            "world room as \"新稿无状态\"\nevent start\n  -> END\n".into(),
        )
        .unwrap();
    assert_eq!(ordinary.inspect_state(&q).unwrap(), normal);
    assert_eq!(draft.inspect_state(&q).unwrap(), rehearsed);
    assert!(draft
        .snapshot()
        .verify_current(&project, std::slice::from_ref(&buffer), false)
        .is_err());
}

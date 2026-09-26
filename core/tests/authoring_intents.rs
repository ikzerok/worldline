use std::fs;
use worldline_core::authoring::EntityDraft;
use worldline_core::authoring_intents::{AuthoringIntent, IntentTarget, TextSelection};
use worldline_core::project::Project;

fn project(name: &str) -> Project {
    let root = std::env::temp_dir().join(format!("worldline-intent-{name}-{}", std::process::id()));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), "event start\n  你看见灯塔😀。\n").unwrap();
    fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.10","required_features":[],"extension":{"keep":true}}"#).unwrap();
    Project::open(&root).unwrap()
}

fn intent(project: &Project) -> AuthoringIntent {
    let text = project.document(&project.entry).unwrap();
    let start = text.find("灯塔😀").unwrap();
    AuthoringIntent {
        expected_baseline: project.content_baseline(),
        target: IntentTarget::CreateEntity {
            path: project.entry.clone(),
            draft: EntityDraft {
                id: "tower".into(),
                entity_type: "place".into(),
                display: "灯塔😀".into(),
                ..Default::default()
            },
        },
        selection: Some(TextSelection {
            path: project.entry.clone(),
            start,
            end: start + "灯塔😀".len(),
            expected_text: "灯塔😀".into(),
        }),
        placement: None,
    }
}

#[test]
fn create_and_link_is_one_previewable_and_reversible_intent() {
    let mut project = project("create");
    let before = project.clone();
    let baseline = project.content_baseline();
    let fingerprint = project.compile().analysis.fingerprint;
    let command = intent(&project);
    let preview = project.preview_authoring_intent(&command).unwrap();
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.is_dirty());
    assert_eq!(preview.changed_files, vec![project.entry.clone()]);
    project.apply_authoring_intent(&command).unwrap();
    let after = project.clone();
    let compiled = project.compile();
    assert!(!compiled.has_errors());
    assert_eq!(compiled.analysis.catalog.text_links.len(), 1);
    assert_eq!(compiled.analysis.fingerprint, fingerprint);
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("你看见[[entity:tower|灯塔😀]]。"));
    assert!(project.restore(before));
    assert_eq!(project.content_baseline(), baseline);
    assert!(project.restore(after));
    assert_eq!(project.compile().analysis.catalog.text_links.len(), 1);
}

#[test]
fn map_creation_and_entity_are_atomic_and_preserve_unknown_fields() {
    use worldline_core::authoring_intents::PlacementRequest;
    use worldline_core::map_creation::{self, CreateMapCommand, CreateMapRequest};
    use worldline_core::presentation_commands::{document_hash, Revision};
    let mut project = project("map");
    let manifest = project.root.join(".world/project.json");
    let expected_documents = [(
        manifest.clone(),
        document_hash(project.authoring_document(&manifest).unwrap().bytes()),
    )]
    .into();
    map_creation::apply(
        &mut project,
        &mut Revision::default(),
        CreateMapCommand {
            expected_revision: Revision::default(),
            expected_documents,
            request: CreateMapRequest::new("atlas", "地图", 100, 100),
        },
    )
    .unwrap();
    let mut command = intent(&project);
    command.selection = None;
    command.placement = Some(PlacementRequest {
        map_id: "atlas".into(),
        placement_id: "tower_marker".into(),
        layer_id: "places".into(),
        geometry: worldline_core::MapGeometry::Point {
            position: [0.2, 0.3],
        },
        annotation: "灯塔入口".into(),
        role: "reference".into(),
        label_override: None,
    });
    let baseline = project.content_baseline();
    command.placement.as_mut().unwrap().layer_id = "missing".into();
    assert!(project.apply_authoring_intent(&command).is_err());
    assert_eq!(project.content_baseline(), baseline);
    command.placement.as_mut().unwrap().layer_id = "places".into();
    let result = project.apply_authoring_intent(&command).unwrap();
    assert_eq!(result.changed_files.len(), 2);
    assert_eq!(
        project.map_index().maps["atlas"].placements["tower_marker"]
            .target_ref
            .as_ref()
            .unwrap()
            .id,
        "tower"
    );
    let raw: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&manifest).unwrap().bytes()).unwrap();
    assert_eq!(raw["extension"]["keep"], true);
}

#[test]
fn selecting_a_comment_cannot_report_a_successful_body_link() {
    let mut project = project("comment");
    project
        .set_text(
            &project.entry.clone(),
            "// 灯塔😀\nevent start\n  正文。\n".into(),
        )
        .unwrap();
    assert!(!project.compile().has_errors());
    let baseline = project.content_baseline();
    let command = intent(&project);
    assert!(project.apply_authoring_intent(&command).is_err());
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn external_edit_before_refresh_rejects_the_entire_intent() {
    let mut project = project("external");
    let command = intent(&project);
    let baseline = project.content_baseline();
    fs::write(&project.entry, "event start\n  外部的新稿。\n").unwrap();
    assert!(project.apply_authoring_intent(&command).is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(
        fs::read_to_string(&project.entry).unwrap(),
        "event start\n  外部的新稿。\n"
    );
}

#[test]
fn reuse_keeps_same_name_objects_distinct_and_preserves_unselected_source() {
    let mut project = project("reuse");
    let source = concat!(
        "character tower as \"灯塔😀\"\n",
        "entity tower kind place as \"灯塔😀\"\n",
        "alias entity tower as \"白塔\"\n",
        "event start\n  你看见灯塔😀。 // 保留\\转义\n",
    );
    project
        .set_text(&project.entry.clone(), source.into())
        .unwrap();
    assert!(!project.compile().has_errors());
    let mut command = intent(&project);
    let start = source.find("你看见").unwrap() + "你看见".len();
    command.selection.as_mut().unwrap().start = start;
    command.selection.as_mut().unwrap().end = start + "灯塔😀".len();
    command.target = IntentTarget::Existing(worldline_core::TargetRef::new("entity", "tower"));
    project.apply_authoring_intent(&command).unwrap();
    assert_eq!(
        project.document(&project.entry).unwrap(),
        source.replacen("你看见灯塔😀", "你看见[[entity:tower|灯塔😀]]", 1)
    );
    let compiled = project.compile();
    assert_eq!(
        compiled.analysis.catalog.text_links[0].target.kind,
        "entity"
    );
    assert_eq!(compiled.analysis.catalog.aliases.len(), 1);
}

#[test]
fn invalid_or_stale_selection_never_creates_an_orphan_entity() {
    let mut project = project("invalid");
    let baseline = project.content_baseline();
    let original = intent(&project);
    let mut cases = Vec::new();
    let mut command = original.clone();
    command.expected_baseline = "old".into();
    cases.push(command);
    let mut command = original.clone();
    command.selection.as_mut().unwrap().start += 1;
    cases.push(command);
    let mut command = original.clone();
    command.selection.as_mut().unwrap().expected_text = "旧稿".into();
    cases.push(command);
    let mut command = original.clone();
    command.selection.as_mut().unwrap().path = project.root.parent().unwrap().join("private.wl");
    cases.push(command);
    let mut command = original.clone();
    command.target = IntentTarget::Existing(worldline_core::TargetRef::new("entity", "absent"));
    cases.push(command);
    for command in cases {
        assert!(project.apply_authoring_intent(&command).is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
}

#[test]
fn legacy_and_unknown_required_capabilities_stay_protected() {
    for (name, manifest) in [
        (
            "legacy",
            r#"{"schema_version":1,"language_version":"1.9","required_features":[]}"#,
        ),
        (
            "unknown",
            r#"{"schema_version":1,"language_version":"1.10","required_features":["future.v1"]}"#,
        ),
    ] {
        let mut project = project(name);
        fs::write(project.root.join(".world/project.json"), manifest).unwrap();
        project.refresh().unwrap();
        let baseline = project.content_baseline();
        let command = intent(&project);
        assert!(project.apply_authoring_intent(&command).is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
}

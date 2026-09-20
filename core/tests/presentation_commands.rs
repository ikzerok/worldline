use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use worldline_core::catalog::TargetRef;
use worldline_core::presentation::MapGeometry;
use worldline_core::presentation_commands::{
    apply, apply_with_content, document_hash, map_index_with_content, undo, Command,
    CommandEnvelope, EditError, Revision,
};
use worldline_core::project::Project;

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "worldline-presentation-commands-{name}-{}",
        std::process::id()
    ))
}

fn project_with_map(name: &str) -> (Project, PathBuf) {
    let root = temp_root(name);
    let _ = fs::remove_dir_all(&root);
    let mut project = Project::new(&root);
    let root = project.root.clone();
    project
        .create_authoring_document(
            &root.join(".world/project.json"),
            r#"{"schema_version":1,"maps":{"harbor":".world/maps/harbor.json"},"unknown_manifest":{"kept":true}}"#
                .as_bytes()
                .to_vec(),
        )
        .unwrap();
    project
        .create_authoring_document(
            &root.join(".world/maps/harbor.json"),
            r#"{
              "schema_version":1,
              "id":"harbor",
              "title":"港口",
              "canvas":{"width":1000,"height":800,"unit":"normalized","canvas_unknown":{"keep":"yes"}},
              "layer_order":["places"],
              "layers":{"places":{"title":"地点","visible_default":true,"locked":false,"layer_unknown":{"keep":1}}},
              "placements":{
                "lighthouse":{"layer_id":"places","target_ref":null,"geometry":{"kind":"point","position":[0.2,0.3]},"annotation":"灯塔","role":"地点入口","placement_unknown":{"keep":true}}
              },
              "extensions":{"future":{"kept":true}},
              "root_unknown":{"keep":true}
            }"#
                .as_bytes()
                .to_vec(),
        )
        .unwrap();
    (project, root)
}

fn envelope(project: &Project, revision: Revision, command: Command) -> CommandEnvelope {
    let path = project.root.join(".world/maps/harbor.json");
    let mut expected_documents = std::collections::BTreeMap::new();
    expected_documents.insert(
        path,
        document_hash(
            project
                .authoring_document(&project.root.join(".world/maps/harbor.json"))
                .unwrap()
                .bytes(),
        ),
    );
    CommandEnvelope {
        expected_revision: revision,
        expected_documents,
        command,
    }
}

fn map_source(project: &Project) -> Value {
    project
        .authoring_document(&project.root.join(".world/maps/harbor.json"))
        .unwrap()
        .bytes()
        .pipe(|bytes| serde_json::from_slice(bytes).unwrap())
}

trait Pipe: Sized {
    fn pipe<T>(self, function: impl FnOnce(Self) -> T) -> T {
        function(self)
    }
}
impl<T> Pipe for T {}

#[test]
fn update_placement_is_one_reversible_presentation_command_and_keeps_unknown_fields() {
    let (mut project, root) = project_with_map("update");
    let mut revision = Revision::default();
    let before = document_hash(
        project
            .authoring_document(&root.join(".world/maps/harbor.json"))
            .unwrap()
            .bytes(),
    );
    let expected_revision = revision;
    let request = envelope(
        &project,
        expected_revision,
        Command::UpdatePlacement {
            map_id: "harbor".into(),
            placement_id: "lighthouse".into(),
            geometry: Some(MapGeometry::point([0.7, 0.8])),
            target_ref: None,
            annotation: None,
            role: None,
            label_override: None,
            layer_id: None,
        },
    );
    let result = apply(&mut project, &mut revision, request).unwrap();
    assert_eq!(
        result.changed_files,
        vec![root.join(".world/maps/harbor.json")]
    );
    assert_eq!(revision.presentation_generation, 1);
    assert_ne!(
        document_hash(
            project
                .authoring_document(&root.join(".world/maps/harbor.json"))
                .unwrap()
                .bytes(),
        ),
        before
    );
    let source = map_source(&project);
    assert_eq!(source["root_unknown"]["keep"], true);
    assert_eq!(source["canvas"]["canvas_unknown"]["keep"], "yes");
    assert_eq!(
        source["placements"]["lighthouse"]["placement_unknown"]["keep"],
        true
    );
    assert_eq!(
        source["placements"]["lighthouse"]["geometry"]["position"],
        serde_json::json!([0.7, 0.8])
    );

    let second_request = envelope(
        &project,
        revision,
        Command::UpdatePlacement {
            map_id: "harbor".into(),
            placement_id: "lighthouse".into(),
            geometry: Some(MapGeometry::point([0.6, 0.7])),
            target_ref: None,
            annotation: None,
            role: None,
            label_override: None,
            layer_id: None,
        },
    );
    let second = apply(&mut project, &mut revision, second_request).unwrap();
    let undo_revision = revision;
    undo(
        &mut project,
        &mut revision,
        undo_revision,
        &second.undo_record,
    )
    .unwrap();
    let undo_revision = revision;
    undo(
        &mut project,
        &mut revision,
        undo_revision,
        &result.undo_record,
    )
    .unwrap();
    assert_eq!(revision.presentation_generation, 4);
    assert_eq!(revision.workspace_generation, 0);
    assert_eq!(revision.content_generation, 0);
    assert_eq!(
        document_hash(
            project
                .authoring_document(&root.join(".world/maps/harbor.json"))
                .unwrap()
                .bytes()
        ),
        before
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn create_and_delete_placement_validate_layer_and_target_without_recompiling_story() {
    let (mut project, root) = project_with_map("create-delete");
    let mut revision = Revision::default();
    let expected_revision = revision;
    let request = envelope(
        &project,
        expected_revision,
        Command::CreatePlacement {
            map_id: "harbor".into(),
            placement_id: "shore".into(),
            layer_id: "places".into(),
            target_ref: None,
            geometry: MapGeometry::point([0.4, 0.5]),
            annotation: "海岸入口".into(),
            role: "说明".into(),
            label_override: Some("海岸".into()),
        },
    );
    let created = apply(&mut project, &mut revision, request).unwrap();
    assert_eq!(revision.presentation_generation, 1);
    assert!(project.map_index().maps["harbor"]
        .placements
        .contains_key("shore"));

    let expected_revision = revision;
    let request = envelope(
        &project,
        expected_revision,
        Command::CreatePlacement {
            map_id: "harbor".into(),
            placement_id: "bad-ref".into(),
            layer_id: "places".into(),
            target_ref: Some(TargetRef::new("character", "missing")),
            geometry: MapGeometry::point([0.1, 0.1]),
            annotation: "断链".into(),
            role: "说明".into(),
            label_override: None,
        },
    );
    let error = apply(&mut project, &mut revision, request).unwrap_err();
    assert!(matches!(error, EditError::MissingReference { .. }));
    assert_eq!(revision.presentation_generation, 1);

    let expected_revision = revision;
    let request = envelope(
        &project,
        expected_revision,
        Command::DeletePlacement {
            map_id: "harbor".into(),
            placement_id: "shore".into(),
        },
    );
    apply(&mut project, &mut revision, request).unwrap();
    assert!(!project.map_index().maps["harbor"]
        .placements
        .contains_key("shore"));
    assert_eq!(revision.presentation_generation, 2);
    let _ = created;
    let _ = fs::remove_dir_all(root);
}

#[test]
fn layer_lock_and_stale_revision_reject_writes() {
    let (mut project, root) = project_with_map("guards");
    let mut revision = Revision::default();
    let expected_revision = revision;
    let request = envelope(
        &project,
        expected_revision,
        Command::CreateLayer {
            map_id: "harbor".into(),
            layer_id: "labels".into(),
            title: "标签".into(),
            visible_default: true,
            locked: false,
        },
    );
    apply(&mut project, &mut revision, request).unwrap();
    let expected_revision = revision;
    let request = envelope(
        &project,
        expected_revision,
        Command::SetLayer {
            map_id: "harbor".into(),
            layer_id: "places".into(),
            title: None,
            visible_default: None,
            locked: None,
            layer_order: Some(vec!["labels".into(), "places".into()]),
        },
    );
    apply(&mut project, &mut revision, request).unwrap();
    assert_eq!(
        project.map_index().maps["harbor"].layer_order,
        vec!["labels", "places"]
    );
    let expected_revision = revision;
    let request = envelope(
        &project,
        expected_revision,
        Command::SetLayer {
            map_id: "harbor".into(),
            layer_id: "places".into(),
            title: None,
            visible_default: None,
            locked: Some(true),
            layer_order: None,
        },
    );
    apply(&mut project, &mut revision, request).unwrap();
    assert_eq!(revision.presentation_generation, 3);

    let expected_revision = revision;
    let request = envelope(
        &project,
        expected_revision,
        Command::UpdatePlacement {
            map_id: "harbor".into(),
            placement_id: "lighthouse".into(),
            geometry: Some(MapGeometry::point([0.9, 0.9])),
            target_ref: None,
            annotation: None,
            role: None,
            label_override: None,
            layer_id: None,
        },
    );
    let locked = apply(&mut project, &mut revision, request).unwrap_err();
    assert!(matches!(locked, EditError::ReadOnlyFeature { .. }));

    let stale_request = envelope(
        &project,
        Revision::default(),
        Command::SetLayer {
            map_id: "harbor".into(),
            layer_id: "places".into(),
            title: Some("地点".into()),
            visible_default: None,
            locked: None,
            layer_order: None,
        },
    );
    let stale = apply(&mut project, &mut revision, stale_request).unwrap_err();
    assert!(matches!(stale, EditError::StaleRevision { .. }));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn expected_document_hash_detects_external_change_before_apply() {
    let (mut project, root) = project_with_map("external");
    let mut revision = Revision::default();
    let path = root.join(".world/maps/harbor.json");
    let mut expected_documents = std::collections::BTreeMap::new();
    expected_documents.insert(path.clone(), "stale".into());
    let expected_revision = revision;
    let error = apply(
        &mut project,
        &mut revision,
        CommandEnvelope {
            expected_revision,
            expected_documents,
            command: Command::DeletePlacement {
                map_id: "harbor".into(),
                placement_id: "lighthouse".into(),
            },
        },
    )
    .unwrap_err();
    assert!(matches!(error, EditError::ExternalConflict { .. }));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_map_command_requires_the_actual_map_document_baseline() {
    let (mut project, root) = project_with_map("required-baseline");
    let mut revision = Revision::default();
    let expected_revision = revision;
    let error = apply(
        &mut project,
        &mut revision,
        CommandEnvelope {
            expected_revision,
            expected_documents: Default::default(),
            command: Command::DeletePlacement {
                map_id: "harbor".into(),
                placement_id: "lighthouse".into(),
            },
        },
    )
    .unwrap_err();
    assert!(matches!(error, EditError::ExternalConflict { .. }));
    assert_eq!(revision, Revision::default());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_old_undo_intent_cannot_overwrite_later_commands_that_restore_the_same_bytes() {
    let (mut project, root) = project_with_map("undo-aba");
    let mut revision = Revision::default();
    let mut first_record = None;
    let mut undo_revision = Revision::default();
    let mut first_bytes = Vec::new();
    for (index, position) in [[0.7, 0.8], [0.6, 0.7], [0.7, 0.8]].into_iter().enumerate() {
        let request = envelope(
            &project,
            revision,
            Command::UpdatePlacement {
                map_id: "harbor".into(),
                placement_id: "lighthouse".into(),
                geometry: Some(MapGeometry::point(position)),
                target_ref: None,
                annotation: None,
                role: None,
                label_override: None,
                layer_id: None,
            },
        );
        let result = apply(&mut project, &mut revision, request).unwrap();
        if index == 0 {
            undo_revision = revision;
            first_bytes = project
                .authoring_document(&root.join(".world/maps/harbor.json"))
                .unwrap()
                .bytes()
                .to_vec();
            first_record = Some(result.undo_record);
        }
    }
    assert_eq!(
        project
            .authoring_document(&root.join(".world/maps/harbor.json"))
            .unwrap()
            .bytes(),
        first_bytes
    );
    let current_revision = revision;
    let error = undo(
        &mut project,
        &mut revision,
        undo_revision,
        &first_record.unwrap(),
    )
    .unwrap_err();
    assert!(matches!(error, EditError::StaleRevision { .. }));
    assert_eq!(revision, current_revision);
    assert_eq!(
        project
            .authoring_document(&root.join(".world/maps/harbor.json"))
            .unwrap()
            .bytes(),
        first_bytes
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cached_content_accepts_line_geometry_and_declares_the_feature() {
    let (mut project, root) = project_with_map("line-feature");
    let mut revision = Revision::default();
    let content = project.compile();
    let request = envelope(
        &project,
        revision,
        Command::CreatePlacement {
            map_id: "harbor".into(),
            placement_id: "route".into(),
            layer_id: "places".into(),
            target_ref: None,
            geometry: MapGeometry::Polyline {
                points: vec![[0.1, 0.1], [0.8, 0.8]],
            },
            annotation: "路线".into(),
            role: "说明".into(),
            label_override: None,
        },
    );
    apply_with_content(&mut project, &mut revision, request, &content).unwrap();
    let source = map_source(&project);
    assert_eq!(
        source["required_features"][0],
        serde_json::json!("presentation.geometry.line_area.v1")
    );
    assert!(map_index_with_content(&project, &content).maps["harbor"]
        .placements
        .contains_key("route"));
    let _ = fs::remove_dir_all(root);
}

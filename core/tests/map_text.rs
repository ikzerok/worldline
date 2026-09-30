use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use worldline_core::presentation::MapGeometry;
use worldline_core::presentation_commands::{
    apply, document_hash, undo, Command, CommandEnvelope, EditError, Revision,
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

fn text_geometry(text: &str) -> MapGeometry {
    MapGeometry::Text {
        position: [0.2, 0.3],
        text: text.into(),
        font_size: 24.0,
        color: "#224466".into(),
    }
}
fn create(text: &str) -> Command {
    Command::CreatePlacement {
        map_id: "harbor".into(),
        placement_id: "label".into(),
        layer_id: "places".into(),
        target_ref: None,
        geometry: text_geometry(text),
        annotation: "文字标签".into(),
        role: "文字标签".into(),
        label_override: None,
    }
}
#[test]
fn text_creation_declares_capability_and_undo_restores_original_bytes() {
    let (mut project, root) = project_with_map("text-create");
    let before = project
        .authoring_document(&root.join(".world/maps/harbor.json"))
        .unwrap()
        .bytes()
        .to_vec();
    let mut revision = Revision::default();
    let request = envelope(&project, revision, create("北境\n山脉"));
    let result = apply(&mut project, &mut revision, request).unwrap();
    let source = map_source(&project);
    assert_eq!(
        source["required_features"][0],
        "presentation.geometry.text.v1"
    );
    assert_eq!(
        source["placements"]["label"]["geometry"]["text"],
        "北境\n山脉"
    );
    assert!(project.map_index().diagnostics.is_empty());
    undo(
        &mut project,
        &mut revision,
        result.new_revision,
        &result.undo_record,
    )
    .unwrap();
    assert_eq!(
        project
            .authoring_document(&root.join(".world/maps/harbor.json"))
            .unwrap()
            .bytes(),
        before
    );
}
#[test]
fn invalid_text_does_not_mutate_or_advance_revision() {
    for (i, text) in ["", "  ", "a\rb", "a\tb", "1\n2\n3\n4\n5", &"字".repeat(161)]
        .iter()
        .enumerate()
    {
        let (mut project, _) = project_with_map(&format!("text-invalid-{i}"));
        let before = map_source(&project);
        let mut revision = Revision::default();
        let request = envelope(&project, revision, create(text));
        assert!(matches!(
            apply(&mut project, &mut revision, request),
            Err(EditError::InvalidGeometry { .. })
        ));
        assert_eq!(map_source(&project), before);
        assert_eq!(revision, Revision::default());
    }
    assert!(worldline_core::presentation::valid_map_text(
        &"字".repeat(160),
        12.0,
        "#abcdef"
    ));
    for size in [0.0, 11.0, 65.0, f64::NAN, f64::INFINITY] {
        assert!(!worldline_core::presentation::valid_map_text(
            "正常", size, "#abcdef"
        ));
    }
    for color in ["red", "#123", "#12zz34", "url(x)", "#中文"] {
        assert!(!worldline_core::presentation::valid_map_text(
            "正常", 24.0, color
        ));
    }
}
#[test]
fn missing_or_unknown_capability_refuses_map_without_discarding_bytes() {
    let (mut project, root) = project_with_map("text-capability");
    let mut source = map_source(&project);
    source["placements"]["lighthouse"]["geometry"] =
        serde_json::to_value(text_geometry("大陆")).unwrap();
    let path = root.join(".world/maps/harbor.json");
    project
        .set_authoring_document(&path, serde_json::to_vec(&source).unwrap())
        .unwrap();
    assert!(project.map_index().maps.is_empty());
    assert!(project
        .map_index()
        .diagnostics
        .iter()
        .any(|d| d.code == "MAP002"));
    source["required_features"] = serde_json::json!(["presentation.geometry.text.v999"]);
    let bytes = serde_json::to_vec(&source).unwrap();
    assert!(project
        .set_authoring_document(&path, bytes.clone())
        .is_err());
    assert!(project.map_index().maps.is_empty());
    assert_ne!(project.authoring_document(&path).unwrap().bytes(), bytes);
}

#[test]
fn label_update_preserves_unknown_geometry_fields_and_rejects_stale_or_locked() {
    let (mut project, root) = project_with_map("text-update");
    let mut revision = Revision::default();
    let request = envelope(&project, revision, create("初稿"));
    apply(&mut project, &mut revision, request).unwrap();
    let path = root.join(".world/maps/harbor.json");
    let mut source = map_source(&project);
    source["placements"]["label"]["geometry"]["future"] = serde_json::json!({"retained":true});
    project
        .set_authoring_document(&path, serde_json::to_vec(&source).unwrap())
        .unwrap();
    let update = Command::UpdatePlacement {
        map_id: "harbor".into(),
        placement_id: "label".into(),
        geometry: Some(text_geometry("修改后")),
        target_ref: None,
        annotation: None,
        role: None,
        label_override: None,
        layer_id: None,
    };
    let stale = envelope(&project, Revision::default(), update.clone());
    assert!(matches!(
        apply(&mut project, &mut revision, stale),
        Err(EditError::StaleRevision { .. })
    ));
    let request = envelope(&project, revision, update.clone());
    apply(&mut project, &mut revision, request).unwrap();
    assert_eq!(
        map_source(&project)["placements"]["label"]["geometry"]["future"]["retained"],
        true
    );
    let mut source = map_source(&project);
    source["layers"]["places"]["locked"] = true.into();
    project
        .set_authoring_document(&path, serde_json::to_vec(&source).unwrap())
        .unwrap();
    let request = envelope(&project, revision, update);
    assert!(matches!(
        apply(&mut project, &mut revision, request),
        Err(EditError::ReadOnlyFeature { .. })
    ));
    assert_eq!(map_source(&project), source);
}

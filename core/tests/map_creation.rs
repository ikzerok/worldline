use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use worldline_core::map_creation::{
    apply, undo, CreateMapCommand, CreateMapRequest, MISSING_DOCUMENT_HASH,
};
use worldline_core::presentation_commands::{document_hash, EditError, Revision};
use worldline_core::project::Project;

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "worldline-map-creation-{name}-{}",
        std::process::id()
    ))
}

fn old_project(name: &str) -> (Project, PathBuf) {
    let root = temp_root(name);
    let _ = fs::remove_dir_all(&root);
    let project = Project::new(&root);
    let root = project.root.clone();
    (project, root)
}

fn manifest_path(root: &Path) -> PathBuf {
    root.join(".world/project.json")
}

fn map_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!(".world/maps/{id}.json"))
}

fn expected_manifest(project: &Project) -> BTreeMap<PathBuf, String> {
    let path = manifest_path(&project.root);
    let mut expected = BTreeMap::new();
    expected.insert(
        path.clone(),
        document_hash(project.authoring_document(&path).unwrap().bytes()),
    );
    expected
}

fn command(
    revision: Revision,
    request: CreateMapRequest,
    expected_documents: BTreeMap<PathBuf, String>,
) -> CreateMapCommand {
    CreateMapCommand {
        expected_revision: revision,
        expected_documents,
        request,
    }
}

fn missing_manifest_baseline(project: &Project) -> BTreeMap<PathBuf, String> {
    let mut expected = BTreeMap::new();
    expected.insert(manifest_path(&project.root), MISSING_DOCUMENT_HASH.into());
    expected
}

#[test]
fn old_project_gets_manifest_and_empty_map_without_compiling_story() {
    let (mut project, root) = old_project("legacy");
    let source_before = project.sources();
    project
        .set_text(&project.entry.clone(), "这不是可编译的故事输入".into())
        .unwrap();
    let source_after_edit = project.sources();
    let mut revision = Revision::default();
    let request = command(
        revision,
        CreateMapRequest::new("harbor", "雾港地图", 1200, 800),
        missing_manifest_baseline(&project),
    );

    let result = apply(&mut project, &mut revision, request).unwrap();

    assert_eq!(source_before.len(), source_after_edit.len());
    assert_eq!(project.sources(), source_after_edit);
    assert_eq!(revision.presentation_generation, 1);
    assert_eq!(
        result.changed_files,
        vec![manifest_path(&root), map_path(&root, "harbor")]
    );

    let manifest: Value = serde_json::from_slice(
        project
            .authoring_document(&manifest_path(&root))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert!(manifest["project_id"]
        .as_str()
        .is_some_and(|id| !id.is_empty()
            && id.chars().enumerate().all(|(index, character)| {
                character.is_ascii_alphanumeric()
                    || character == '_'
                    || (character == '-' && index > 0)
            })
            && manifest["project_id"]
                .as_str()
                .unwrap()
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')));
    assert_eq!(manifest["maps"]["harbor"], ".world/maps/harbor.json");
    assert_eq!(manifest["required_features"][0], "presentation.maps.v1");

    let map: Value = serde_json::from_slice(
        project
            .authoring_document(&map_path(&root, "harbor"))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(map["id"], "harbor");
    assert_eq!(map["title"], "雾港地图");
    assert_eq!(map["canvas"]["width"], 1200);
    assert_eq!(map["canvas"]["height"], 800);
    assert_eq!(map["raster_layers"], serde_json::json!([]));
    assert_eq!(map["placements"], serde_json::json!({}));
    assert_eq!(map["layer_order"], serde_json::json!(["places"]));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn invalid_registered_map_entry_blocks_creation_without_mutation() {
    let (mut project, root) = old_project("invalid-registration");
    let manifest = manifest_path(&root);
    project
        .create_authoring_document(
            &manifest,
            br#"{
              "schema_version": 1,
              "project_id": "legacy",
              "language_version": "1.9",
              "entry": "world.wl",
              "required_features": ["presentation.maps.v1"],
              "maps": {"bad_value": 42, "bad_path": "../bad.txt"},
              "graph_views": {}
            }"#
            .to_vec(),
        )
        .unwrap();
    let before = project
        .authoring_document(&manifest)
        .unwrap()
        .bytes()
        .to_vec();
    let mut revision = Revision::default();
    let request = command(
        revision,
        CreateMapRequest::new("city", "城市", 640, 480),
        expected_manifest(&project),
    );

    let error = apply(&mut project, &mut revision, request).unwrap_err();

    assert!(matches!(error, EditError::InvalidSchema { .. }));
    assert_eq!(revision, Revision::default());
    assert_eq!(
        project.authoring_document(&manifest).unwrap().bytes(),
        before
    );
    assert!(project
        .authoring_document(&map_path(&root, "city"))
        .is_err());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn existing_manifest_unknown_values_survive_map_creation() {
    let (mut project, root) = old_project("unknown-fields");
    let manifest = manifest_path(&root);
    project
        .create_authoring_document(
            &manifest,
            br#"{
              "schema_version": 1,
              "project_id": "legacy",
              "language_version": "1.9",
              "entry": "world.wl",
              "required_features": ["presentation.maps.v1"],
              "maps": {},
              "graph_views": {"future_view": ".world/graph_views/future.json"},
              "extensions": {"vendor": {"preserve": "yes"}},
              "unknown_manifest": {"nested": {"keep": 42}}
            }"#
            .to_vec(),
        )
        .unwrap();
    let mut revision = Revision::default();
    let expected = expected_manifest(&project);
    let request = command(
        revision,
        CreateMapRequest::new("city", "城市", 640, 480),
        expected,
    );

    apply(&mut project, &mut revision, request).unwrap();

    let value: Value =
        serde_json::from_slice(project.authoring_document(&manifest).unwrap().bytes()).unwrap();
    assert_eq!(value["unknown_manifest"]["nested"]["keep"], 42);
    assert_eq!(
        value["graph_views"]["future_view"],
        ".world/graph_views/future.json"
    );
    assert_eq!(value["project_id"], "legacy");
    assert_eq!(value["extensions"]["vendor"]["preserve"], "yes");
    assert_eq!(value["maps"]["city"], ".world/maps/city.json");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn unknown_required_feature_is_read_only_and_does_not_mutate() {
    let (_unused, root) = old_project("read-only");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("world.wl"),
        "world harbor as \"雾港\"\n  description \"测试\"\n",
    )
    .unwrap();
    let manifest = manifest_path(&root);
    fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    fs::write(
        &manifest,
        br#"{
              "schema_version": 1,
              "required_features": ["presentation.maps.v1", "future.required.v1"],
              "maps": {}
            }"#,
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    let before = project
        .authoring_document(&manifest)
        .unwrap()
        .bytes()
        .to_vec();
    let mut revision = Revision::default();
    let expected = expected_manifest(&project);
    let request = command(
        revision,
        CreateMapRequest::new("future", "未来", 10, 10),
        expected,
    );
    let error = apply(&mut project, &mut revision, request).unwrap_err();
    assert!(matches!(error, EditError::ReadOnlyFeature { .. }));
    assert_eq!(revision, Revision::default());
    assert_eq!(
        project.authoring_document(&manifest).unwrap().bytes(),
        before
    );
    assert!(project
        .authoring_document(&map_path(&root, "future"))
        .is_err());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn stale_manifest_baseline_fails_before_creating_either_file() {
    let (mut project, root) = old_project("stale");
    let manifest = manifest_path(&root);
    project
        .create_authoring_document(
            &manifest,
            br#"{"schema_version":1,"maps":{},"unknown":{"kept":true}}"#.to_vec(),
        )
        .unwrap();
    let before = project
        .authoring_document(&manifest)
        .unwrap()
        .bytes()
        .to_vec();
    let mut revision = Revision::default();
    let mut expected = BTreeMap::new();
    expected.insert(manifest.clone(), "stale-hash".into());
    let request = command(
        revision,
        CreateMapRequest::new("city", "城市", 640, 480),
        expected,
    );
    let error = apply(&mut project, &mut revision, request).unwrap_err();
    assert!(matches!(error, EditError::ExternalConflict { .. }));
    assert_eq!(revision, Revision::default());
    assert_eq!(
        project.authoring_document(&manifest).unwrap().bytes(),
        before
    );
    assert!(project
        .authoring_document(&map_path(&root, "city"))
        .is_err());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn successful_creation_can_be_undone_as_one_two_document_change() {
    let (mut project, root) = old_project("undo");
    let mut revision = Revision::default();
    let request = command(
        revision,
        CreateMapRequest::new("harbor", "雾港", 100, 100),
        missing_manifest_baseline(&project),
    );
    let result = apply(&mut project, &mut revision, request).unwrap();
    assert_eq!(result.undo_record.changes.len(), 2);

    let undo_revision = revision;
    undo(
        &mut project,
        &mut revision,
        undo_revision,
        &result.undo_record,
    )
    .unwrap();
    assert!(project
        .authoring_document(&manifest_path(&root))
        .unwrap()
        .is_deleted());
    assert!(project
        .authoring_document(&map_path(&root, "harbor"))
        .unwrap()
        .is_deleted());
    assert_eq!(revision.presentation_generation, 2);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn manifest_appearing_after_form_baseline_is_an_external_conflict() {
    let (mut project, root) = old_project("manifest-race");
    let mut revision = Revision::default();
    let expected = missing_manifest_baseline(&project);
    project
        .create_authoring_document(
            &manifest_path(&root),
            br#"{"schema_version":1,"maps":{}}"#.to_vec(),
        )
        .unwrap();
    let request = command(
        revision,
        CreateMapRequest::new("city", "城市", 640, 480),
        expected,
    );
    let error = apply(&mut project, &mut revision, request).unwrap_err();
    assert!(matches!(error, EditError::ExternalConflict { .. }));
    assert_eq!(revision, Revision::default());
    assert!(project
        .authoring_document(&map_path(&root, "city"))
        .is_err());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn manifest_appearing_on_refresh_after_form_baseline_is_rejected_atomically() {
    let (mut project, root) = old_project("manifest-refresh-race");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("world.wl"),
        "world harbor as \"雾港\"\n  description \"测试\"\n",
    )
    .unwrap();
    project.refresh().unwrap();
    let mut revision = Revision::default();
    let expected = missing_manifest_baseline(&project);

    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(manifest_path(&root), br#"{"schema_version":1,"maps":{}}"#).unwrap();
    project.refresh().unwrap();
    let request = command(
        revision,
        CreateMapRequest::new("city", "城市", 640, 480),
        expected,
    );
    let error = apply(&mut project, &mut revision, request).unwrap_err();
    assert!(matches!(error, EditError::ExternalConflict { .. }));
    assert_eq!(revision, Revision::default());
    assert!(project
        .authoring_document(&map_path(&root, "city"))
        .is_err());
    assert_eq!(
        project
            .authoring_document(&manifest_path(&root))
            .unwrap()
            .bytes(),
        br#"{"schema_version":1,"maps":{}}"#
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn invalid_input_and_existing_target_are_rejected_without_partial_state() {
    let (mut project, root) = old_project("validation");
    let mut revision = Revision::default();
    let request = command(
        revision,
        CreateMapRequest::new("bad/id", "坏", 100, 100),
        missing_manifest_baseline(&project),
    );
    let invalid = apply(&mut project, &mut revision, request).unwrap_err();
    assert!(matches!(invalid, EditError::InvalidSchema { .. }));
    assert!(project.authoring_document(&manifest_path(&root)).is_err());

    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(map_path(&root, "existing"), b"user json").unwrap();
    let request = command(
        revision,
        CreateMapRequest::new("existing", "已存在", 100, 100),
        missing_manifest_baseline(&project),
    );
    let conflict = apply(&mut project, &mut revision, request).unwrap_err();
    assert!(matches!(conflict, EditError::ExternalConflict { .. }));
    assert!(project.authoring_document(&manifest_path(&root)).is_err());
    assert_eq!(fs::read(map_path(&root, "existing")).unwrap(), b"user json");

    let _ = fs::remove_dir_all(root);
}

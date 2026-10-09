#![cfg(not(target_arch = "wasm32"))]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{
    ast::PropertyValue,
    authoring::{CharacterDraft, EntityDraft},
    authoring_intents::{AuthoringIntent, IntentTarget, PlacementRequest, TextSelection},
    catalog::TargetRef,
    project::Project,
};

const SOURCE: &str = "character elder as \"前辈\"\nevent start\n  你看见林😀。\n  -> END\n";
struct Workspace(PathBuf);
impl Workspace {
    fn new(source: &str, manifest: Option<&str>) -> (Self, Project) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-character-intent-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("world.wl"), source).unwrap();
        fs::write(root.join("people.wl"), "// 资料原稿\n").unwrap();
        if let Some(manifest) = manifest {
            fs::create_dir_all(root.join(".world")).unwrap();
            fs::write(root.join(".world/project.json"), manifest).unwrap();
        }
        let project = Project::open(&root).unwrap();
        (Self(root), project)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn intent(project: &Project, destination: &Path) -> AuthoringIntent {
    let text = project.document(&project.entry).unwrap();
    let offset = text.rfind("林😀").unwrap();
    AuthoringIntent {
        expected_baseline: project.content_baseline(),
        target: IntentTarget::CreateCharacter {
            path: destination.to_owned(),
            draft: CharacterDraft {
                id: "lin".into(),
                display: "林😀".into(),
                properties: vec![("occupation".into(), PropertyValue::Str("航海员".into()))],
                relations: vec![("elder".into(), "师父".into())],
            },
        },
        selection: Some(TextSelection {
            path: project.entry.clone(),
            start: offset,
            end: offset + "林😀".len(),
            expected_text: "林😀".into(),
        }),
        placement: None,
    }
}

#[test]
fn tagged_character_dto_roundtrips_properties_relations_and_existing_variants() {
    let (_work, project) = Workspace::new(SOURCE, None);
    let command = intent(&project, &project.entry);
    let value = serde_json::to_value(&command).unwrap();
    assert_eq!(value["target"]["kind"], "create_character");
    assert_eq!(value["target"]["value"]["draft"]["id"], "lin");
    assert_eq!(
        value["target"]["value"]["draft"]["relations"],
        serde_json::json!([["elder", "师父"]])
    );
    let decoded: AuthoringIntent = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), value);
    let mut minimal = value;
    minimal["target"]["value"]["draft"] = serde_json::json!({"id":"lin","display":"林😀"});
    let decoded: AuthoringIntent = serde_json::from_value(minimal).unwrap();
    match decoded.target {
        IntentTarget::CreateCharacter { draft, .. } => {
            assert!(draft.properties.is_empty());
            assert!(draft.relations.is_empty());
        }
        _ => panic!("必须保持正式人物类型"),
    }
    for target in [
        IntentTarget::Existing(TargetRef::new("character", "elder")),
        IntentTarget::CreateEntity {
            path: project.entry.clone(),
            draft: EntityDraft {
                id: "harbor".into(),
                entity_type: "place".into(),
                display: "港口".into(),
                ..Default::default()
            },
        },
    ] {
        let mut command = command.clone();
        command.target = target;
        let value = serde_json::to_value(&command).unwrap();
        let decoded: AuthoringIntent = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), value);
    }
}

#[test]
fn character_creation_links_same_or_other_source_on_19_and_restores_both_together() {
    for same_file in [true, false] {
        let (work, mut project) = Workspace::new(SOURCE, None);
        let destination = if same_file {
            project.entry.clone()
        } else {
            work.0.join("people.wl")
        };
        let before = project.clone();
        let baseline = project.content_baseline();
        let command = intent(&project, &destination);
        let preview = project.preview_authoring_intent(&command).unwrap();
        assert_eq!(preview.target, TargetRef::new("character", "lin"));
        assert_eq!(preview.changed_files.len(), if same_file { 1 } else { 2 });
        assert_eq!(preview.reference_impact.content_references.len(), 1);
        assert_eq!(project.content_baseline(), baseline);
        assert!(!project.is_dirty());
        assert_eq!(fs::read_to_string(&project.entry).unwrap(), SOURCE);
        let result = project.apply_authoring_intent(&command).unwrap();
        assert_eq!(result.new_baseline, preview.new_baseline);
        assert_eq!(project.language_version(), "1.9");
        assert!(project
            .document(&project.entry)
            .unwrap()
            .contains("你看见[[character:lin|林😀]]。"));
        let character_source = project.document(&destination).unwrap();
        assert!(character_source.contains("character lin as \"林😀\""));
        assert!(character_source.contains("property occupation = \"航海员\""));
        assert!(character_source.contains("relation elder as \"师父\""));
        assert!(!character_source.contains("entity lin"));
        let compiled = project.compile_object_search_snapshot();
        assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
        assert!(compiled.analysis.catalog.entities.is_empty());
        assert_eq!(
            compiled.analysis.catalog.text_links[0].target,
            TargetRef::new("character", "lin")
        );
        assert_eq!(fs::read_to_string(&project.entry).unwrap(), SOURCE);
        assert_eq!(
            fs::read_to_string(work.0.join("people.wl")).unwrap(),
            "// 资料原稿\n"
        );
        let after = project.clone();
        let after_baseline = project.content_baseline();
        assert!(project.apply_authoring_intent(&command).is_err());
        assert_eq!(project.content_baseline(), after_baseline);
        assert!(project.restore(before));
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(project.document(&project.entry).unwrap(), SOURCE);
        assert_eq!(
            project.document(&work.0.join("people.wl")).unwrap(),
            "// 资料原稿\n"
        );
        assert!(project.restore(after));
        project.save().unwrap();
        let reopened = Project::open(&work.0).unwrap();
        assert_eq!(reopened.language_version(), "1.9");
        assert!(reopened
            .compile_object_search_snapshot()
            .analysis
            .catalog
            .object(&TargetRef::new("character", "lin"))
            .is_some());
        assert!(!work.0.join(".world/project.json").exists());
    }
}

#[test]
fn duplicate_character_id_and_invalid_new_character_data_are_atomic() {
    let source = format!("character lin as \"已存在的林\"\n{SOURCE}");
    let (_work, mut project) = Workspace::new(&source, None);
    let command = intent(&project, &project.entry);
    let baseline = project.content_baseline();
    assert!(project.preview_authoring_intent(&command).is_err());
    assert!(project.apply_authoring_intent(&command).is_err());
    assert_eq!(project.content_baseline(), baseline);
    for (id, display) in [("new_lin", "   "), ("invalid id", "林😀"), ("", "林😀")] {
        let mut command = command.clone();
        if let IntentTarget::CreateCharacter { draft, .. } = &mut command.target {
            draft.id = id.into();
            draft.display = display.into();
        }
        assert!(project.apply_authoring_intent(&command).is_err());
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(project.document(&project.entry).unwrap(), source);
    }
}

#[test]
fn character_and_entity_sharing_id_and_display_remain_distinct_formal_targets() {
    let source = format!("entity lin kind place as \"林😀\"\n{SOURCE}");
    let manifest = r#"{"schema_version":1,"language_version":"1.10","required_features":[]}"#;
    let (_work, mut project) = Workspace::new(&source, Some(manifest));
    let command = intent(&project, &project.entry);
    project.apply_authoring_intent(&command).unwrap();
    let compiled = project.compile_object_search_snapshot();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert!(compiled
        .analysis
        .catalog
        .object(&TargetRef::new("character", "lin"))
        .is_some());
    assert!(compiled
        .analysis
        .catalog
        .object(&TargetRef::new("entity", "lin"))
        .is_some());
    assert_eq!(
        compiled.analysis.catalog.text_links[0].target,
        TargetRef::new("character", "lin")
    );
    assert_eq!(compiled.analysis.catalog.entities.len(), 1);
    assert_eq!(project.language_version(), "1.10");
}

#[test]
fn generic_source_intent_still_requires_entity_enable_but_allows_19_characters() {
    let (_work, mut project) = Workspace::new(SOURCE, None);
    let mut command = intent(&project, &project.entry);
    command.target = IntentTarget::CreateEntity {
        path: project.entry.clone(),
        draft: EntityDraft {
            id: "lin".into(),
            entity_type: "place".into(),
            display: "林😀".into(),
            ..Default::default()
        },
    };
    let baseline = project.content_baseline();
    assert!(project
        .apply_authoring_intent(&command)
        .err()
        .unwrap()
        .contains("1.10"));
    assert_eq!(project.content_baseline(), baseline);
    command = intent(&project, &project.entry);
    project.apply_authoring_intent(&command).unwrap();
    assert_eq!(project.language_version(), "1.9");
}

#[test]
fn generic_character_intent_keeps_map_placement_in_same_atomic_candidate() {
    use worldline_core::{
        map_creation::{self, CreateMapCommand, CreateMapRequest, MISSING_DOCUMENT_HASH},
        presentation_commands::Revision,
    };
    let (work, mut project) = Workspace::new(SOURCE, None);
    map_creation::apply(
        &mut project,
        &mut Revision::default(),
        CreateMapCommand {
            expected_revision: Revision::default(),
            expected_documents: [(
                work.0.join(".world/project.json"),
                MISSING_DOCUMENT_HASH.into(),
            )]
            .into(),
            request: CreateMapRequest::new("atlas", "地图", 100, 100),
        },
    )
    .unwrap();
    let mut command = intent(&project, &work.0.join("people.wl"));
    command.placement = Some(PlacementRequest {
        map_id: "atlas".into(),
        placement_id: "lin_marker".into(),
        layer_id: "missing".into(),
        geometry: worldline_core::MapGeometry::Point {
            position: [0.25, 0.75],
        },
        annotation: "正文中的人物".into(),
        role: "reference".into(),
        label_override: None,
    });
    let baseline = project.content_baseline();
    let map_path = work.0.join(".world/maps/atlas.json");
    let map_before = project
        .authoring_document(&map_path)
        .unwrap()
        .bytes()
        .to_vec();
    assert!(project.apply_authoring_intent(&command).is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(
        project.authoring_document(&map_path).unwrap().bytes(),
        map_before
    );
    command.placement.as_mut().unwrap().layer_id = "places".into();
    let preview = project.preview_authoring_intent(&command).unwrap();
    assert_eq!(preview.changed_files.len(), 3);
    assert_eq!(preview.reference_impact.map_placements.len(), 1);
    assert_eq!(preview.reference_impact.content_references.len(), 1);
    assert_eq!(project.content_baseline(), baseline);
    project.apply_authoring_intent(&command).unwrap();
    assert_eq!(
        project.map_index().maps["atlas"].placements["lin_marker"].target_ref,
        Some(TargetRef::new("character", "lin"))
    );
    assert_eq!(project.language_version(), "1.9");
}

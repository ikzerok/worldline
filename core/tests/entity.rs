use std::collections::BTreeMap;
use std::fs;

use worldline_core::catalog::TargetRef;
use worldline_core::{
    compile_source, compile_source_with_options, CompileOptions, LanguageVersion,
};

#[test]
fn anchor_target_choices_follow_the_explicit_language_version() {
    assert!(
        !worldline_core::anchors::anchor_target_kinds(CompileOptions::v1_9()).contains(&"entity")
    );
    assert!(
        worldline_core::anchors::anchor_target_kinds(CompileOptions::v1_10()).contains(&"entity")
    );
}

#[test]
fn default_compile_keeps_entity_keyword_in_legacy_19_mode() {
    let result = compile_source("world.wl", "entity lighthouse kind place as \"灯塔\"\n");
    assert!(result.has_errors());
    assert!(result.program.entities.is_empty());
    assert!(result.diagnostics.iter().any(|d| d.code == "P002"));
}

#[test]
fn explicit_110_entity_is_available_from_catalog_and_navigation() {
    let source = concat!(
        "entity lighthouse kind place as \"雾港灯塔\"\n",
        "  description \"由作者编写的地点资料。\"\n",
        "  property height = 38\n",
        "  property lit = true\n",
        "event start\n",
        "  你看见 [[entity:lighthouse|灯塔]]。\n",
    );
    let result = compile_source_with_options("world.wl", source, CompileOptions::v1_10());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let entity = result.analysis.catalog.entities.get("lighthouse").unwrap();
    assert_eq!(entity.entity_type, "place");
    assert_eq!(entity.display, "雾港灯塔");
    assert_eq!(entity.description, "由作者编写的地点资料。");
    assert_eq!(
        entity.properties["height"],
        worldline_core::ast::PropertyValue::Num(38.0)
    );
    assert_eq!(
        entity.properties["lit"],
        worldline_core::ast::PropertyValue::Bool(true)
    );
    assert_eq!(
        result
            .analysis
            .catalog
            .object(&TargetRef::new("entity", "lighthouse"))
            .unwrap()
            .display,
        "雾港灯塔"
    );
    assert_eq!(result.analysis.catalog.text_links.len(), 1);
    assert_eq!(
        result.analysis.catalog.text_links[0].target,
        TargetRef::new("entity", "lighthouse")
    );
}

#[test]
fn entity_does_not_change_program_fingerprint() {
    let old = compile_source_with_options(
        "world.wl",
        "event start\n  正文。\n",
        CompileOptions::v1_10(),
    );
    let new = compile_source_with_options(
        "world.wl",
        "entity lighthouse kind place as \"灯塔\"\nevent start\n  正文。\n",
        CompileOptions::v1_10(),
    );
    assert_eq!(old.analysis.fingerprint, new.analysis.fingerprint);
    assert_eq!(old.program.events.len(), new.program.events.len());
    assert_eq!(new.options.language_version, LanguageVersion::V1_10);
}

#[test]
fn entity_metadata_is_outside_the_fingerprint_but_legacy_metadata_is_not() {
    let baseline = compile_source_with_options(
        "world.wl",
        "event start\n  正文。\n",
        CompileOptions::v1_10(),
    );
    let changed_entity = compile_source_with_options(
        "world.wl",
        concat!(
            "entity lighthouse kind organization as \"守灯会\"\n",
            "  description \"新的说明\"\n",
            "  property height = 42\n",
            "event start\n  正文。\n",
        ),
        CompileOptions::v1_10(),
    );
    assert_eq!(
        baseline.analysis.fingerprint,
        changed_entity.analysis.fingerprint
    );

    let character = compile_source(
        "world.wl",
        "character lin as \"林舟\"\nevent start with lin\n  正文。\n",
    );
    let changed_character = compile_source(
        "world.wl",
        "character lin as \"阿舟\"\nevent start with lin\n  正文。\n",
    );
    assert_ne!(
        character.analysis.fingerprint,
        changed_character.analysis.fingerprint
    );

    let world = compile_source(
        "world.wl",
        "world harbor as \"雾港\"\n  description \"旧说明\"\n",
    );
    let changed_world = compile_source(
        "world.wl",
        "world harbor as \"雾港\"\n  description \"新说明\"\n",
    );
    assert_ne!(
        world.analysis.fingerprint,
        changed_world.analysis.fingerprint
    );
}

#[test]
fn legacy_19_entity_text_and_target_references_remain_legacy() {
    let result = compile_source(
        "world.wl",
        "entity lighthouse kind place\nevent start\n  [[entity:lighthouse|灯塔]]\n",
    );
    assert!(result.has_errors());
    assert!(result.program.entities.is_empty());
    assert!(result.analysis.catalog.entities.is_empty());
    assert!(result.analysis.catalog.text_links.is_empty());
    assert!(result.diagnostics.iter().any(|d| d.code == "P002"));
}

#[test]
fn entity_alias_mark_anchor_and_map_references_block_deletion() {
    let root = std::env::temp_dir().join(format!("worldline-entity-impact-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(
        root.join("world.wl"),
        concat!(
            "entity lighthouse kind place as \"雾港灯塔\"\n",
            "tag places as \"地点\"\n",
            "alias entity lighthouse as \"灯塔别名\"\n",
            "mark entity lighthouse with places\n",
            "anchor_def landmarks as \"地标\"\n",
            "anchor_link landmarks entity lighthouse\n",
            "event start\n",
            "  你看见 [[entity:lighthouse|灯塔]]。\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","maps":{"overview":".world/maps/overview.json"},"required_features":[]}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/overview.json"),
        r#"{"schema_version":1,"id":"overview","title":"Overview","canvas":{"width":100,"height":100,"unit":"normalized"},"layer_order":["places"],"layers":{"places":{"title":"Places","visible_default":true,"locked":false}},"placements":{"lighthouse_marker":{"layer_id":"places","annotation":"Lighthouse","role":"reference","target_ref":{"kind":"entity","id":"lighthouse"},"geometry":{"kind":"point","position":[0.2,0.3]}}}}"#,
    )
    .unwrap();

    let mut project = worldline_core::project::Project::open(&root).unwrap();
    let result = project.compile();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let target = TargetRef::new("entity", "lighthouse");
    let original_baseline = project.content_baseline();
    let map_index = project.map_index();
    assert_eq!(
        map_index.placements_for(&target),
        vec![worldline_core::presentation::MapPlacementRef {
            map_id: "overview".into(),
            placement_id: "lighthouse_marker".into(),
        }],
        "maps={:?} diagnostics={:?}",
        map_index.maps.keys().collect::<Vec<_>>(),
        map_index.diagnostics
    );
    let impact = project.deletion_impact(&target);
    assert!(impact.complete, "{:?}", impact.diagnostics);
    assert!(!impact.can_delete());
    assert!(!impact.map_placements.is_empty());
    assert!(impact
        .content_references
        .iter()
        .any(|reference| reference.kind == "正文链接"));
    assert!(impact
        .content_references
        .iter()
        .any(|reference| reference.kind == "别名引用"));
    assert!(impact
        .content_references
        .iter()
        .any(|reference| reference.kind == "标签标记"));
    assert!(impact
        .content_references
        .iter()
        .any(|reference| reference.kind == "锚点关联"));

    let before = project.map_index().maps["overview"].placements["lighthouse_marker"].clone();
    let draft = worldline_core::authoring::EntityDraft {
        id: "lighthouse".into(),
        entity_type: "organization".into(),
        display: "守灯会".into(),
        description: "更新后的资料".into(),
        properties: Vec::new(),
    };
    project
        .write_entity(&root.join("world.wl"), Some("lighthouse"), &draft)
        .unwrap();
    let after = project.map_index().maps["overview"].placements["lighthouse_marker"].clone();
    assert_eq!(before.id, after.id);
    assert_eq!(before.geometry, after.geometry);
    assert_eq!(
        project.compile().analysis.catalog.entities["lighthouse"].display,
        "守灯会"
    );
    assert_ne!(project.content_baseline(), original_baseline);
    assert!(!project.deletion_impact(&target).map_placements.is_empty());

    let map_path = root.join(".world/maps/overview.json");
    let map_bytes = project
        .authoring_document(&map_path)
        .unwrap()
        .bytes()
        .to_vec();
    let mut map: serde_json::Value = serde_json::from_slice(&map_bytes).unwrap();
    map["title"] = serde_json::json!("Updated overview");
    project
        .set_authoring_document(&map_path, serde_json::to_vec(&map).unwrap())
        .unwrap();
    assert_ne!(project.content_baseline(), original_baseline);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_entities_have_a_related_diagnostic() {
    let result = compile_source_with_options(
        "world.wl",
        "entity lighthouse kind place\nentity lighthouse kind item\n",
        CompileOptions::v1_10(),
    );
    let duplicate = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "A104")
        .unwrap();
    assert!(duplicate.message.contains("实体"));
    assert_eq!(duplicate.related.len(), 1);
}

#[test]
fn compile_sources_with_options_preserves_memory_overrides() {
    let entry = std::path::PathBuf::from("world.wl");
    let sources = BTreeMap::from([(entry.clone(), "entity x kind place\n".to_string())]);
    let result = worldline_core::compile_sources_with_options(
        &entry,
        &sources,
        CompileOptions::new(LanguageVersion::V1_10),
    );
    assert_eq!(result.analysis.catalog.entities["x"].entity_type, "place");
}

#[test]
fn project_manifest_selects_110_without_upgrading_legacy_projects() {
    let root =
        std::env::temp_dir().join(format!("worldline-entity-project-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(
        root.join("world.wl"),
        "entity lighthouse kind place as \"灯塔\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":[]}"#,
    )
    .unwrap();
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    assert_eq!(project.language_version(), "1.10");
    assert_eq!(project.language_version_kind(), LanguageVersion::V1_10);
    assert_eq!(project.compile().analysis.catalog.entities.len(), 1);

    std::fs::remove_file(root.join(".world/project.json")).unwrap();
    std::fs::remove_dir(root.join(".world")).unwrap();
    std::fs::write(root.join("world.wl"), "entity lighthouse kind place\n").unwrap();
    let mut legacy = worldline_core::project::Project::open(&root).unwrap();
    assert_eq!(legacy.language_version(), "1.9");
    assert!(legacy.compile().has_errors());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deleting_manifest_reverts_project_to_legacy_language_and_restore_reinstates_it() {
    let root = std::env::temp_dir().join(format!(
        "worldline-entity-manifest-delete-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(
        root.join("world.wl"),
        "entity lighthouse kind place as \"灯塔\"\n",
    )
    .unwrap();
    let manifest = root.join(".world/project.json");
    fs::write(
        &manifest,
        r#"{"schema_version":1,"language_version":"1.10","required_features":[]}"#,
    )
    .unwrap();
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    let before = project.clone();
    assert_eq!(project.language_version(), "1.10");
    assert_eq!(project.compile().analysis.catalog.entities.len(), 1);

    project.delete_authoring_document(&manifest).unwrap();
    assert_eq!(project.language_version(), "1.9");
    assert!(project.compile().has_errors());
    assert!(project
        .write_entity(
            &root.join("world.wl"),
            Some("lighthouse"),
            &worldline_core::authoring::EntityDraft {
                id: "lighthouse".into(),
                entity_type: "place".into(),
                display: "灯塔".into(),
                description: String::new(),
                properties: Vec::new(),
            },
        )
        .is_err());

    assert!(project.restore(before));
    assert_eq!(project.language_version(), "1.10");
    assert_eq!(project.compile().analysis.catalog.entities.len(), 1);
    project.delete_authoring_document(&manifest).unwrap();
    project.refresh().unwrap();
    assert_eq!(project.language_version(), "1.9");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unknown_language_manifest_is_read_only_for_source_edits() {
    let root = std::env::temp_dir().join(format!(
        "worldline-entity-unknown-language-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    let source = "entity lighthouse kind place as \"灯塔\"\n";
    fs::write(root.join("world.wl"), source).unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"2.0","required_features":[]}"#,
    )
    .unwrap();
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    assert_eq!(project.language_version(), "1.9");
    assert!(project
        .authoring_diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "WS003"));
    assert!(project
        .authoring_document(&root.join(".world/project.json"))
        .unwrap()
        .is_read_only());
    let before = project.sources();
    assert!(project
        .set_text(&root.join("world.wl"), "changed\n".into())
        .is_err());
    assert!(project
        .edit(|candidate| candidate.set_text(&root.join("world.wl"), "changed\n".into()))
        .is_err());
    assert_eq!(project.sources(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unknown_required_feature_keeps_entity_project_read_only() {
    let root = std::env::temp_dir().join(format!(
        "worldline-entity-unknown-feature-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(
        root.join("world.wl"),
        "entity lighthouse kind place as \"灯塔\"\n",
    )
    .unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","required_features":["future.entities.v2"]}"#,
    )
    .unwrap();
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    assert_eq!(project.language_version(), "1.10");
    assert!(project
        .authoring_diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "WS003"));
    assert!(project
        .set_text(&root.join("world.wl"), "changed\n".into())
        .is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_writes_updates_and_removes_entities_through_source_buffers() {
    let root = std::env::temp_dir().join(format!("worldline-entity-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(root.join("world.wl"), "").unwrap();
    std::fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1"]}"#,
    )
    .unwrap();
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    let draft = worldline_core::authoring::EntityDraft {
        id: "lighthouse".into(),
        entity_type: "place".into(),
        display: "雾港灯塔".into(),
        description: "临海的灯塔".into(),
        properties: vec![(
            "height".into(),
            worldline_core::ast::PropertyValue::Num(38.0),
        )],
    };
    project
        .write_entity(&root.join("world.wl"), None, &draft)
        .unwrap();
    let first = project.compile();
    assert_eq!(
        first.analysis.catalog.entities["lighthouse"].display,
        "雾港灯塔"
    );

    let mut changed = draft.clone();
    changed.entity_type = "organization".into();
    changed.display = "守灯会".into();
    project
        .write_entity(&root.join("world.wl"), Some("lighthouse"), &changed)
        .unwrap();
    let second = project.compile();
    let entity = &second.analysis.catalog.entities["lighthouse"];
    assert_eq!(entity.entity_type, "organization");
    assert_eq!(entity.display, "守灯会");
    assert!(project.is_dirty());

    project.remove_entity("lighthouse").unwrap();
    assert!(project.compile().analysis.catalog.entities.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn entity_only_project_can_compile_edit_and_save_without_an_entry_event() {
    let root = std::env::temp_dir().join(format!("worldline-entity-only-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(root.join("world.wl"), "").unwrap();
    std::fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":[]}"#,
    )
    .unwrap();

    let mut project = worldline_core::project::Project::open(&root).unwrap();
    project
        .write_entity(
            &root.join("world.wl"),
            None,
            &worldline_core::authoring::EntityDraft {
                id: "lighthouse".into(),
                entity_type: "place".into(),
                display: "灯塔".into(),
                description: "仅有作者资料".into(),
                properties: Vec::new(),
            },
        )
        .unwrap();
    let result = project.compile();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(result.program.events.is_empty());
    assert_eq!(result.analysis.catalog.entities.len(), 1);
    project.save().unwrap();
    assert!(std::fs::read_to_string(root.join("world.wl"))
        .unwrap()
        .contains("entity lighthouse kind place"));
    std::fs::remove_dir_all(root).unwrap();
}

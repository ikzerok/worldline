use std::{fs, path::PathBuf};
use worldline_core::{catalog::TargetRef, compile_path, project::Project};
use worldline_runtime::Story;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "wl-workspace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn recursive_index_refresh_conflict_and_complete_export() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    fs::create_dir_all(root.join("chapters/deep")).unwrap();
    fs::create_dir_all(root.join(".agent/skills")).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    fs::write(
        root.join("chapters/deep/extra.wl"),
        "character extra as \"目录人物\"\n",
    )
    .unwrap();
    fs::write(root.join("README.md"), "作者的说明").unwrap();
    fs::write(root.join(".agent/skills/note.md"), "关联创作资料").unwrap();
    fs::write(root.join("unused.bin"), [0, 255, 128]).unwrap();
    let mut project = Project::open(&root).unwrap();
    // 文档索引与刷新结果使用工程规范路径。
    let root = project.root.clone();
    assert_eq!(project.documents.len(), 2);
    assert!(project
        .compile()
        .analysis
        .symbols
        .characters
        .contains_key("extra"));
    assert_eq!(project.search("目录人物").len(), 1);
    assert_eq!(
        compile_path(&root).unwrap().analysis.fingerprint,
        project.compile().analysis.fingerprint
    );
    let extra = root.join("chapters/deep/extra.wl");
    fs::write(&extra, "character changed\n").unwrap();
    project.refresh().unwrap();
    assert!(project
        .compile()
        .analysis
        .symbols
        .characters
        .contains_key("changed"));
    project
        .set_text(&extra, "character local\n".into())
        .unwrap();
    fs::write(&extra, "character remote\n").unwrap();
    assert_eq!(project.refresh().unwrap(), vec![extra.clone()]);
    assert!(project.document(&extra).unwrap().contains("local"));
    assert!(project.save().is_err());
    let export = temp.0.join("export");
    project.export(&export).unwrap();
    assert_eq!(fs::read(export.join("unused.bin")).unwrap(), [0, 255, 128]);
    assert_eq!(
        fs::read_to_string(export.join("README.md")).unwrap(),
        "作者的说明"
    );
    assert!(export.join(".agent/skills/note.md").is_file());
    assert!(fs::read_to_string(export.join("chapters/deep/extra.wl"))
        .unwrap()
        .contains("local"));
    assert!(project.export(&root.join("nested-export")).is_err());
    let new_file = root.join("added.wl");
    fs::write(&new_file, "tag new_tag\n").unwrap();
    project.refresh().unwrap();
    assert!(project.documents.contains_key(&new_file));
    fs::remove_file(&new_file).unwrap();
    project.refresh().unwrap();
    assert!(!project.documents.contains_key(&new_file));
}

#[test]
fn external_includes_assets_and_symlink_ancestors_are_rejected() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    fs::create_dir(&root).unwrap();
    fs::write(temp.0.join("outside.wl"), "character hidden\n").unwrap();
    fs::write(
        root.join("world.wl"),
        "include \"../outside.wl\"\nevent start\n  -> END\n",
    )
    .unwrap();
    let result = compile_path(&root).unwrap();
    assert!(result.diagnostics.iter().any(|d| d.code == "A109"));
    assert!(!result.analysis.symbols.characters.contains_key("hidden"));
    fs::write(
        root.join("world.wl"),
        "asset leak file \"../outside.wl\"\nevent start\n  -> END\n",
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    assert!(project
        .compile()
        .diagnostics
        .iter()
        .any(|d| d.code == "A109"));
    assert!(project
        .add_asset_reference(
            &worldline_core::catalog::TargetRef::new("event", "start"),
            &temp.0.join("outside.wl")
        )
        .is_err());
    assert!(project
        .add_file(std::path::Path::new("../escape.wl"))
        .is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&temp.0, root.join("link")).unwrap();
        assert!(project.refresh().is_err());
        assert!(project
            .add_file(std::path::Path::new("link/new.wl"))
            .is_err());
    }
}

#[test]
fn registered_authoring_json_keeps_raw_bytes_through_lifecycle() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{
  "schema_version": 1,
  "project_id": "raw_bytes",
  "language_version": "1.9",
  "entry": "world.wl",
  "required_features": ["presentation.maps.v1"],
  "maps": {"raw_map": ".world/maps/raw.json"},
  "graph_views": {}
}"#,
    )
    .unwrap();
    let original = b"{\"schema_version\":1,\"future\":\xff}".to_vec();
    let map = root.join(".world/maps/raw.json");
    fs::write(&map, &original).unwrap();
    fs::write(root.join("notes.json"), b"ordinary json").unwrap();

    let mut project = Project::open(&root).unwrap();
    assert!(!project.sources().contains_key(&map));
    assert_eq!(project.authoring_document(&map).unwrap().bytes(), original);
    assert!(project
        .authoring_document(&root.join("notes.json"))
        .is_err());

    let before_edit = project.clone();
    let edited = b"{\"schema_version\":1,\"future\":\xfe}".to_vec();
    project
        .set_authoring_document(&map, edited.clone())
        .unwrap();
    assert!(project.is_dirty());
    assert_eq!(
        project.export_files().unwrap()[&PathBuf::from(".world/maps/raw.json")],
        edited
    );
    project.mark_saved();
    assert!(!project.is_dirty());
    project.restore(before_edit);
    assert_eq!(project.authoring_document(&map).unwrap().bytes(), original);
    assert!(project.is_dirty());
}

#[test]
fn external_manifest_registration_changes_apply_in_one_refresh() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    let manifest = root.join(".world/project.json");
    fs::write(&manifest, br#"{"schema_version":1,"maps":{}}"#).unwrap();
    let mut project = Project::open(&root).unwrap();
    let map = root.join(".world/maps/new.json");
    fs::write(&map, b"{}").unwrap();
    fs::write(
        &manifest,
        br#"{"schema_version":1,"maps":{"new":".world/maps/new.json"}}"#,
    )
    .unwrap();
    assert!(project.refresh().unwrap().is_empty());
    assert_eq!(project.authoring_document(&map).unwrap().bytes(), b"{}");
    fs::write(&manifest, br#"{"schema_version":1,"maps":{}}"#).unwrap();
    project.refresh().unwrap();
    assert!(project.authoring_document(&map).is_err());
    assert_eq!(
        project.export_files().unwrap()[&PathBuf::from(".world/maps/new.json")],
        b"{}"
    );
}

#[test]
fn newly_registered_documents_can_be_saved_deleted_and_undone() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    let mut project = Project::new(&root);
    project.save().unwrap();
    let before = project.clone();
    let manifest = root.join(".world/project.json");
    let map = root.join(".world/maps/new.json");
    project
        .create_authoring_document(
            &manifest,
            br#"{"schema_version":1,"maps":{"new":".world/maps/new.json"}}"#.to_vec(),
        )
        .unwrap();
    project
        .create_authoring_document(&map, b"{\"opaque\":17}".to_vec())
        .unwrap();
    project.save().unwrap();
    assert!(!project.is_dirty());
    let saved = project.clone();
    project.restore(before);
    assert!(project.is_dirty());
    assert!(project.authoring_document(&map).unwrap().is_deleted());
    assert!(!project
        .export_files()
        .unwrap()
        .contains_key(&PathBuf::from(".world/maps/new.json")));
    project.save().unwrap();
    assert!(!map.exists());
    assert!(!manifest.exists());
    project.restore(saved);
    assert!(project.is_dirty());
    project.save().unwrap();
    assert_eq!(fs::read(&map).unwrap(), b"{\"opaque\":17}");
    project.delete_document(&map).unwrap();
    project.save().unwrap();
    assert!(!map.exists());
    assert!(!project.is_dirty());
}

#[test]
fn unsupported_manifest_capabilities_protect_registered_documents() {
    for capability in [r#""future""#, "[17]", r#"["future.v9"]"#] {
        let temp = Temp::new();
        let root = temp.0.join("world");
        fs::create_dir_all(root.join(".world/maps")).unwrap();
        fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
        let manifest = root.join(".world/project.json");
        let map = root.join(".world/maps/map.json");
        fs::write(&map, b"{}").unwrap();
        let bytes = format!(
            r#"{{"schema_version":1,"required_features":{capability},"maps":{{"map":".world/maps/map.json"}}}}"#
        );
        fs::write(&manifest, bytes.as_bytes()).unwrap();
        let mut project = Project::open(&root).unwrap();
        assert!(!project.authoring_diagnostics().is_empty());
        assert!(project
            .authoring_document(&manifest)
            .unwrap()
            .is_read_only());
        assert!(project
            .set_authoring_document(&map, b"changed".to_vec())
            .is_err());
        assert!(project.delete_document(&map).is_err());
        assert!(!project.is_dirty());
        project.save().unwrap();
        assert_eq!(fs::read(&manifest).unwrap(), bytes.as_bytes());
    }
}

#[test]
fn authoring_conflicts_preserve_local_bytes_and_block_all_writes() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    let mut project = Project::new(&root);
    project.save().unwrap();
    let manifest = root.join(".world/project.json");
    let map = root.join(".world/maps/map.json");
    project
        .create_authoring_document(
            &manifest,
            br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#.to_vec(),
        )
        .unwrap();
    project
        .create_authoring_document(&map, b"old".to_vec())
        .unwrap();
    project.save().unwrap();
    let original_source = fs::read(root.join("world.wl")).unwrap();
    let entry = project.entry.clone();
    let changed_source = format!("{}\n// local\n", project.document(&entry).unwrap());
    project.set_text(&entry, changed_source).unwrap();
    project
        .set_authoring_document(&map, b"local".to_vec())
        .unwrap();
    fs::write(&map, b"external").unwrap();
    let conflicts = project.refresh().unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(
        conflicts[0].canonicalize().unwrap(),
        map.canonicalize().unwrap()
    );
    assert_eq!(project.authoring_document(&map).unwrap().bytes(), b"local");
    assert!(project.save().is_err());
    assert_eq!(fs::read(&map).unwrap(), b"external");
    assert_eq!(fs::read(&entry).unwrap(), original_source);
    project.save_as(&temp.0.join("copy")).unwrap();
    assert_eq!(
        fs::read(temp.0.join("copy/.world/maps/map.json")).unwrap(),
        b"local"
    );
    assert!(!project.is_dirty());
}

#[test]
fn legacy_workspace_roundtrip_preserves_all_bytes_without_manifest() {
    let temp = Temp::new();
    let root = temp.0.join("legacy");
    fs::create_dir_all(&root).unwrap();
    let source = b"// preserved\r\nevent start\r\n  -> END\r\n";
    let ordinary = b"\xff{not system json}\x00";
    fs::write(root.join("world.wl"), source).unwrap();
    fs::write(root.join("ordinary.json"), ordinary).unwrap();
    let mut project = Project::open(&root).unwrap();
    assert!(project.authoring_documents.is_empty());
    project.save().unwrap();
    let files = project.export_files().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[&PathBuf::from("world.wl")], source);
    assert_eq!(files[&PathBuf::from("ordinary.json")], ordinary);
    project.save_as(&temp.0.join("copy")).unwrap();
    assert!(!temp.0.join("copy/.world").exists());
    assert_eq!(fs::read(temp.0.join("copy/world.wl")).unwrap(), source);
    assert_eq!(
        fs::read(temp.0.join("copy/ordinary.json")).unwrap(),
        ordinary
    );
}

#[test]
fn source_tombstone_is_not_reloaded_by_include_before_save() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("world.wl"),
        "include \"chapter.wl\"\nevent start\n  -> END\n",
    )
    .unwrap();
    let chapter = root.join("chapter.wl");
    fs::write(&chapter, "event removed\n  -> END\n").unwrap();
    let mut project = Project::open(&root).unwrap();
    let before = project.clone();
    project.delete_document(&chapter).unwrap();
    let result = project.compile();
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "A105"));
    assert!(!result.sources.contains_key(&chapter));
    assert!(!project.sources().contains_key(&chapter));
    assert!(project.search("removed").is_empty());
    project.restore(before);
    assert!(!project.compile().has_errors());
    assert!(!project.is_dirty());
}

#[test]
fn project_reads_maps_and_indexes_object_placements_without_changing_program() {
    let temp = Temp::new();
    let root = temp.0.join("maps");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(
        root.join("world.wl"),
        "tag lighthouse as \"灯塔\"\nasset base image \"base.png\" as \"底图\"\n",
    )
    .unwrap();
    fs::write(root.join("base.png"), b"placeholder").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"required_features":["presentation.maps.v1"],"maps":{"harbor":".world/maps/harbor.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/harbor.json"),
        r#"{
          "schema_version":1,
          "id":"harbor",
          "title":"雾港",
          "raster_layers":[{"id":"base","asset":{"kind":"asset","id":"base"},"rect":[0,0,1,1]}],
          "canvas":{"width":800,"height":600,"unit":"normalized"},
          "layer_order":["places"],
          "layers":{"places":{"title":"地点","visible_default":true,"locked":false}},
          "placements":{"lighthouse":{"layer_id":"places","target_ref":{"kind":"tag","id":"lighthouse"},"geometry":{"kind":"point","position":[0.5,0.25]},"annotation":"入口","role":"地点"}},
          "future_optional":{"kept":true}
        }"#,
    )
    .unwrap();

    let mut project = Project::open(&root).unwrap();
    let fingerprint = project.compile().analysis.fingerprint;
    let index = project.map_index();
    assert!(
        index.diagnostics.is_empty(),
        "地图应合法: {:?}",
        index.diagnostics
    );
    assert_eq!(index.maps["harbor"].canvas.width, 800);
    assert_eq!(
        index.maps["harbor"].extra["future_optional"]["kept"],
        serde_json::Value::Bool(true)
    );
    assert_eq!(
        index.placements_for(&TargetRef::new("tag", "lighthouse")),
        vec![worldline_core::presentation::MapPlacementRef {
            map_id: "harbor".into(),
            placement_id: "lighthouse".into(),
        }]
    );
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
}

#[test]
fn zero_area_raster_rectangles_are_rejected() {
    let temp = Temp::new();
    let root = temp.0.join("raster-rect");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "// content only\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    for rect in [[0, 0, 0, 1], [0, 0, 1, 0]] {
        let map = serde_json::json!({
            "schema_version": 1, "id": "map", "title": "Map",
            "canvas": {"width": 10, "height": 10, "unit": "normalized"},
            "layer_order": [], "layers": {}, "placements": {},
            "raster_layers": [{"id": "base", "asset": {"kind": "asset", "id": "image"}, "rect": rect}]
        });
        let bytes = serde_json::to_vec(&map).unwrap();
        fs::write(root.join(".world/maps/map.json"), &bytes).unwrap();
        let project = Project::open(&root).unwrap();
        let index = project.map_index();
        assert!(
            index.diagnostics.iter().any(|d| d.code == "MAP008"),
            "{rect:?}"
        );
        assert!(index.maps.is_empty());
        assert_eq!(
            project
                .authoring_document(&project.root.join(".world/maps/map.json"))
                .unwrap()
                .bytes(),
            bytes
        );
    }
}

#[test]
fn serialized_map_source_preserves_unknown_fields_and_required_features() {
    let temp = Temp::new();
    let root = temp.0.join("map-source");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(
        root.join("world.wl"),
        "tag harbor as \"雾港\"\nasset base image \"base.png\" as \"底图\"\n",
    )
    .unwrap();
    fs::write(root.join("base.png"), b"placeholder").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    let map = serde_json::json!({
        "schema_version": 1,
        "required_features": ["presentation.maps.v1"],
        "source": {"author_extension": true},
        "id": "map",
        "title": "Map",
        "raster_layers": [{
            "id": "base",
            "asset": {"kind": "asset", "id": "base", "future_asset": {"kept": true}},
            "rect": [0, 0, 1, 1]
        }],
        "canvas": {"width": 10, "height": 10, "unit": "normalized"},
        "layer_order": ["notes"],
        "layers": {"notes": {"title": "Notes", "visible_default": true, "locked": false}},
        "placements": {"note": {
            "layer_id": "notes",
            "target_ref": {"kind": "tag", "id": "harbor", "future_target": {"kept": true}},
            "geometry": {"kind": "point", "position": [0.5, 0.5], "future_geometry": {"kept": true}},
            "annotation": "source",
            "role": "note"
        }}
    });
    fs::write(
        root.join(".world/maps/map.json"),
        serde_json::to_vec(&map).unwrap(),
    )
    .unwrap();

    let project = Project::open(&root).unwrap();
    let index = project.map_index();
    assert!(
        index.diagnostics.is_empty(),
        "地图应合法: {:?}",
        index.diagnostics
    );
    let serialized = serde_json::to_value(&index).unwrap();
    assert_eq!(serialized["maps"]["map"]["source"], map);
}

#[test]
fn pure_content_project_can_save_and_export_but_play_reports_missing_entry() {
    let temp = Temp::new();
    let root = temp.0.join("content-only");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("world.wl"), "tag lighthouse as \"灯塔\"\n").unwrap();

    let mut project = Project::open(&root).unwrap();
    let result = project.compile();
    assert!(result.program.events.is_empty());
    assert!(!result.has_errors());
    project.save().unwrap();
    let package = project.export_files().unwrap();
    assert_eq!(
        package[&PathBuf::from("world.wl")],
        "tag lighthouse as \"灯塔\"\n".as_bytes()
    );

    let run_error = match Story::new(&result.program, &result.analysis) {
        Ok(_) => panic!("纯内容工程不应启动试玩"),
        Err(error) => error,
    };
    assert!(run_error.to_string().contains("没有可运行入口"));
}

#[test]
fn malformed_map_isolated_from_valid_map_and_keeps_original_bytes() {
    let temp = Temp::new();
    let root = temp.0.join("map-isolation");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "tag harbor as \"雾港\"\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"maps":{"good":".world/maps/good.json","bad":".world/maps/bad.json"}}"#,
    )
    .unwrap();
    let good = r#"{
      "schema_version":1,"id":"good","title":"合法地图",
      "canvas":{"width":10,"height":10,"unit":"normalized"},
      "layer_order":[],"layers":{},"placements":{}
    }"#;
    let bad = br#"{"schema_version":1,"id":"bad","title":"bad","schema_version":1}"#;
    fs::write(root.join(".world/maps/good.json"), good).unwrap();
    fs::write(root.join(".world/maps/bad.json"), bad).unwrap();

    let mut project = Project::open(&root).unwrap();
    let index = project.map_index();
    assert!(index.maps.contains_key("good"));
    assert!(!index.maps.contains_key("bad"));
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAP001"));
    assert_eq!(
        project
            .authoring_document(&root.join(".world/maps/bad.json"))
            .unwrap()
            .bytes(),
        bad
    );
    assert!(project
        .compile()
        .analysis
        .catalog
        .tags
        .contains_key("harbor"));
}

#[test]
fn map_query_reports_manifest_errors_without_polluting_language_diagnostics() {
    let temp = Temp::new();
    let root = temp.0.join("invalid-registry");
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), "tag harbor\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":[]}"#,
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    assert!(project
        .map_index()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "WS004"));
    assert!(!project.compile().has_errors());
}

#[test]
fn raster_layers_requires_an_array_instead_of_silent_legacy_coercion() {
    let temp = Temp::new();
    let root = temp.0.join("raster-format");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "tag harbor\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/map.json"),
        br#"{
        "schema_version":1,"id":"map","title":"Map",
        "canvas":{"width":10,"height":10,"unit":"normalized"},
        "layers":{},"layer_order":[],"placements":{},
        "raster_layers":{"id":"base","asset":{"kind":"asset","id":"image"},"rect":[0,0,1,1]}
    }"#,
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    let index = project.map_index();
    assert!(!index.maps.contains_key("map"));
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAP008"));
}

#[test]
fn non_image_asset_is_isolated_without_hiding_the_map() {
    let temp = Temp::new();
    let root = temp.0.join("raster-type");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(
        root.join("world.wl"),
        "asset sound audio \"sound.wav\" as \"声音\"\n",
    )
    .unwrap();
    fs::write(root.join("sound.wav"), b"placeholder").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/map.json"),
        br#"{
        "schema_version":1,"id":"map","title":"Map",
        "canvas":{"width":10,"height":10,"unit":"normalized"},
        "layers":{},"layer_order":[],"placements":{},
        "raster_layers":[{"id":"base","asset":{"kind":"asset","id":"sound"},"rect":[0,0,1,1]}]
    }"#,
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    assert!(!project.compile().has_errors());
    let index = project.map_index();
    assert!(index.maps["map"].raster_layers[0].asset_info.is_none());
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAP010"));
}

#[test]
fn rejected_empty_map_titles_and_roles_always_have_diagnostics() {
    let temp = Temp::new();
    let root = temp.0.join("map-errors");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "tag harbor\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    let valid = serde_json::json!({"schema_version":1,"id":"map","title":"Map",
        "canvas":{"width":10,"height":10,"unit":"normalized"},
        "layers":{"notes":{"title":"Notes","visible_default":true,"locked":false}},
        "layer_order":["notes"],"placements":{"note":{"layer_id":"notes","target_ref":null,
            "geometry":{"kind":"point","position":[0.5,0.5]},"annotation":"","role":"note"}}});
    for pointer in ["/title", "/placements/note/role"] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).unwrap() = serde_json::json!("");
        fs::write(
            root.join(".world/maps/map.json"),
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap();
        let project = Project::open(&root).unwrap();
        let index = project.map_index();
        assert!(index.maps.is_empty());
        assert!(
            index
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == worldline_core::Severity::Error),
            "缺少{pointer}错误原因"
        );
    }
}

#[test]
fn map_geometry_rejects_out_of_range_and_self_intersecting_shapes() {
    let temp = Temp::new();
    let root = temp.0.join("map-geometry");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "tag harbor\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"maps":{"valid":".world/maps/valid.json","invalid":".world/maps/invalid.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/valid.json"),
        r#"{
          "schema_version":1,"id":"valid","title":"valid",
          "canvas":{"width":10,"height":10,"unit":"normalized"},
          "layer_order":["places"],
          "layers":{"places":{"title":"places","visible_default":true,"locked":false}},
          "placements":{"concave":{"layer_id":"places","target_ref":null,"geometry":{"kind":"polygon","points":[[0.1,0.1],[0.9,0.1],[0.9,0.4],[0.5,0.4],[0.5,0.9],[0.1,0.9]]},"annotation":"shape","role":"note"}}
        }"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/invalid.json"),
        r#"{
          "schema_version":1,"id":"invalid","title":"invalid",
          "canvas":{"width":10,"height":10,"unit":"normalized"},
          "layer_order":["places"],
          "layers":{"places":{"title":"places","visible_default":true,"locked":false}},
          "placements":{
            "cross":{"layer_id":"places","target_ref":null,"geometry":{"kind":"polygon","points":[[0.1,0.1],[0.9,0.9],[0.1,0.9],[0.9,0.1]]},"annotation":"shape","role":"note"},
            "out_of_range":{"layer_id":"places","target_ref":null,"geometry":{"kind":"point","position":[1.1,0.5]},"annotation":"point","role":"note"}
          }
        }"#,
    )
    .unwrap();

    let project = Project::open(&root).unwrap();
    let index = project.map_index();
    assert!(index.maps.contains_key("valid"));
    assert!(!index.maps.contains_key("invalid"));
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAP006"));
}

#[test]
fn map_query_reports_unresolved_scope_refs_without_hiding_the_map() {
    let temp = Temp::new();
    let root = temp.0.join("map-scope-ref");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "tag harbor\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/map.json"),
        br#"{
          "schema_version":1,
          "id":"map",
          "title":"Map",
          "canvas":{"width":10,"height":10,"unit":"normalized"},
          "layer_order":["notes"],
          "layers":{"notes":{"title":"Notes","visible_default":true,"locked":false}},
          "placements":{"note":{
            "layer_id":"notes",
            "target_ref":null,
            "geometry":{"kind":"point","position":[0.5,0.5]},
            "annotation":"scope",
            "role":"note",
            "scope_refs":[{"kind":"period","id":"missing_period"}]
          }}
        }"#,
    )
    .unwrap();

    let project = Project::open(&root).unwrap();
    let index = project.map_index();
    assert!(index.maps.contains_key("map"));
    let unresolved = index
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "MAP011")
        .collect::<Vec<_>>();
    assert_eq!(unresolved.len(), 1);
    assert!(unresolved[0].message.contains("scope_refs"));
}

#[test]
fn refresh_loads_external_recreation_after_a_saved_deletion() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    let mut project = Project::new(&root);
    project.save().unwrap();
    let map = root.join(".world/maps/map.json");
    project
        .create_authoring_document(
            &root.join(".world/project.json"),
            br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#.to_vec(),
        )
        .unwrap();
    project
        .create_authoring_document(&map, b"old".to_vec())
        .unwrap();
    let source = root.join("extra.wl");
    fs::write(&source, "// old").unwrap();
    project.refresh().unwrap();
    project.save().unwrap();
    project.delete_document(&map).unwrap();
    project.delete_document(&source).unwrap();
    project.save().unwrap();
    fs::write(&map, b"external").unwrap();
    fs::write(&source, "// external").unwrap();
    assert!(project.refresh().unwrap().is_empty());
    assert_eq!(project.document(&source).unwrap(), "// external");
    assert_eq!(
        project.authoring_document(&map).unwrap().bytes(),
        b"external"
    );
    assert!(!project.authoring_document(&map).unwrap().is_deleted());
    assert!(!project.is_dirty());
}

#[test]
fn editing_cannot_promote_documents_to_an_unsupported_format() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    let mut project = Project::new(&root);
    project.save().unwrap();
    let manifest = root.join(".world/project.json");
    project
        .create_authoring_document(&manifest, br#"{"schema_version":1,"maps":{}}"#.to_vec())
        .unwrap();
    project.save().unwrap();
    let before = fs::read(&manifest).unwrap();
    for bytes in [
        br#"{"schema_version":2}"#.as_slice(),
        br#"{"schema_version":1,"required_features":["future.v9"]}"#.as_slice(),
    ] {
        assert!(project
            .set_authoring_document(&manifest, bytes.to_vec())
            .is_err());
        assert_eq!(
            project.authoring_document(&manifest).unwrap().bytes(),
            before
        );
        assert!(!project.is_dirty());
    }
}

#[test]
fn stale_undo_snapshot_cannot_overwrite_external_refresh() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    let mut project = Project::new(&root);
    project.save().unwrap();
    let before = project.clone();
    let entry = project.entry.clone();
    let external = format!(
        "{}\n// external change\n",
        project.document(&entry).unwrap()
    );
    fs::write(&entry, external.as_bytes()).unwrap();
    project.refresh().unwrap();
    project.restore(before);
    assert_eq!(project.document(&entry).unwrap(), external);
    project.save().unwrap();
    assert_eq!(fs::read_to_string(&entry).unwrap(), external);
}

#[test]
fn external_capability_upgrade_locks_dirty_documents_without_losing_drafts() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    let mut project = Project::new(&root);
    project.save().unwrap();
    let manifest = root.join(".world/project.json");
    let map = root.join(".world/maps/map.json");
    project
        .create_authoring_document(
            &manifest,
            br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#.to_vec(),
        )
        .unwrap();
    project
        .create_authoring_document(&map, b"old".to_vec())
        .unwrap();
    project.save().unwrap();
    project
        .set_authoring_document(&map, b"local draft".to_vec())
        .unwrap();
    fs::write(
        &manifest,
        br#"{"schema_version":2,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    project.refresh().unwrap();
    assert!(project.authoring_document(&map).unwrap().is_read_only());
    assert!(project.save().is_err());
    assert_eq!(fs::read(&map).unwrap(), b"old");
    assert_eq!(
        project.authoring_document(&map).unwrap().bytes(),
        b"local draft"
    );
    project.save_as(&temp.0.join("rescue")).unwrap();
    assert_eq!(
        fs::read(temp.0.join("rescue/.world/maps/map.json")).unwrap(),
        b"local draft"
    );
}

#[test]
fn external_capability_upgrade_also_blocks_unsaved_new_documents() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    let mut project = Project::new(&root);
    project.save().unwrap();
    let manifest = root.join(".world/project.json");
    let map = root.join(".world/maps/new.json");
    project
        .create_authoring_document(
            &manifest,
            br#"{"schema_version":1,"maps":{"new":".world/maps/new.json"}}"#.to_vec(),
        )
        .unwrap();
    project.save().unwrap();
    project
        .create_authoring_document(&map, b"new local draft".to_vec())
        .unwrap();
    fs::write(
        &manifest,
        br#"{"schema_version":2,"maps":{"new":".world/maps/new.json"}}"#,
    )
    .unwrap();
    project.refresh().unwrap();
    assert!(project.authoring_document(&map).unwrap().is_read_only());
    assert!(project.save().is_err());
    assert!(!map.exists());
    project.save_as(&temp.0.join("rescue")).unwrap();
    assert_eq!(
        fs::read(temp.0.join("rescue/.world/maps/new.json")).unwrap(),
        b"new local draft"
    );
}

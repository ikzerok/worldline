use std::{fs, path::PathBuf};
use worldline_core::{compile_path, project::Project};

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
    assert_eq!(project.refresh().unwrap(), vec![map.clone()]);
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

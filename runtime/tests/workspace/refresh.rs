use super::*;

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

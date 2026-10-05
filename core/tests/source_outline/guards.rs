use super::*;

#[test]
fn exact_unsaved_editor_source_is_read_only_and_not_an_applied_buffer_requirement() {
    let source = "event old\n  -> END\n";
    let mut fixture = Fixture::new(source, None);
    let before = fixture.project.content_baseline();
    let compiled_before = fixture.project.compile();
    let mut without_provenance = compiled_before.program.clone();
    without_provenance.source_provenance = Default::default();
    assert_eq!(
        worldline_core::fingerprint_program(&compiled_before.program),
        worldline_core::fingerprint_program(&without_provenance)
    );
    let draft = "// 新稿🙂\nevent fresh as \"新名\"\n  新正文\n  -> END\n";
    let outline = fixture.ready(draft);
    assert_eq!(outline.entries[0].id, "fresh");
    assert!(outline.matches_source(draft));
    assert!(!outline.matches_source(source));
    assert_eq!(
        fixture.project.document(&fixture.project.entry).unwrap(),
        source
    );
    assert_eq!(fs::read_to_string(&fixture.project.entry).unwrap(), source);
    assert_eq!(fixture.project.content_baseline(), before);
    let compiled_after = fixture.project.compile();
    assert_eq!(
        compiled_before.analysis.fingerprint,
        compiled_after.analysis.fingerprint
    );
    assert_eq!(compiled_before.sources, compiled_after.sources);
    assert_eq!(
        worldline_core::fingerprint_program(&compiled_before.program),
        worldline_core::fingerprint_program(&compiled_after.program)
    );
    assert!(!fixture.project.is_dirty());
    let path = fixture.project.entry.clone();
    fixture.project.set_text(&path, draft.into()).unwrap();
    assert!(fixture
        .project
        .source_outline_range(&outline, draft, 0)
        .is_err());
    let applied = fixture.ready(draft);
    assert_eq!(applied.entries[0].id, "fresh");
    assert!(fixture.project.is_dirty());
}

#[test]
fn stale_text_tampering_and_current_invalid_draft_disable_navigation() {
    let source = "event e\n  -> END\n";
    let fixture = Fixture::new(source, None);
    let outline = fixture.ready(source);
    assert!(fixture
        .project
        .source_outline_range(&outline, "\nevent e\n  -> END\n", 0)
        .is_err());
    let mut tampered = outline.clone();
    tampered.entries[0].header.start += 1;
    assert!(fixture
        .project
        .source_outline_range(&tampered, source, 0)
        .is_err());
    for draft in [
        "event",
        "event e\n",
        "event e\n\t坏缩进\n",
        "/*未闭合",
        "event e\n  if (\n    文本\n",
    ] {
        let broken = fixture.outline(draft);
        assert_eq!(
            broken.status,
            Status::SyntaxInvalid,
            "{draft}: {:?}",
            broken.message
        );
        assert!(broken.entries.is_empty());
        assert!(fixture
            .project
            .source_outline_range(&broken, draft, 0)
            .is_err());
    }
}

#[test]
fn empty_comment_only_and_non_declaration_files_are_ready_empty() {
    let fixture = Fixture::new("", None);
    for source in [
        "",
        "\r\n // 注释🙂\r\n",
        "alias character missing as \"别名\"\n",
        "include \"missing.wl\"\n",
    ] {
        let outline = fixture.ready(source);
        assert!(outline.entries.is_empty());
        assert!(outline.current_item(0).is_none());
    }
}

#[test]
fn broken_other_file_does_not_block_a_precise_current_file_outline() {
    let fixture = Fixture::new("event e\n  -> END\n", None);
    fs::write(fixture.root.join("broken.wl"), "event broken\n").unwrap();
    let mut project = Project::open(&fixture.root).unwrap();
    assert!(project.compile().has_errors());
    let source = project.document(&project.entry).unwrap();
    let outline = project.source_outline(&project.entry, source);
    assert_eq!(outline.status, Status::Ready, "{:?}", outline.message);
    assert_eq!(outline.entries.len(), 1);
}

#[test]
fn external_changes_conflicts_refresh_and_manifest_changes_invalidate_old_queries() {
    let source = "event e\n  -> END\n";
    let mut fixture = Fixture::new(source, None);
    let outline = fixture.ready(source);
    fs::write(&fixture.project.entry, "// 外改\nevent e\n  -> END\n").unwrap();
    assert!(fixture
        .project
        .source_outline_range(&outline, source, 0)
        .is_err());
    assert_eq!(fixture.outline(source).status, Status::Unavailable);
    fixture.project.refresh().unwrap();
    assert!(fixture
        .project
        .source_outline_range(&outline, source, 0)
        .is_err());
    let refreshed = fixture
        .project
        .document(&fixture.project.entry)
        .unwrap()
        .to_string();
    fixture.ready(&refreshed);
    let path = fixture.project.entry.clone();
    fixture
        .project
        .set_text(&path, "event local\n  -> END\n".into())
        .unwrap();
    fs::write(&path, "event external\n  -> END\n").unwrap();
    assert!(!fixture.project.refresh().unwrap().is_empty());
    assert_eq!(
        fixture.outline("event local\n  -> END\n").status,
        Status::Unavailable
    );
    assert_eq!(
        fixture.project.document(&path).unwrap(),
        "event local\n  -> END\n"
    );

    let mut manifest = Fixture::new(source, None);
    let old = manifest.ready(source);
    fs::create_dir_all(manifest.root.join(".world")).unwrap();
    fs::write(
        manifest.root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.13","required_features":[]}"#,
    )
    .unwrap();
    assert!(manifest
        .project
        .source_outline_range(&old, source, 0)
        .is_err());
    manifest.project.refresh().unwrap();
    assert!(manifest
        .project
        .source_outline_range(&old, source, 0)
        .is_err());
}

#[test]
fn archived_unknown_capability_deleted_and_moved_sources_never_jump() {
    let source = "character c\n";
    let mut fixture = Fixture::new(source, None);
    fs::write(fixture.root.join("draft.wl"), source).unwrap();
    fs::create_dir_all(fixture.root.join(".world")).unwrap();
    fs::write(fixture.root.join(".world/project.json"), r#"{"schema_version":1,"required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":["world.wl"],"archived":["draft.wl"]}}"#).unwrap();
    fixture.project.refresh().unwrap();
    assert_eq!(
        fixture
            .project
            .source_outline(&fixture.root.join("draft.wl"), source)
            .status,
        Status::Inactive
    );
    let current = fixture.ready(source);
    fs::rename(fixture.root.join("world.wl"), fixture.root.join("moved.wl")).unwrap();
    assert!(fixture
        .project
        .source_outline_range(&current, source, 0)
        .is_err());
    assert_eq!(fixture.outline(source).status, Status::Unavailable);

    let unknown = Fixture::new(source, Some("9.9"));
    assert_eq!(unknown.outline(source).status, Status::Unavailable);
    let mut unknown_feature = Fixture::new(source, None);
    fs::create_dir_all(unknown_feature.root.join(".world")).unwrap();
    fs::write(
        unknown_feature.root.join(".world/project.json"),
        r#"{"schema_version":1,"required_features":["unknown.v999"]}"#,
    )
    .unwrap();
    unknown_feature.project.refresh().unwrap();
    assert_eq!(unknown_feature.outline(source).status, Status::Unavailable);
}

#[test]
fn tombstones_and_unresolved_transactions_disable_navigation_without_writes() {
    let mut fixture = Fixture::new("character c\n", None);
    let other = fixture.root.join("other.wl");
    fs::write(&other, "character other\n").unwrap();
    fixture.project.refresh().unwrap();
    let outline = fixture.project.source_outline(&other, "character other\n");
    assert_eq!(outline.status, Status::Ready);
    fixture.project.delete_document(&other).unwrap();
    assert!(fixture
        .project
        .source_outline_range(&outline, "character other\n", 0)
        .is_err());
    assert_eq!(
        fixture
            .project
            .source_outline(&other, "character other\n")
            .status,
        Status::Unavailable
    );
    fs::create_dir_all(fixture.root.join(".world/.transactions/pending")).unwrap();
    let baseline = fixture.project.content_baseline();
    assert_eq!(fixture.outline("character c\n").status, Status::Unavailable);
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert!(other.exists());
}

#[test]
fn explicit_113_character_ref_capability_is_used_without_automatic_upgrades() {
    let source =
        "character c\n  property friend = ref(\"character\", \"other\")\ncharacter other\n";
    let mut fixture = Fixture::new(source, Some("1.13"));
    assert_eq!(fixture.outline(source).status, Status::SyntaxInvalid);
    fs::write(fixture.root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.13","required_features":["content.object_refs.v1","content.character_refs.v1"]}"#).unwrap();
    fixture.project.refresh().unwrap();
    assert_eq!(fixture.ready(source).entries.len(), 2);
    assert_eq!(fixture.project.language_version(), "1.13");
}

#[cfg(unix)]
#[test]
fn symlink_boundaries_cannot_be_used_for_a_source_outline() {
    let fixture = Fixture::new("character c\n", None);
    let other = Fixture::new("character outside\n", None);
    std::os::unix::fs::symlink(&other.project.entry, fixture.root.join("link.wl")).unwrap();
    assert_eq!(
        fixture
            .project
            .source_outline(&fixture.root.join("link.wl"), "character outside\n")
            .status,
        Status::Unavailable
    );
    assert_eq!(fixture.outline("character c\n").status, Status::Unavailable);
}

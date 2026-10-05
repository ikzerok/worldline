use super::*;

const SOURCE: &str = "event start\n  初稿\n  -> END\n";

#[test]
fn unsaved_unapplied_text_is_the_only_coordinate_source_and_queries_are_read_only() {
    let mut fixture = Fixture::new(SOURCE, None);
    let entry = fixture.project.entry.clone();
    let compiled = fixture.project.compile();
    let baseline = fixture.project.content_baseline();
    let snapshot = fixture.project.clone();
    let draft = "// 🙂 新稿\r\n/*坏语法\r\n当前正文";
    let preview = fixture.ready(draft, "3:5");
    assert_eq!(preview.position.byte_offset, draft.len());
    assert_eq!(preview.context.text, "当前正文");
    assert!(fixture
        .project
        .resolve_source_jump(&preview, SOURCE)
        .is_err());
    assert_eq!(fixture.project.document(&entry).unwrap(), SOURCE);
    assert_eq!(fs::read_to_string(&entry).unwrap(), SOURCE);
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert!(!fixture.project.is_dirty());
    let after = fixture.project.compile();
    assert_eq!(compiled.analysis.fingerprint, after.analysis.fingerprint);
    assert_eq!(compiled.sources, after.sources);
    assert!(fixture.project.restore(snapshot));
    fixture.project.set_text(&entry, draft.into()).unwrap();
    assert!(fixture
        .project
        .resolve_source_jump(&preview, draft)
        .is_err());
    fixture.ready(draft, "3:5");
    assert!(fixture.project.is_dirty());
}

#[test]
fn each_public_preview_field_is_revalidated_against_private_original_target() {
    let fixture = Fixture::new(SOURCE, None);
    let preview = fixture.ready(SOURCE, "2:3");
    let mut cases = Vec::new();
    macro_rules! altered {
        ($field:ident, $value:expr) => {{
            let mut altered = preview.clone();
            altered.$field = $value;
            cases.push(altered);
        }};
    }
    altered!(path, fixture.root.join("other.wl"));
    altered!(line_count, preview.line_count + 1);
    altered!(max_column, preview.max_column + 1);
    let mut position = preview.position;
    position.line += 1;
    altered!(position, position);
    position = preview.position;
    position.column += 1;
    altered!(position, position);
    position = preview.position;
    position.byte_offset += 1;
    altered!(position, position);
    position = preview.position;
    position.character_offset += 1;
    altered!(position, position);
    let mut context = preview.context.clone();
    context.text.push('x');
    altered!(context, context);
    context = preview.context.clone();
    context.byte_range.start += 1;
    altered!(context, context);
    context = preview.context.clone();
    context.byte_range.end += 1;
    altered!(context, context);
    context = preview.context.clone();
    context.start_column += 1;
    altered!(context, context);
    context = preview.context.clone();
    context.truncated_start = !context.truncated_start;
    altered!(context, context);
    context = preview.context.clone();
    context.truncated_end = !context.truncated_end;
    altered!(context, context);
    // 即使同时替换所有公开坐标与语境，也不能改变原先预览的请求。
    let target = fixture.ready(SOURCE, "1:1");
    let mut rewritten = preview.clone();
    rewritten.position = target.position;
    rewritten.context = target.context;
    rewritten.max_column = target.max_column;
    cases.push(rewritten);
    for altered in cases {
        assert!(
            fixture
                .project
                .resolve_source_jump(&altered, SOURCE)
                .is_err(),
            "{altered:?}"
        );
    }
    assert_eq!(
        fixture.project.document(&fixture.project.entry).unwrap(),
        SOURCE
    );
    assert!(!fixture.project.is_dirty());
}

#[test]
fn same_length_changes_other_buffers_and_another_root_reject_old_previews() {
    let mut fixture = Fixture::new(SOURCE, None);
    let preview = fixture.ready(SOURCE, "2");
    assert!(fixture
        .project
        .resolve_source_jump(&preview, &SOURCE.replace("初稿", "新稿"))
        .is_err());
    let other = Fixture::new(SOURCE, None);
    assert_eq!(
        fixture.project.content_baseline(),
        other.project.content_baseline()
    );
    assert!(other.project.resolve_source_jump(&preview, SOURCE).is_err());
    let mut moved_path = preview.clone();
    moved_path.path = other.project.entry.clone();
    assert!(other
        .project
        .resolve_source_jump(&moved_path, SOURCE)
        .is_err());

    let path = fixture.root.join("other.wl");
    fs::write(&path, "character other\n").unwrap();
    fixture.project.refresh().unwrap();
    let fresh = fixture.ready(SOURCE, "2");
    fixture
        .project
        .set_text(&path, "/*bad draft".into())
        .unwrap();
    assert!(fixture.project.resolve_source_jump(&fresh, SOURCE).is_err());
    fixture.ready(SOURCE, "2");
}

#[test]
fn save_alone_keeps_preview_valid_without_turning_navigation_into_save() {
    let mut fixture = Fixture::new(SOURCE, None);
    let path = fixture.project.entry.clone();
    let draft = SOURCE.replace("初稿", "已应用稿");
    fixture.project.set_text(&path, draft.clone()).unwrap();
    let preview = fixture.ready(&draft, "2:3");
    let baseline = fixture.project.content_baseline();
    fixture.project.save().unwrap();
    assert_eq!(fixture.project.content_baseline(), baseline);
    fixture
        .project
        .resolve_source_jump(&preview, &draft)
        .unwrap();
    assert!(!fixture.project.is_dirty());
}

#[test]
fn external_change_refresh_generation_and_conflicts_require_new_preview() {
    let mut fixture = Fixture::new(SOURCE, None);
    let preview = fixture.ready(SOURCE, "2");
    let entry = fixture.project.entry.clone();
    let external = SOURCE.replace("初稿", "外稿");
    fs::write(&entry, &external).unwrap();
    assert!(fixture
        .project
        .resolve_source_jump(&preview, SOURCE)
        .is_err());
    assert!(fixture.preview(SOURCE, "2").is_err());
    assert_eq!(fixture.project.document(&entry).unwrap(), SOURCE);
    fixture.project.refresh().unwrap();
    assert!(fixture
        .project
        .resolve_source_jump(&preview, SOURCE)
        .is_err());
    fixture.ready(&external, "2");
    // 恢复相同内容基线也不复活更早刷新代次的预览。
    fs::write(&entry, SOURCE).unwrap();
    fixture.project.refresh().unwrap();
    assert!(fixture
        .project
        .resolve_source_jump(&preview, SOURCE)
        .is_err());
    let local = SOURCE.replace("初稿", "本稿");
    fixture.project.set_text(&entry, local.clone()).unwrap();
    fs::write(&entry, external).unwrap();
    assert!(!fixture.project.refresh().unwrap().is_empty());
    assert!(fixture.preview(&local, "2").is_err());
    assert_eq!(fixture.project.document(&entry).unwrap(), local);
}

#[test]
fn added_deleted_moved_invalid_utf8_and_untracked_sources_are_not_silently_loaded() {
    for change in ["added", "deleted", "moved", "invalid_utf8"] {
        let fixture = Fixture::new(SOURCE, None);
        let preview = fixture.ready(SOURCE, "2");
        let baseline = fixture.project.content_baseline();
        match change {
            "added" => fs::write(fixture.root.join("new.wl"), "event").unwrap(),
            "deleted" => fs::remove_file(&fixture.project.entry).unwrap(),
            "moved" => fs::rename(&fixture.project.entry, fixture.root.join("moved.wl")).unwrap(),
            _ => fs::write(&fixture.project.entry, [0xff, 0xfe]).unwrap(),
        }
        assert!(
            fixture
                .project
                .resolve_source_jump(&preview, SOURCE)
                .is_err(),
            "{change}"
        );
        assert!(fixture.preview(SOURCE, "2").is_err(), "{change}");
        assert_eq!(fixture.project.content_baseline(), baseline);
        assert_eq!(
            fixture.project.document(&fixture.project.entry).unwrap(),
            SOURCE
        );
    }
    let fixture = Fixture::new(SOURCE, None);
    assert!(fixture
        .project
        .preview_source_jump(&fixture.root.join("not-loaded.wl"), SOURCE, "1")
        .is_err());
    assert!(fixture
        .project
        .preview_source_jump(&fixture.root.join("data.json"), SOURCE, "1")
        .is_err());
}

#[test]
fn new_or_changed_manifest_and_unknown_capability_or_schema_are_guarded() {
    for manifest in [
        r#"{"schema_version":1,"required_features":["unknown.v999"]}"#,
        r#"{"schema_version":999,"required_features":[]}"#,
        r#"{"schema_version":1,"language_version":"9.9","required_features":[]}"#,
        "{broken",
    ] {
        let mut fixture = Fixture::new(SOURCE, None);
        let preview = fixture.ready(SOURCE, "1");
        fs::create_dir_all(fixture.root.join(".world")).unwrap();
        fs::write(fixture.root.join(".world/project.json"), manifest).unwrap();
        assert!(fixture
            .project
            .resolve_source_jump(&preview, SOURCE)
            .is_err());
        fixture.project.refresh().unwrap();
        assert!(!fixture.project.authoring_diagnostics().is_empty());
        assert!(fixture.preview(SOURCE, "1").is_err());
        assert_eq!(
            fs::read_to_string(fixture.root.join(".world/project.json")).unwrap(),
            manifest
        );
        assert_eq!(
            fixture.project.document(&fixture.project.entry).unwrap(),
            SOURCE
        );
    }
    let mut fixture = Fixture::new(SOURCE, Some("1.9"));
    let preview = fixture.ready(SOURCE, "1");
    fixture.manifest(r#"{"schema_version":1,"language_version":"1.13","required_features":[]}"#);
    assert!(fixture
        .project
        .resolve_source_jump(&preview, SOURCE)
        .is_err());
    fixture.ready(SOURCE, "1");
}

#[test]
fn archived_and_unselected_tracked_sources_are_plain_text_without_activation() {
    let mut fixture = Fixture::new(SOURCE, None);
    fs::write(fixture.root.join("draft.wl"), "/*not valid").unwrap();
    fs::write(fixture.root.join("unselected.wl"), "event").unwrap();
    fixture.manifest(r#"{"schema_version":1,"required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":["world.wl"],"archived":["draft.wl"]}}"#);
    let baseline = fixture.project.content_baseline();
    for path in [
        fixture.root.join("draft.wl"),
        fixture.root.join("unselected.wl"),
    ] {
        let text = fixture.project.document(&path).unwrap();
        let preview = fixture
            .project
            .preview_source_jump(&path, text, "1:2")
            .unwrap();
        assert_eq!(
            fixture.project.resolve_source_jump(&preview, text).unwrap(),
            1..1
        );
        assert!(!fixture.project.source_selection().unwrap().is_active(&path));
    }
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert!(!fixture.project.is_dirty());
}

#[test]
fn tombstones_and_pending_transactions_are_not_recovered_by_navigation() {
    let mut fixture = Fixture::new(SOURCE, None);
    let preview = fixture.ready(SOURCE, "1");
    let entry = fixture.project.entry.clone();
    fixture.project.delete_document(&entry).unwrap();
    assert!(fixture
        .project
        .resolve_source_jump(&preview, SOURCE)
        .is_err());
    assert!(fixture.preview(SOURCE, "1").is_err());
    assert!(entry.exists());

    let fixture = Fixture::new(SOURCE, None);
    let preview = fixture.ready(SOURCE, "1");
    let transaction = fixture.root.join(".world/.transactions/pending");
    fs::create_dir_all(&transaction).unwrap();
    fs::write(transaction.join("marker"), "keep").unwrap();
    assert!(fixture
        .project
        .resolve_source_jump(&preview, SOURCE)
        .is_err());
    assert!(fixture.preview(SOURCE, "1").is_err());
    assert_eq!(
        fs::read_to_string(transaction.join("marker")).unwrap(),
        "keep"
    );
    assert_eq!(fs::read_to_string(&fixture.project.entry).unwrap(), SOURCE);
    assert!(!fixture.project.is_dirty());
}

#[test]
fn registered_document_external_change_and_whole_workspace_budget_are_checked() {
    let mut fixture = Fixture::new(SOURCE, None);
    fs::create_dir_all(fixture.root.join(".world")).unwrap();
    let book = fixture.root.join(".world/book.json");
    fs::write(&book, r#"{"schema_version":1,"id":"book","entries":[]}"#).unwrap();
    fixture.manifest(r#"{"schema_version":1,"required_features":["presentation.manuscripts.v1"],"manuscripts":{"book":".world/book.json"}}"#);
    let preview = fixture.ready(SOURCE, "1");
    fs::write(&book, [0xff]).unwrap();
    assert!(fixture
        .project
        .resolve_source_jump(&preview, SOURCE)
        .is_err());
    assert_eq!(fs::read(&book).unwrap(), [0xff]);

    let fixture = Fixture::new(SOURCE, None);
    let mut project = fixture.project.clone();
    let document = project.documents[&project.entry].clone();
    for index in 0..4096 {
        project.documents.insert(
            fixture.root.join(format!("buffer-{index}.wl")),
            document.clone(),
        );
    }
    assert!(project
        .preview_source_jump(&project.entry, SOURCE, "1")
        .unwrap_err()
        .contains("4096"));
}

#[test]
fn missing_new_workspace_can_be_queried_without_creating_it() {
    let fixture = Fixture::new(SOURCE, None);
    let root = fixture.root.join("not-created");
    let project = Project::new(&root);
    let preview = project
        .preview_source_jump(&project.entry, "未应用稿", "1:5")
        .unwrap();
    assert_eq!(
        project.resolve_source_jump(&preview, "未应用稿").unwrap(),
        12..12
    );
    assert!(!root.exists());
}

#[cfg(unix)]
#[test]
fn symlinks_and_outside_paths_are_rejected_while_read_only_text_stays_navigable() {
    let fixture = Fixture::new(SOURCE, None);
    let outside = Fixture::new(SOURCE, None);
    assert!(fixture
        .project
        .preview_source_jump(&outside.project.entry, SOURCE, "1")
        .is_err());
    let mut permissions = fs::metadata(&fixture.project.entry).unwrap().permissions();
    let original = permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&fixture.project.entry, permissions).unwrap();
    fixture.ready(SOURCE, "1");
    fs::set_permissions(&fixture.project.entry, original).unwrap();
    std::os::unix::fs::symlink(&outside.project.entry, fixture.root.join("link.wl")).unwrap();
    assert!(fixture.preview(SOURCE, "1").is_err());
    assert!(fixture
        .project
        .preview_source_jump(&fixture.root.join("link.wl"), SOURCE, "1")
        .is_err());
}

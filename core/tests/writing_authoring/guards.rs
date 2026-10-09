use super::*;

#[test]
fn canceled_previews_and_tampered_plans_never_mutate_project_or_input() {
    let (work, mut project) = Workspace::new(&[("world.wl", SOURCE)], None);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    buffer.replace_source(SOURCE.replace("你看见", "作者尚未应用的稿：你看见"));
    let command = request(&project, &buffer, "林😀", character(&project.entry));
    let baseline = project.content_baseline();
    let disk = work.bytes();
    let input = buffer_state(&buffer);
    let canceled = project
        .preview_writing_authoring(&[buffer.clone()], &command)
        .unwrap();
    drop(canceled);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(buffer_state(&buffer), input);
    assert_eq!(work.bytes(), disk);
    let plan = project
        .preview_writing_authoring(&[buffer.clone()], &command)
        .unwrap();
    let mutations: [fn(&mut WritingAuthoringPlan); 12] = [
        |plan| plan.changes.clear(),
        |plan| plan.changes[0].after.push_str("\n// 客户端篡改\n"),
        |plan| plan.changes[0].before = None,
        |plan| plan.changes[0].includes_unapplied_draft = false,
        |plan| plan.included_buffers.clear(),
        |plan| plan.included_buffers[0].generation += 1,
        |plan| plan.target.id = "other".into(),
        |plan| plan.source.id = "other".into(),
        |plan| plan.cursor_utf8 += 1,
        |plan| plan.link_start_utf8 += 1,
        |plan| plan.runtime_fingerprint_after ^= 1,
        |plan| plan.plan_digest.push('0'),
    ];
    for mutate in mutations {
        let mut modified = plan.clone();
        mutate(&mut modified);
        assert!(project
            .apply_writing_authoring(&[buffer.clone()], &modified)
            .is_err());
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(buffer_state(&buffer), input);
        assert_eq!(work.bytes(), disk);
    }
    project
        .apply_writing_authoring(&[buffer.clone()], &plan)
        .unwrap();
    assert_eq!(buffer_state(&buffer), input);
    assert_eq!(work.bytes(), disk);
}

#[test]
fn stale_generation_baseline_utf8_text_and_invalid_ranges_preserve_original_input() {
    let (work, mut project) = Workspace::new(&[("world.wl", SOURCE)], None);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let command = request(&project, &buffer, "林😀", character(&project.entry));
    let baseline = project.content_baseline();
    let disk = work.bytes();
    let original = buffer_state(&buffer);
    let mutations: [fn(&mut WritingAuthoringRequest); 9] = [
        |request| request.expected_baseline = "stale-baseline".into(),
        |request| request.generation += 1,
        |request| request.selection.start += 1,
        |request| request.selection.end -= 1,
        |request| request.selection.expected_text = "不同原文".into(),
        |request| request.selection.expected_text.clear(),
        |request| request.selection.start = request.selection.end + 1,
        |request| request.selection.end = usize::MAX,
        |request| request.source = TargetRef::new("event", "missing"),
    ];
    for mutate in mutations {
        let mut invalid = command.clone();
        mutate(&mut invalid);
        assert!(project
            .preview_writing_authoring(&[buffer.clone()], &invalid)
            .is_err());
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(buffer_state(&buffer), original);
        assert_eq!(work.bytes(), disk);
    }
    let plan = project
        .preview_writing_authoring(&[buffer.clone()], &command)
        .unwrap();
    buffer.replace_source(format!("{SOURCE}// 新输入\n"));
    buffer.replace_source(SOURCE.into());
    assert_eq!(buffer.source(), SOURCE);
    assert_eq!(buffer.generation(), 2);
    assert!(project
        .apply_writing_authoring(&[buffer.clone()], &plan)
        .is_err());
    assert_eq!(project.content_baseline(), baseline);
    let keep = buffer_state(&buffer);
    project
        .set_text(&project.entry.clone(), format!("{SOURCE}// 已应用变化\n"))
        .unwrap();
    let mut rebased_request = command;
    rebased_request.expected_baseline = project.content_baseline();
    rebased_request.generation = buffer.generation();
    let changed_baseline = project.content_baseline();
    assert!(project
        .preview_writing_authoring(&[buffer.clone()], &rebased_request)
        .is_err());
    assert_eq!(project.content_baseline(), changed_baseline);
    assert_eq!(buffer_state(&buffer), keep);
    assert_eq!(work.bytes(), disk);
}

#[test]
fn selected_buffer_must_be_unique_and_all_later_full_draft_changes_invalidate_plan() {
    let (work, mut project) = Workspace::new(
        &[("world.wl", SOURCE), ("people.wl", "// 资料原稿\n")],
        None,
    );
    let mut source = project.open_writing_buffer(&start()).unwrap();
    let mut people = project
        .open_source_writing_buffer(&work.0.join("people.wl"))
        .unwrap();
    people.replace_source("// 已预览资料整稿😀\n".into());
    let command = request(&project, &source, "林😀", character(people.path()));
    assert!(project.preview_writing_authoring(&[], &command).is_err());
    assert!(project
        .preview_writing_authoring(&[source.clone(), source.clone()], &command)
        .is_err());
    assert!(project
        .preview_writing_authoring(&[source.clone(), people.clone(), people.clone()], &command)
        .is_err());
    let plan = project
        .preview_writing_authoring(&[source.clone(), people.clone()], &command)
        .unwrap();
    let baseline = project.content_baseline();
    people.replace_source("// 确认前又输入的新资料整稿🧭\n".into());
    let keep = buffer_state(&people);
    assert!(project
        .apply_writing_authoring(&[source.clone(), people.clone()], &plan)
        .is_err());
    assert_eq!(buffer_state(&people), keep);
    assert_eq!(project.content_baseline(), baseline);
    source.replace_source(SOURCE.replace("你看见", "确认前新增的全文输入：你看见"));
    let source_keep = buffer_state(&source);
    assert!(project
        .apply_writing_authoring(&[source.clone(), people.clone()], &plan)
        .is_err());
    assert_eq!(buffer_state(&source), source_keep);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(fs::read_to_string(&project.entry).unwrap(), SOURCE);
}

#[test]
fn declarations_properties_comments_links_and_cross_line_selections_are_rejected() {
    let source = concat!(
        "character keeper as \"声明标签\"\n  property role = \"属性标签\"\n",
        "event start\n  // 注释标签\n  既有[[character:keeper|旧链接标签]]。\n",
        "  你看见林😀。\n  下一物理行。\n  -> END\n"
    );
    let (work, project) = Workspace::new(&[("world.wl", source)], None);
    assert!(!project.compile_object_search_snapshot().has_errors());
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let baseline = project.content_baseline();
    let disk = work.bytes();
    let keep = buffer_state(&buffer);
    for label in [
        "声明标签",
        "属性标签",
        "注释标签",
        "旧链接标签",
        "😀。\n  下一",
    ] {
        let command = request(&project, &buffer, label, character(&project.entry));
        assert!(
            project
                .preview_writing_authoring(std::slice::from_ref(&buffer), &command)
                .is_err(),
            "{label}"
        );
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(buffer_state(&buffer), keep);
        assert_eq!(work.bytes(), disk);
    }
}

#[test]
fn unrepresentable_labels_are_rejected_without_cleaning_or_truncating_them() {
    for label in ["林|😀", "林[😀", "林]😀", "林#😀", "林~😀"] {
        let source = format!("event start\n  你看见{label}。\n  -> END\n");
        let (work, project) = Workspace::new(&[("world.wl", &source)], None);
        let buffer = project.open_source_writing_buffer(&project.entry).unwrap();
        let command = request(&project, &buffer, label, character(&project.entry));
        let baseline = project.content_baseline();
        let keep = buffer_state(&buffer);
        assert!(
            project
                .preview_writing_authoring(std::slice::from_ref(&buffer), &command)
                .is_err(),
            "{label}"
        );
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(buffer_state(&buffer), keep);
        assert_eq!(fs::read_to_string(work.0.join("world.wl")).unwrap(), source);
    }
}

#[test]
fn missing_conflicting_and_deleted_targets_cannot_leave_an_orphan_declaration_or_link() {
    let source = format!("character lin as \"已有人物\"\n{SOURCE}");
    let (work, mut project) = Workspace::new(
        &[
            ("world.wl", &source),
            ("people.wl", "character gone as \"将删除的人物\"\n"),
        ],
        None,
    );
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let baseline = project.content_baseline();
    for target in [
        character(&project.entry),
        IntentTarget::Existing(TargetRef::new("character", "missing")),
    ] {
        let command = request(&project, &buffer, "林😀", target);
        assert!(project
            .preview_writing_authoring(std::slice::from_ref(&buffer), &command)
            .is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
    let command = request(
        &project,
        &buffer,
        "林😀",
        IntentTarget::Existing(TargetRef::new("character", "gone")),
    );
    let plan = project
        .preview_writing_authoring(std::slice::from_ref(&buffer), &command)
        .unwrap();
    project.delete_document(&work.0.join("people.wl")).unwrap();
    let deleted_baseline = project.content_baseline();
    let mut buffer = buffer;
    let keep = buffer_state(&buffer);
    assert!(project
        .insert_writing_reference(&mut buffer, &plan)
        .is_err());
    assert_eq!(buffer_state(&buffer), keep);
    assert_eq!(project.content_baseline(), deleted_baseline);
    assert_eq!(project.document(&project.entry).unwrap(), source);
    assert!(work.0.join("people.wl").exists());
}

#[test]
fn external_edits_deletes_and_new_sources_reject_old_plan_without_overwriting_disk() {
    for case in [
        "source",
        "destination",
        "new-source",
        "new-manifest",
        "delete-source",
    ] {
        let (work, mut project) = Workspace::new(
            &[("world.wl", SOURCE), ("people.wl", "// 资料原稿\n")],
            None,
        );
        let buffer = project.open_writing_buffer(&start()).unwrap();
        let command = request(
            &project,
            &buffer,
            "林😀",
            character(&work.0.join("people.wl")),
        );
        let plan = project
            .preview_writing_authoring(std::slice::from_ref(&buffer), &command)
            .unwrap();
        let baseline = project.content_baseline();
        let keep = buffer_state(&buffer);
        match case {
            "source" => fs::write(&project.entry, "event start\n  外部原稿。\n  -> END\n").unwrap(),
            "destination" => fs::write(work.0.join("people.wl"), "// 外部资料原稿\n").unwrap(),
            "new-source" => fs::write(work.0.join("new.wl"), "event new\n  -> END\n").unwrap(),
            "new-manifest" => {
                fs::create_dir_all(work.0.join(".world")).unwrap();
                fs::write(
                    work.0.join(".world/project.json"),
                    r#"{"schema_version":1,"language_version":"1.10"}"#,
                )
                .unwrap();
            }
            _ => fs::remove_file(&project.entry).unwrap(),
        }
        let external = work.bytes();
        assert!(
            project
                .apply_writing_authoring(std::slice::from_ref(&buffer), &plan)
                .is_err(),
            "{case}"
        );
        assert!(
            project
                .preview_writing_authoring(std::slice::from_ref(&buffer), &command)
                .is_err(),
            "{case}"
        );
        assert_eq!(project.content_baseline(), baseline, "{case}");
        assert_eq!(buffer_state(&buffer), keep, "{case}");
        assert_eq!(work.bytes(), external, "{case}");
    }
}

#[test]
fn unknown_capabilities_and_language_are_read_only_and_keep_all_raw_bytes() {
    for manifest in [
        r#"{"schema_version":1,"language_version":"1.9","required_features":["future.v99"],"x":1e2}"#,
        r#"{"schema_version":1,"language_version":"1.14","required_features":[],"x":1e2}"#,
    ] {
        let (work, project) = Workspace::new(&[("world.wl", SOURCE)], Some(manifest));
        let buffer = project.open_source_writing_buffer(&project.entry).unwrap();
        let command = request(&project, &buffer, "林😀", character(&project.entry));
        let baseline = project.content_baseline();
        let disk = work.bytes();
        let keep = buffer_state(&buffer);
        assert!(project
            .preview_writing_authoring(std::slice::from_ref(&buffer), &command)
            .is_err());
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(buffer_state(&buffer), keep);
        assert_eq!(work.bytes(), disk);
    }
}

#[cfg(unix)]
#[test]
fn readonly_source_and_destination_reject_preview_and_previously_valid_apply() {
    use std::os::unix::fs::PermissionsExt;
    for relative in ["world.wl", "people.wl"] {
        let (work, mut project) = Workspace::new(
            &[("world.wl", SOURCE), ("people.wl", "// 资料原稿\n")],
            None,
        );
        let buffer = project.open_writing_buffer(&start()).unwrap();
        let command = request(
            &project,
            &buffer,
            "林😀",
            character(&work.0.join("people.wl")),
        );
        let plan = project
            .preview_writing_authoring(std::slice::from_ref(&buffer), &command)
            .unwrap();
        let baseline = project.content_baseline();
        let disk = work.bytes();
        let path = work.0.join(relative);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let preview = project.preview_writing_authoring(std::slice::from_ref(&buffer), &command);
        let applied = project.apply_writing_authoring(std::slice::from_ref(&buffer), &plan);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(preview.is_err(), "{relative}");
        assert!(applied.is_err(), "{relative}");
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(work.bytes(), disk);
        assert_eq!(buffer.source(), SOURCE);
    }
}

#[test]
fn inactive_archived_tombstoned_and_outside_sources_are_never_activated() {
    let manifest = r#"{"schema_version":1,"language_version":"1.9","required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":["world.wl"],"archived":["archive.wl"]}}"#;
    let (work, mut project) = Workspace::new(
        &[
            ("world.wl", SOURCE),
            ("archive.wl", "event archived\n  林😀\n  -> END\n"),
            ("inactive.wl", "event inactive\n  林😀\n  -> END\n"),
        ],
        Some(manifest),
    );
    let source = project.open_writing_buffer(&start()).unwrap();
    let baseline = project.content_baseline();
    let disk = work.bytes();
    for relative in ["archive.wl", "inactive.wl"] {
        let path = work.0.join(relative);
        let archived = project.open_source_writing_buffer(&path).unwrap();
        let command = request(&project, &source, "林😀", character(&path));
        assert!(project
            .preview_writing_authoring(&[source.clone(), archived.clone()], &command)
            .is_err());
        let command = request(&project, &archived, "林😀", character(&project.entry));
        assert!(project
            .preview_writing_authoring(&[archived], &command)
            .is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
    let outside = work.0.parent().unwrap().join("outside-authoring.wl");
    let command = request(&project, &source, "林😀", character(&outside));
    assert!(project
        .preview_writing_authoring(std::slice::from_ref(&source), &command)
        .is_err());
    project
        .delete_document(&work.0.join("inactive.wl"))
        .unwrap();
    let deleted_baseline = project.content_baseline();
    let fresh_source = project.open_writing_buffer(&start()).unwrap();
    let command = request(
        &project,
        &fresh_source,
        "林😀",
        character(&work.0.join("inactive.wl")),
    );
    assert!(project
        .preview_writing_authoring(&[fresh_source], &command)
        .is_err());
    assert_eq!(project.content_baseline(), deleted_baseline);
    assert_eq!(project.sources().len(), 1);
    assert_eq!(work.bytes(), disk);
}

#[test]
fn request_and_buffer_count_budgets_return_no_partial_plan() {
    let (work, project) = Workspace::new(&[("world.wl", SOURCE)], None);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let command = request(&project, &buffer, "林😀", character(&project.entry));
    let baseline = project.content_baseline();
    let disk = work.bytes();
    assert!(project
        .preview_writing_authoring(&vec![buffer.clone(); 4097], &command)
        .is_err());
    let mut large = command;
    if let IntentTarget::CreateCharacter { draft, .. } = &mut large.target {
        draft.display =
            "字".repeat(worldline_core::manuscript::MAX_WRITING_AUTHORING_REQUEST_BYTES);
    }
    assert!(project
        .preview_writing_authoring(std::slice::from_ref(&buffer), &large)
        .is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(buffer.source(), SOURCE);
    assert_eq!(work.bytes(), disk);
}

#[test]
fn oversized_full_draft_is_rejected_before_invalid_source_is_compiled() {
    let (work, project) = Workspace::new(&[("world.wl", SOURCE)], None);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let mut huge = SOURCE.to_owned();
    huge.push_str("  if (\n");
    huge.push_str(&"x".repeat(worldline_core::manuscript::MAX_WRITING_AUTHORING_CHANGE_BYTES));
    buffer.replace_source(huge);
    let command = request(&project, &buffer, "林😀", character(&project.entry));
    let baseline = project.content_baseline();
    let disk = work.bytes();
    let error = project
        .preview_writing_authoring(&[buffer.clone()], &command)
        .err()
        .unwrap();
    assert!(error.contains("8 MiB"), "{error}");
    assert!(error.contains("未编译"), "{error}");
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(work.bytes(), disk);
    assert!(buffer.source().ends_with(&"x".repeat(64)));
}

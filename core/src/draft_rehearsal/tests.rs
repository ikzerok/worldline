use super::*;
use crate::state_inspection_source::{DeclarationKind, DeclarationSource};

fn fixture() -> (Project, WritingBuffer) {
    let root = std::env::temp_dir().join(format!(
        "draft-rehearsal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut project = Project::new(&root);
    project
        .set_text(
            &project.entry.clone(),
            "let count = 0\nevent start\n  原文🙂\n  choice \"继续\"\n    -> END\n".into(),
        )
        .unwrap();
    let mut buffer = project.open_source_writing_buffer(&project.entry).unwrap();
    buffer.replace_source(buffer.source().replace("原文🙂", "未应用新稿🌦️"));
    (project, buffer)
}

fn request(project: &Project, buffer: &WritingBuffer) -> DraftRehearsalRequest {
    DraftRehearsalRequest::from_writing_buffers(
        project,
        std::slice::from_ref(buffer),
        vec![DraftRehearsalExcludedInput {
            kind: "新建事件表单".into(),
            source: "待提交".into(),
        }],
        false,
    )
    .unwrap()
}

#[test]
fn immutable_snapshot_uses_real_draft_and_preserves_project_disk_and_buffer() {
    let (mut project, buffer) = fixture();
    project.save().unwrap();
    let before = project.snapshot_files().unwrap();
    let baseline = project.content_baseline();
    let snapshot = project
        .compile_draft_rehearsal(&request(&project, &buffer))
        .unwrap();
    assert!(snapshot.compiled().sources[&project.entry].contains("未应用新稿🌦️"));
    assert!(project.document(&project.entry).unwrap().contains("原文🙂"));
    assert_eq!(before, project.snapshot_files().unwrap());
    assert_eq!(
        std::fs::read_to_string(&project.entry).unwrap(),
        project.document(&project.entry).unwrap()
    );
    assert_eq!(baseline, project.content_baseline());
    assert_eq!(buffer.generation(), 1);
    assert_eq!(snapshot.scope().sources.len(), 1);
    assert_eq!(
        snapshot.scope().sources[0].kind,
        DraftRehearsalSourceKind::WritingDraft
    );
    assert_eq!(snapshot.scope().excluded_inputs[0].kind, "新建事件表单");
    snapshot
        .verify_navigation(&project, &[buffer], false)
        .unwrap();
    std::fs::remove_dir_all(&project.root).unwrap();
}

#[test]
fn duplicates_stale_generation_invalid_source_and_ime_are_rejected_without_edits() {
    let (mut project, mut buffer) = fixture();
    let before = project.content_baseline();
    assert!(DraftRehearsalRequest::from_writing_buffers(
        &project,
        &[buffer.clone(), buffer.clone()],
        vec![],
        false
    )
    .is_err());
    let mut duplicate = request(&project, &buffer);
    duplicate.drafts.push(duplicate.drafts[0].clone());
    assert!(project.compile_draft_rehearsal(&duplicate).is_err());
    let mut composing = request(&project, &buffer);
    composing.composing = true;
    assert!(project.compile_draft_rehearsal(&composing).is_err());
    let snapshot = project
        .compile_draft_rehearsal(&request(&project, &buffer))
        .unwrap();
    let source = buffer.source().to_owned();
    buffer.replace_source(format!("{source}// edit\n"));
    buffer.replace_source(source);
    assert!(snapshot
        .verify_current(&project, &[buffer.clone()], false)
        .is_err());
    buffer.replace_source("event start\n  if (\n".into());
    assert!(project
        .compile_draft_rehearsal(&request(&project, &buffer))
        .is_err());
    assert_eq!(project.content_baseline(), before);
    project
        .set_text(&project.entry.clone(), "event other\n  -> END\n".into())
        .unwrap();
    assert!(
        DraftRehearsalRequest::from_writing_buffers(&project, &[buffer], vec![], false).is_err()
    );
}

#[test]
fn exact_declaration_source_resolves_draft_line_and_rejects_forgery() {
    let (project, mut buffer) = fixture();
    buffer.replace_source(format!("// new first line\n{}", buffer.source()));
    let snapshot = project
        .compile_draft_rehearsal(&request(&project, &buffer))
        .unwrap();
    let source = DeclarationSource {
        kind: DeclarationKind::GlobalVariable,
        id: "count".into(),
        file: project.entry.display().to_string(),
        line: 2,
    };
    let hit = snapshot.declaration_source(&source).unwrap();
    assert_eq!(hit.preview, "let count = 0");
    assert!(hit.draft);
    let mut wrong = source;
    wrong.line = 1;
    assert!(snapshot.declaration_source(&wrong).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn disk_changes_new_sources_and_deletion_block_start_or_navigation() {
    let (mut project, buffer) = fixture();
    project.save().unwrap();
    let input = request(&project, &buffer);
    let snapshot = project.compile_draft_rehearsal(&input).unwrap();
    let original = std::fs::read(&project.entry).unwrap();
    std::fs::write(&project.entry, b"event external\n  -> END\n").unwrap();
    assert!(project.compile_draft_rehearsal(&input).is_err());
    assert!(snapshot
        .verify_navigation(&project, std::slice::from_ref(&buffer), false)
        .is_err());
    assert!(buffer.source().contains("未应用"));
    std::fs::write(&project.entry, original).unwrap();
    std::fs::write(project.root.join("new.wl"), b"event new\n  -> END\n").unwrap();
    assert!(project.compile_draft_rehearsal(&input).is_err());
    std::fs::remove_file(project.root.join("new.wl")).unwrap();
    std::fs::remove_file(&project.entry).unwrap();
    assert!(snapshot
        .verify_navigation(&project, &[buffer], false)
        .is_err());
    std::fs::remove_dir_all(&project.root).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unknown_capabilities_inactive_sources_and_hardlink_aliases_fail_closed() {
    let (mut project, _) = fixture();
    project.save().unwrap();
    std::fs::create_dir_all(project.root.join(".world")).unwrap();
    std::fs::write(
        project.root.join(".world/project.json"),
        br#"{"schema_version":1,"required_features":["future.unknown.v1"]}"#,
    )
    .unwrap();
    let readonly = Project::open(&project.entry).unwrap();
    let mut buffer = readonly
        .open_source_writing_buffer(&readonly.entry)
        .unwrap();
    buffer.replace_source(buffer.source().replace("原文", "草稿"));
    assert!(readonly
        .compile_draft_rehearsal(&request(&readonly, &buffer))
        .is_err());
    std::fs::write(project.root.join(".world/project.json"), br#"{"schema_version":1,"required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":["world.wl"],"archived":["archive.wl"]}}"#).unwrap();
    std::fs::write(
        project.root.join("archive.wl"),
        b"event archived\n  -> END\n",
    )
    .unwrap();
    let project = Project::open(&project.entry).unwrap();
    let mut inactive = project
        .open_source_writing_buffer(&project.root.join("archive.wl"))
        .unwrap();
    inactive.replace_source("event changed\n  -> END\n".into());
    assert!(project
        .compile_draft_rehearsal(&request(&project, &inactive))
        .is_err());
    let alias = project.root.join("alias.wl");
    std::fs::hard_link(&project.entry, &alias).unwrap();
    assert!(input::verify_file_identities(&[project.entry.clone(), alias]).is_err());
    std::fs::remove_dir_all(&project.root).unwrap();
}

#[test]
fn dto_budgets_paths_and_safe_generations_are_never_truncated() {
    let (project, buffer) = fixture();
    let mut input = request(&project, &buffer);
    input.drafts[0].path = "../escape.wl".into();
    assert!(input.validate().is_err());
    input = request(&project, &buffer);
    input.drafts[0].generation = 9_007_199_254_740_992;
    assert!(input.validate().is_err());
    input = request(&project, &buffer);
    input.drafts[0].source = "字".repeat(MAX_DRAFT_REHEARSAL_BYTES / 3 + 1);
    assert!(input.validate().is_err());
    input = request(&project, &buffer);
    input.excluded_inputs[0].source = "x".repeat(4097);
    assert!(input.validate().is_err());
}

#[test]
fn machine_draft_order_does_not_make_a_valid_snapshot_stale() {
    let (mut project, _) = fixture();
    let second_path = project.add_file(std::path::Path::new("a.wl")).unwrap();
    project
        .set_text(&second_path, "event side\n  旁章原文\n  -> END\n".into())
        .unwrap();
    let mut first = project.open_source_writing_buffer(&project.entry).unwrap();
    first.replace_source(first.source().replace("原文🙂", "当前正文"));
    let mut second = project.open_source_writing_buffer(&second_path).unwrap();
    second.replace_source(second.source().replace("旁章原文", "旁章草稿"));
    let buffers = vec![first, second];
    let canonical =
        DraftRehearsalRequest::from_writing_buffers(&project, &buffers, vec![], false).unwrap();
    let mut reversed = canonical.clone();
    reversed.drafts.reverse();
    assert_ne!(reversed.drafts, canonical.drafts);
    let baseline = project.content_baseline();
    let snapshot = project.compile_draft_rehearsal(&reversed).unwrap();
    reversed.verify_current(&project, &buffers, false).unwrap();
    snapshot.verify_current(&project, &buffers, false).unwrap();
    let mut reversed_buffers = buffers.clone();
    reversed_buffers.reverse();
    snapshot
        .verify_navigation(&project, &reversed_buffers, false)
        .unwrap();
    assert_eq!(snapshot.compiled().sources.len(), 2);
    assert_eq!(project.content_baseline(), baseline);
    let mut forged = reversed.clone();
    forged.drafts[0].generation += 1;
    assert!(forged.verify_current(&project, &buffers, false).is_err());
    forged = reversed;
    forged.drafts.push(forged.drafts[0].clone());
    assert!(forged.verify_current(&project, &buffers, false).is_err());
}

#![cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{catalog::TargetRef, manuscript::WritingBlockKind, project::Project};
struct Workspace(PathBuf);
impl Workspace {
    fn new(source: &str) -> (Self, Project) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-writing-insertion-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let mut project = Project::new(&root);
        project
            .set_text(&project.entry.clone(), source.into())
            .unwrap();
        (Self(root), project)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn start() -> TargetRef {
    TargetRef::new("event", "start")
}

#[test]
fn default_empty_slot_is_noop_until_real_input_then_clear_and_resume_same_offset() {
    let (_work, project) = Workspace::new("event start\n  -> END\n");
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let slot = project
        .project_writing_buffer(&buffer, &start())
        .unwrap()
        .empty_prose_slot
        .unwrap();
    let original = buffer.source().to_owned();
    project
        .insert_writing_prose(&mut buffer, &slot, "")
        .unwrap();
    assert_eq!(buffer.source(), original);
    assert_eq!(buffer.generation(), 0);
    project
        .insert_writing_prose(&mut buffer, &slot, "中文🙂第一段\n第二行")
        .unwrap();
    let projection = project.project_writing_buffer(&buffer, &start()).unwrap();
    assert!(projection.empty_prose_slot.is_none());
    let block = projection
        .blocks
        .iter()
        .find(|b| b.kind == WritingBlockKind::Prose)
        .unwrap();
    assert_eq!(block.range.start, slot.offset());
    assert_eq!(block.text, "中文🙂第一段\n第二行");
    buffer
        .replace_prose(projection.generation, block, "")
        .unwrap();
    let slot_two = project
        .project_writing_buffer(&buffer, &start())
        .unwrap()
        .empty_prose_slot
        .unwrap();
    assert_eq!(slot_two.offset(), slot.offset());
    project
        .insert_writing_prose(&mut buffer, &slot_two, "继续写作")
        .unwrap();
    assert_eq!(buffer.source(), "event start\n  继续写作\n  -> END\n");
    assert_eq!(project.document(&project.entry).unwrap(), original);
}

#[test]
fn crlf_unicode_no_final_newline_and_adjacent_declarations_keep_external_bytes() {
    for (source, newline) in [
        ("//前文🙂\r\nevent start\r\n  //保留\r\n  -> END //尾注\r\n\r\nevent other\r\n  另一章\r\n  -> END", "\r\n"),
        ("//前文🙂\nevent start\n  -> END", "\n"),
        ("//首行\r\nevent start\n  -> END\n\nevent other\r\n  -> END\r\n", "\n"),
    ] {
        let (_work, project) = Workspace::new(source);
        let mut buffer = project.open_writing_buffer(&start()).unwrap();
        let slot = project.project_writing_buffer(&buffer, &start()).unwrap().empty_prose_slot.unwrap();
        let at = slot.offset() - 2;
        project.insert_writing_prose(&mut buffer, &slot, "第一行🧭\n第二行").unwrap();
        let inserted = format!("  第一行🧭{newline}  第二行{newline}");
        assert_eq!(buffer.source(), format!("{}{inserted}{}", &source[..at], &source[at..]));
        assert_eq!(buffer.source().ends_with('\n'), source.ends_with('\n'));
    }
}

#[test]
fn original_blank_line_is_reused_without_growing_extra_lines() {
    let (_work, project) = Workspace::new("event start\n  \n  -> END\n");
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let slot = project
        .project_writing_buffer(&buffer, &start())
        .unwrap()
        .empty_prose_slot
        .unwrap();
    project
        .insert_writing_prose(&mut buffer, &slot, "文字")
        .unwrap();
    assert_eq!(buffer.source(), "event start\n  文字\n  -> END\n");
    assert_eq!(slot.offset(), "event start\n  ".len());
}

#[test]
fn stale_generation_project_language_and_disk_changes_preserve_buffer() {
    let (work, mut project) = Workspace::new("event start\n  -> END\n");
    project.save().unwrap();
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let slot = project
        .project_writing_buffer(&buffer, &start())
        .unwrap()
        .empty_prose_slot
        .unwrap();
    buffer.replace_source("event start\n  \n  -> END\n".into());
    let keep = buffer.source().to_owned();
    assert!(project
        .insert_writing_prose(&mut buffer, &slot, "不能写")
        .is_err());
    assert_eq!(buffer.source(), keep);
    let slot = project
        .project_writing_buffer(&buffer, &start())
        .unwrap()
        .empty_prose_slot
        .unwrap();
    fs::write(
        work.0.join("world.wl"),
        "event start\n  外部正文\n  -> END\n",
    )
    .unwrap();
    assert!(project
        .insert_writing_prose(&mut buffer, &slot, "不能写")
        .is_err());
    assert_eq!(buffer.source(), keep);
    fs::write(work.0.join("world.wl"), "event start\n  -> END\n").unwrap();
    project
        .create_authoring_document(
            &work.0.join(".world/project.json"),
            br#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#.to_vec(),
        )
        .unwrap();
    assert!(project
        .insert_writing_prose(&mut buffer, &slot, "不能写")
        .is_err());
    assert_eq!(buffer.source(), keep);
}

#[test]
fn complex_source_and_multiline_comment_do_not_invent_insertion_points() {
    for source in [
        "event start\n  if true\n    -> END\n  -> END\n",
        "event start\n  scene room\n    -> END\n  -> END\n",
        "event start\n  已有正文\n  -> END\n",
        "event start\n  /* 注释\n   */ -> END\n",
        "event start\n  ->> END\n",
    ] {
        let (_work, project) = Workspace::new(source);
        let buffer = project.open_writing_buffer(&start()).unwrap();
        let projection = project.project_writing_buffer(&buffer, &start());
        assert!(
            projection.is_err() || projection.unwrap().empty_prose_slot.is_none(),
            "{source}"
        );
    }
}

#[test]
fn source_syntax_input_is_kept_even_when_first_paragraph_is_invalid() {
    let (_work, project) = Workspace::new("event start\n  -> END\n");
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let slot = project
        .project_writing_buffer(&buffer, &start())
        .unwrap()
        .empty_prose_slot
        .unwrap();
    project
        .insert_writing_prose(&mut buffer, &slot, "if (")
        .unwrap();
    assert!(buffer.source().contains("  if (\n"));
    assert!(project.project_writing_buffer(&buffer, &start()).is_err());
    assert!(project.preview_writing_buffer(&buffer).is_err());
    assert!(buffer.source().contains("if ("));
}

#[test]
fn newly_arrived_manifest_and_deleted_source_invalidate_empty_slot() {
    let (work, mut project) = Workspace::new("event start\n  -> END\n");
    project.save().unwrap();
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let slot = project
        .project_writing_buffer(&buffer, &start())
        .unwrap()
        .empty_prose_slot
        .unwrap();
    let original = buffer.source().to_owned();
    fs::create_dir_all(work.0.join(".world")).unwrap();
    fs::write(
        work.0.join(".world/project.json"),
        r#"{"schema_version":1,"required_features":["unknown.v99"]}"#,
    )
    .unwrap();
    assert!(project
        .insert_writing_prose(&mut buffer, &slot, "保留输入")
        .is_err());
    assert_eq!(buffer.source(), original);
    fs::remove_file(work.0.join(".world/project.json")).unwrap();
    project.delete_document(&project.entry.clone()).unwrap();
    assert!(project
        .insert_writing_prose(&mut buffer, &slot, "保留输入")
        .is_err());
    assert_eq!(buffer.source(), original);
}

#[test]
fn scene_entity_and_fragment_do_not_receive_event_slots() {
    let (_work, mut project) = Workspace::new("entity place kind place\n  description \"\"\nfragment empty()\n  return\nevent start\n  scene room\n    -> END\n  -> END\n");
    project
        .create_authoring_document(
            &project.root.join(".world/project.json"),
            br#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#.to_vec(),
        )
        .unwrap();
    for target in [
        TargetRef::new("scene", "start.room"),
        TargetRef::new("entity", "place"),
        TargetRef::new("fragment", "empty"),
    ] {
        let buffer = project.open_writing_buffer(&target).unwrap();
        assert!(project
            .project_writing_buffer(&buffer, &target)
            .unwrap()
            .empty_prose_slot
            .is_none());
    }
}

#[test]
fn whitespace_first_then_prose_keeps_exact_input_and_same_control_offset() {
    let (_work, project) = Workspace::new("event start\n  -> END\n");
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let slot = project
        .project_writing_buffer(&buffer, &start())
        .unwrap()
        .empty_prose_slot
        .unwrap();
    let anchor = slot.offset();
    assert_eq!(slot.text(), "");
    project
        .insert_writing_prose(&mut buffer, &slot, "   \n\n")
        .unwrap();
    let slot = project
        .project_writing_buffer(&buffer, &start())
        .unwrap()
        .empty_prose_slot
        .unwrap();
    assert_eq!(slot.offset(), anchor);
    assert_eq!(slot.text(), "   \n\n");
    project
        .insert_writing_prose(&mut buffer, &slot, "   \n\n中文🙂")
        .unwrap();
    let projection = project.project_writing_buffer(&buffer, &start()).unwrap();
    let block = projection
        .blocks
        .iter()
        .find(|b| b.kind == WritingBlockKind::Prose)
        .unwrap();
    assert_eq!(block.range.start, anchor);
    assert_eq!(block.text, "   \n\n中文🙂");
    buffer
        .replace_prose(projection.generation, block, "")
        .unwrap();
    let slot = project
        .project_writing_buffer(&buffer, &start())
        .unwrap()
        .empty_prose_slot
        .unwrap();
    assert_eq!(slot.offset(), anchor);
    assert_eq!(slot.text(), "");
    project
        .insert_writing_prose(&mut buffer, &slot, "\n")
        .unwrap();
    let slot = project
        .project_writing_buffer(&buffer, &start())
        .unwrap()
        .empty_prose_slot
        .unwrap();
    assert_eq!(slot.text(), "\n");
    project
        .insert_writing_prose(&mut buffer, &slot, "")
        .unwrap();
    assert_eq!(buffer.source(), "event start\n  \n  -> END\n");
}

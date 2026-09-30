use super::*;
use crate::project::Project;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);

fn fixture() -> Project {
    let root = std::env::temp_dir().join(format!(
        "writing-buffer-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.documents.retain(|path, _| path == &entry);
    project
        .set_text(
            &entry,
            concat!(
                "character lin as \"林岑\"\n",
                "event first as \"开篇\"\n",
                "  // 保留注释\n",
                "  海雾与 [[character:lin|林岑]]。\n",
                "  scene dock\n",
                "    港口。\n",
                "    choice \"靠岸\"\n",
                "      踏上岸边。\n",
                "      -> END\n",
                "event second\n",
                "  另一章原文。\n",
                "  -> END\n",
            )
            .into(),
        )
        .unwrap();
    project
}
fn target(kind: &str, id: &str) -> TargetRef {
    TargetRef {
        kind: kind.into(),
        id: id.into(),
    }
}

#[test]
fn writing_projection_scopes_scene_and_preserves_unrelated_bytes() {
    let mut project = fixture();
    let original = project.document(&project.entry).unwrap().to_owned();
    let mut buffer = project
        .open_writing_buffer(&target("scene", "first.dock"))
        .unwrap();
    let projection = project
        .project_writing_buffer(&buffer, &target("scene", "first.dock"))
        .unwrap();
    assert!(!projection.source.contains("海雾"));
    assert!(!projection.source.contains("另一章"));
    assert!(projection
        .blocks
        .iter()
        .any(|block| block.kind == WritingBlockKind::Structure && block.text == "choice \"靠岸\""));
    let block = projection
        .blocks
        .iter()
        .find(|block| block.text == "港口。")
        .unwrap();
    buffer
        .replace_prose(projection.generation, block, "渡口。\n潮水退去。")
        .unwrap();
    assert_eq!(project.document(&project.entry).unwrap(), original);
    project.preview_writing_buffer(&buffer).unwrap();
    assert_eq!(project.document(&project.entry).unwrap(), original);
    let before = project.clone();
    project.apply_writing_buffer(&buffer).unwrap();
    assert_eq!(
        project.document(&project.entry).unwrap(),
        original.replace("    港口。", "    渡口。\n    潮水退去。")
    );
    assert!(project.restore(before));
    assert_eq!(project.document(&project.entry).unwrap(), original);
}

#[test]
fn writing_stale_projection_and_project_never_overwrite() {
    let mut project = fixture();
    let mut buffer = project
        .open_writing_buffer(&target("event", "first"))
        .unwrap();
    let projection = project
        .project_writing_buffer(&buffer, &target("event", "first"))
        .unwrap();
    let block = projection
        .blocks
        .iter()
        .find(|block| block.text == "港口。")
        .unwrap();
    buffer
        .replace_prose(projection.generation, block, "新稿。")
        .unwrap();
    let preserved = buffer.source().to_owned();
    assert!(buffer
        .replace_prose(projection.generation, block, "过期稿。")
        .is_err());
    assert_eq!(buffer.source(), preserved);
    let entry = project.entry.clone();
    project
        .set_text(&entry, "event first\n  外部缓冲。\n  -> END\n".into())
        .unwrap();
    let baseline = project.content_baseline();
    assert!(project
        .apply_writing_buffer(&buffer)
        .unwrap_err()
        .contains("过期"));
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(buffer.source(), preserved);
}

#[test]
fn invalid_draft_survives_modes_and_rejects_application() {
    let mut project = fixture();
    let mut buffer = project
        .open_writing_buffer(&target("event", "first"))
        .unwrap();
    let original = project.document(&project.entry).unwrap().to_owned();
    buffer.replace_source("event first\n  if (\n    尚未写完。\n".into());
    assert!(buffer.is_changed());
    assert!(project
        .project_writing_buffer(&buffer, &target("event", "first"))
        .is_err());
    assert!(project.apply_writing_buffer(&buffer).is_err());
    assert!(buffer.source().contains("尚未写完"));
    assert_eq!(project.document(&project.entry).unwrap(), original);
    buffer.replace_source(original.replace("港口。", "港口修改。"));
    project.apply_writing_buffer(&buffer).unwrap();
}

#[test]
fn same_buffer_projects_multiple_chapters_without_duplicate_sources() {
    let project = fixture();
    let mut buffer = project
        .open_writing_buffer(&target("event", "first"))
        .unwrap();
    let first = project
        .project_writing_buffer(&buffer, &target("event", "first"))
        .unwrap();
    let block = first
        .blocks
        .iter()
        .find(|block| block.text == "港口。")
        .unwrap();
    buffer
        .replace_prose(first.generation, block, "码头。")
        .unwrap();
    let second = project
        .project_writing_buffer(&buffer, &target("event", "second"))
        .unwrap();
    assert!(second.source.contains("另一章原文"));
    let first_again = project
        .project_writing_buffer(&buffer, &target("event", "first"))
        .unwrap();
    assert!(first_again.source.contains("码头。"));
}

#[test]
fn writing_preserves_crlf_and_rejects_external_disk_change() {
    let mut project = fixture();
    let entry = project.entry.clone();
    let source = project.document(&entry).unwrap().replace('\n', "\r\n");
    project.set_text(&entry, source.clone()).unwrap();
    project.save().unwrap();
    let mut buffer = project
        .open_writing_buffer(&target("event", "first"))
        .unwrap();
    let projection = project
        .project_writing_buffer(&buffer, &target("event", "first"))
        .unwrap();
    let block = projection
        .blocks
        .iter()
        .find(|block| block.text == "港口。")
        .unwrap();
    buffer
        .replace_prose(projection.generation, block, "港湾。\n另一行。")
        .unwrap();
    assert!(buffer.source().contains("港湾。\r\n    另一行。"));
    std::fs::write(&entry, source.replace("港口", "磁盘修改")).unwrap();
    assert!(project
        .apply_writing_buffer(&buffer)
        .unwrap_err()
        .contains("外部"));
    assert_eq!(project.document(&entry).unwrap(), source);
    std::fs::remove_dir_all(&project.root).unwrap();
}

fn organization() -> ManuscriptDraft {
    let mut entries = Vec::new();
    for (id, parent, kind) in [
        ("one", None, ManuscriptEntryKind::Section),
        ("nested", Some("one"), ManuscriptEntryKind::Section),
        ("chapter", Some("nested"), ManuscriptEntryKind::Chapter),
        ("two", None, ManuscriptEntryKind::Section),
    ] {
        entries.push(ManuscriptEntryDraft {
            id: id.into(),
            kind,
            parent_id: parent.map(str::to_owned),
            title: "重复标题".into(),
            summary: None,
            pov: None,
            status: None,
            goal: None,
            target_ref: (kind == ManuscriptEntryKind::Chapter).then(|| target("event", "first")),
        });
    }
    ManuscriptDraft {
        id: "book".into(),
        title: "书".into(),
        entries,
    }
}

#[test]
fn move_section_retains_descendants_and_rejects_cycles() {
    let mut draft = organization();
    assert!(draft.move_to_section("one", Some("nested")).is_err());
    assert!(draft.move_to_section("one", Some("chapter")).is_err());
    assert!(draft.move_to_section("one", Some("missing")).is_err());
    assert!(draft.move_to_section("one", Some("two")).unwrap());
    assert_eq!(
        draft.entry_subtree("one").unwrap(),
        ["one", "nested", "chapter"]
    );
    assert_eq!(draft.entries[2].parent_id.as_deref(), Some("nested"));
    assert!(!draft.move_to_section("one", Some("two")).unwrap());
    assert!(draft.move_to_section("chapter", None).unwrap());
    assert_eq!(draft.entries[2].parent_id, None);
}

#[test]
fn remove_only_explicit_arrangement_subtree() {
    let project = fixture();
    let baseline = project.content_baseline();
    let mut draft = organization();
    let removed = draft.remove_entry_subtree("one").unwrap();
    assert_eq!(removed, ["one", "nested", "chapter"]);
    assert_eq!(draft.entries.len(), 1);
    assert_eq!(draft.entries[0].id, "two");
    assert_eq!(project.content_baseline(), baseline);
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("event first"));
}

#[test]
fn explicit_source_transaction_keeps_invalid_draft_and_undo() {
    let mut project = fixture();
    let original = project.clone();
    let mut buffer = project.open_source_writing_buffer(&project.entry).unwrap();
    buffer.replace_source("event first\n  if (\n    尚未完成\n".into());
    let diagnostics = project.preview_source_writing_buffer(&buffer).unwrap();
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == crate::Severity::Error));
    assert!(!project
        .document(&project.entry)
        .unwrap()
        .contains("尚未完成"));
    project.apply_source_writing_buffer(&buffer).unwrap();
    assert_eq!(project.document(&project.entry).unwrap(), buffer.source());
    let reopened = project.open_source_writing_buffer(&project.entry).unwrap();
    assert_eq!(reopened.source(), buffer.source());
    assert!(project.restore(original));
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("另一章原文"));
}

#[test]
fn language_111_structures_are_not_misclassified_as_prose() {
    let mut project = fixture();
    let manifest = project.root.join(".world/project.json");
    project
        .create_authoring_document(
            &manifest,
            br#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#.to_vec(),
        )
        .unwrap();
    let entry = project.entry.clone();
    project
        .set_text(
            &entry,
            concat!(
                "character lin\n",
                "rule fee(n: num) -> num = n * 2\n",
                "fragment greet(name: str)\n",
                "  say lin \"欢迎{name}\" direction \"私有演出备注\"\n",
                "  return\n",
                "event first\n",
                "  call greet(\"码头\")\n",
                "  正文。\n",
                "  -> END\n",
            )
            .into(),
        )
        .unwrap();
    let result = project.compile_current();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let buffer = project
        .open_writing_buffer(&target("fragment", "greet"))
        .unwrap();
    let projection = project
        .project_writing_buffer(&buffer, &target("fragment", "greet"))
        .unwrap();
    assert!(projection
        .blocks
        .iter()
        .all(|block| block.kind == WritingBlockKind::Structure));
    let event = project
        .project_writing_buffer(&buffer, &target("event", "first"))
        .unwrap();
    assert_eq!(
        event
            .blocks
            .iter()
            .filter(|block| block.kind == WritingBlockKind::Prose)
            .count(),
        1
    );
    let book = br#"{"schema_version":1,"id":"novel","title":"book","entries":[{"id":"fragment","kind":"chapter","title":"greet","target_ref":{"kind":"fragment","id":"greet"}},{"id":"event","kind":"chapter","title":"first","target_ref":{"kind":"event","id":"first"}}]}"#;
    let index = build_manuscript_index(
        book,
        "book.json",
        "novel",
        &[MANUSCRIPT_REQUIRED_FEATURE.into()],
        false,
        &result,
    );
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(
        index.entries[0]
            .source
            .as_ref()
            .unwrap()
            .stats
            .unwrap()
            .han_characters,
        2
    );
    assert_eq!(
        index.entries[1]
            .source
            .as_ref()
            .unwrap()
            .stats
            .unwrap()
            .han_characters,
        2
    );
}

#[test]
fn multiline_prose_remains_one_editable_block_across_blank_lines() {
    let project = fixture();
    let target = target("scene", "first.dock");
    let mut buffer = project.open_writing_buffer(&target).unwrap();
    let projection = project.project_writing_buffer(&buffer, &target).unwrap();
    let block = projection
        .blocks
        .iter()
        .find(|block| block.kind == WritingBlockKind::Prose)
        .unwrap();
    buffer
        .replace_prose(projection.generation, block, "第一段。\n\n第二段。")
        .unwrap();
    let projection = project.project_writing_buffer(&buffer, &target).unwrap();
    let block = projection
        .blocks
        .iter()
        .find(|block| block.kind == WritingBlockKind::Prose)
        .unwrap();
    assert_eq!(block.text, "第一段。\n\n第二段。");
    buffer
        .replace_prose(projection.generation, block, "第一段。\n\n第二段继续。")
        .unwrap();
    assert!(buffer
        .source()
        .contains("    第一段。\n    \n    第二段继续。"));
    assert!(buffer.source().contains("choice \"靠岸\""));
}

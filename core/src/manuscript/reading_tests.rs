use super::{reading_projection, WritingBuffer};
use crate::{catalog::TargetRef, project::Project};

fn fixture() -> (Project, WritingBuffer) {
    let mut project = Project::new(&std::env::temp_dir().join("reading-draft-projection"));
    let path = project.entry.clone();
    project.documents.retain(|entry, _| entry == &path);
    project.set_text(&path, "event start\n  原文。\n  scene inner\n    内章。\n  -> END\nevent next\n  次章。\n  -> END\n".into()).unwrap();
    let buffer = project.open_source_writing_buffer(&path).unwrap();
    (project, buffer)
}

#[test]
fn draft_reading_projects_current_text_without_applying_or_executing() {
    let (project, mut buffer) = fixture();
    let baseline = project.content_baseline();
    buffer.replace_source(buffer.source().replace("原文。", "当前稿 {rnd(1, 10)}。"));
    let result = project
        .compile_writing_drafts(std::slice::from_ref(&buffer))
        .unwrap();
    let projection = reading_projection(&result, &TargetRef::new("event", "start")).unwrap();
    assert!(projection
        .lines
        .iter()
        .flatten()
        .any(|part| part.text.contains("当前稿")));
    assert!(projection
        .lines
        .iter()
        .flatten()
        .any(|part| part.text == "〔动态内容〕"));
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.document(&project.entry).unwrap().contains("当前稿"));
    let scene = reading_projection(&result, &TargetRef::new("scene", "start.inner")).unwrap();
    assert_eq!(scene.lines.len(), 1);
    assert_eq!(scene.lines[0][0].text, "内章。");
}

#[test]
fn invalid_stale_or_duplicate_drafts_never_fake_current_reading() {
    let (mut project, mut buffer) = fixture();
    let original = buffer.source().to_owned();
    buffer.replace_source("event start\n  if (\n".into());
    assert!(project
        .compile_writing_drafts(std::slice::from_ref(&buffer))
        .err()
        .unwrap()
        .contains("暂不能解析"));
    assert!(buffer.source().contains("if ("));
    buffer.replace_source(original.clone() + "// changed\n");
    let mut other = buffer.clone();
    other.replace_source(original + "// different\n");
    assert!(project
        .compile_writing_drafts(&[buffer.clone(), other])
        .err()
        .unwrap()
        .contains("同一文件"));
    project
        .set_text(
            &project.entry.clone(),
            "event replacement\n  外部变化\n".into(),
        )
        .unwrap();
    assert!(project
        .compile_writing_drafts(&[buffer])
        .err()
        .unwrap()
        .contains("过期"));
}

#[test]
fn same_file_shared_by_chapters_has_one_current_source() {
    let (project, mut buffer) = fixture();
    buffer.replace_source(buffer.source().replace("原文。", "一章稿。"));
    let result = project
        .compile_writing_drafts(&[buffer.clone(), buffer])
        .unwrap();
    assert_eq!(
        reading_projection(&result, &TargetRef::new("event", "next"))
            .unwrap()
            .lines[0][0]
            .text,
        "次章。"
    );
    assert!(reading_projection(&result, &TargetRef::new("event", "missing")).is_err());
}

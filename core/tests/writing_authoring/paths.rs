use super::*;
use worldline_core::draft_rehearsal::DraftRehearsalRequest;

fn assert_buffer_uses_loaded_identity(work: &Workspace, project: &Project, input: &Path) {
    let baseline = project.content_baseline();
    let disk = work.bytes();
    let mut buffer = project.open_source_writing_buffer(input).unwrap();
    assert_eq!(buffer.source(), SOURCE);
    assert_eq!(buffer.baseline(), baseline);
    assert_eq!(buffer.generation(), 0);
    let draft = SOURCE.replace("你看见林😀。", "未应用正文🌙。");
    buffer.replace_source(draft.clone());
    let before = buffer_state(&buffer);
    project.project_writing_buffer(&buffer, &start()).unwrap();
    assert_eq!(buffer.path(), project.entry);
    let request = DraftRehearsalRequest::from_writing_buffers(
        project,
        std::slice::from_ref(&buffer),
        vec![],
        false,
    )
    .unwrap();
    assert_eq!(request.drafts.len(), 1);
    assert_eq!(request.drafts[0].path, Path::new("world.wl"));
    assert_eq!(request.drafts[0].original_source, SOURCE);
    assert_eq!(request.drafts[0].source, draft);
    assert_eq!(request.drafts[0].generation, buffer.generation());
    let snapshot = project.compile_draft_rehearsal(&request).unwrap();
    assert_eq!(snapshot.compiled().sources[&project.entry], draft);
    snapshot
        .verify_navigation(project, std::slice::from_ref(&buffer), false)
        .unwrap();
    let duplicate = project.open_source_writing_buffer(&project.entry).unwrap();
    assert!(DraftRehearsalRequest::from_writing_buffers(
        project,
        &[buffer.clone(), duplicate],
        vec![],
        false,
    )
    .is_err());
    let mut aliased_request = request;
    aliased_request.drafts[0].path = "chapters/../world.wl".into();
    assert!(project.compile_draft_rehearsal(&aliased_request).is_err());
    assert_eq!(buffer_state(&buffer), before);
    assert_eq!(project.document(&project.entry).unwrap(), SOURCE);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(work.bytes(), disk);
}

#[test]
fn source_writing_buffer_normalizes_relative_parent_path_for_projection_and_rehearsal() {
    let (work, project) = Workspace::new(
        &[
            ("world.wl", SOURCE),
            ("chapters/notes.wl", "// 保留资料原稿\n"),
        ],
        None,
    );
    assert_buffer_uses_loaded_identity(&work, &project, Path::new("./chapters/../world.wl"));
}

#[cfg(unix)]
#[test]
fn source_writing_buffer_normalizes_parent_directory_alias_without_duplicate_draft_identity() {
    use std::os::unix::fs::symlink;
    struct DirectoryAlias(PathBuf);
    impl Drop for DirectoryAlias {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let (work, project) = Workspace::new(&[("world.wl", SOURCE)], None);
    // 别名位于工作区外，只模拟宿主根目录拼写；不在作品中加入可遍历链接。
    let alias = DirectoryAlias(work.0.with_extension("directory-alias"));
    symlink(&work.0, &alias.0).unwrap();
    let reopened = Project::open(&alias.0).unwrap();
    assert_eq!(reopened.root, project.root);
    assert_eq!(reopened.entry, project.entry);
    let input = alias.0.join("world.wl");
    assert_ne!(input, project.entry);
    assert_buffer_uses_loaded_identity(&work, &project, &input);
}

#[test]
fn source_writing_buffer_rejects_outside_paths_without_changing_either_workspace() {
    let (work, project) = Workspace::new(&[("world.wl", SOURCE)], None);
    let (outside, other) = Workspace::new(&[("world.wl", "event other\n  -> END\n")], None);
    let baseline = project.content_baseline();
    let other_baseline = other.content_baseline();
    let disk = work.bytes();
    let other_disk = outside.bytes();
    for path in [
        other.entry.clone(),
        Path::new("..")
            .join(other.root.file_name().unwrap())
            .join("world.wl"),
    ] {
        assert!(project.open_source_writing_buffer(&path).is_err());
    }
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(other.content_baseline(), other_baseline);
    assert_eq!(work.bytes(), disk);
    assert_eq!(outside.bytes(), other_disk);
}

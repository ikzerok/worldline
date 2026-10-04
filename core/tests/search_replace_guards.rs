use std::{
    fs,
    path::{Path, PathBuf},
};
use worldline_core::{project::Project, search_replace::*};

fn fixture(name: &str) -> (PathBuf, Project, SearchRequest) {
    let root = std::env::temp_dir().join(format!("replace-guards-{name}-{}", std::process::id()));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(
        root.join("world.wl"),
        "event start\n  needle 第一 needle 第二 needle\n  -> END\n",
    )
    .unwrap();
    fs::write(root.join("second.wl"), "event next\n  needle\n  -> END\n").unwrap();
    let project = Project::open(&root).unwrap();
    let request = SearchRequest {
        query: "needle".into(),
        replacement: "changed".into(),
        options: SearchOptions::default(),
        scope: SearchScope::Prose,
        files: vec![SearchFile {
            path: "world.wl".into(),
            range: None,
        }],
    };
    (root, project, request)
}

#[test]
fn selected_captured_range_and_empty_replacement_use_true_zero_width_after_range() {
    let (root, project, mut request) = fixture("captured");
    let buffer = project
        .open_source_writing_buffer(Path::new("world.wl"))
        .unwrap();
    let all = project.search_drafts(&request, &[]).unwrap();
    request.files[0].range = Some(all[1].range.clone());
    request.replacement.clear();
    let hits = project.search_drafts(&request, &[]).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].range, all[1].range);
    let plan = project
        .preview_search_replace_selected(&request, &[], &hits)
        .unwrap();
    let next = project.replace_search_draft(&plan, &[], &buffer).unwrap();
    assert!(next.source().contains("needle 第一  第二 needle"));
    assert_eq!(plan.occurrences[0].after_range.start, all[1].range.start);
    assert!(plan.occurrences[0].after_range.is_empty());
    assert!(plan.occurrences[0].after_context.highlight.is_empty());
    assert_eq!(
        project.document(&root.join("world.wl")).unwrap(),
        buffer.source()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn selected_drafts_deduplicate_equal_copies_but_reject_conflicting_and_stale_sources() {
    let (root, mut project, request) = fixture("drafts");
    let mut first = project
        .open_source_writing_buffer(Path::new("world.wl"))
        .unwrap();
    first.replace_source(first.source().replace("第一", "草稿"));
    let hits = project
        .search_drafts(&request, std::slice::from_ref(&first))
        .unwrap();
    let duplicates = [first.clone(), first.clone()];
    assert!(project
        .preview_search_replace_selected(&request, &duplicates, &hits[..1])
        .is_ok());
    let mut conflicting = first.clone();
    conflicting.replace_source(conflicting.source().replace("草稿", "冲突"));
    assert!(project
        .search_drafts(&request, &[first.clone(), conflicting])
        .is_err());
    project
        .set_text(
            &root.join("second.wl"),
            "event next\n  已改变\n  -> END\n".into(),
        )
        .unwrap();
    let current = project
        .search_drafts(&request, std::slice::from_ref(&first))
        .unwrap();
    assert_eq!(current.len(), 3);
    let before = first.source().to_owned();
    assert!(project
        .preview_search_replace_selected(&request, std::slice::from_ref(&first), &current[..1])
        .is_err());
    assert_eq!(first.source(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unknown_workspace_capability_allows_selected_find_but_refuses_preview() {
    let (root, _, request) = fixture("unknown");
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"required_features":["future.unknown"]}"#,
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    let hits = project.search_drafts(&request, &[]).unwrap();
    assert_eq!(hits.len(), 3);
    assert!(project
        .preview_search_replace_selected(&request, &[], &hits[..1])
        .is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_all_hit_and_zero_hit_plans_keep_their_existing_semantics() {
    let (root, mut project, mut request) = fixture("legacy");
    let all = project.preview_search_replace(&request, &[]).unwrap();
    assert_eq!(all.hits.len(), 3);
    assert_eq!(all.changes[0].count, 3);
    assert_eq!(all.occurrences.len(), 3);
    project.apply_search_replace(&all, &[]).unwrap();
    request.query = "absentneedle".into();
    let empty = project.preview_search_replace(&request, &[]).unwrap();
    assert!(empty.hits.is_empty());
    assert!(empty.changes.is_empty());
    let baseline = project.content_baseline();
    project.apply_search_replace(&empty, &[]).unwrap();
    assert_eq!(project.content_baseline(), baseline);
    assert!(project
        .preview_search_replace_selected(&request, &[], &[])
        .is_err());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn read_only_selected_file_rejects_whole_multi_file_commit_without_mutation() {
    use std::os::unix::fs::PermissionsExt;
    let (root, mut project, mut request) = fixture("permissions");
    request.files.push(SearchFile {
        path: "second.wl".into(),
        range: None,
    });
    let hits = project.search_drafts(&request, &[]).unwrap();
    let plan = project
        .preview_search_replace_selected(&request, &[], &[hits[0].clone(), hits[3].clone()])
        .unwrap();
    let path = root.join("second.wl");
    let permissions = fs::metadata(&path).unwrap().permissions();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    let baseline = project.content_baseline();
    assert!(project.apply_search_replace(&plan, &[]).is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project
        .document(&root.join("world.wl"))
        .unwrap()
        .contains("changed"));
    fs::set_permissions(&path, permissions).unwrap();
    fs::remove_dir_all(root).unwrap();
}

use std::{fs, path::PathBuf};
use worldline_core::{project::Project, search_replace::*};
fn fixture(name: &str) -> (PathBuf, Project) {
    let root = std::env::temp_dir().join(format!("replace-{name}-{}", std::process::id()));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#,
    )
    .unwrap();
    fs::write(root.join("world.wl"),"character hero as \"英雄\"\nlet count = 1\nevent start\n  Hello 世界 Hello {count} [[character:hero|Hello]] #line:hello\n  say hero \"Hello {count}\" direction \"Hello private\"\n  -> END\n").unwrap();
    fs::write(
        root.join("second.wl"),
        "event next\n  Hello again\n  -> END\n",
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    (root, project)
}
fn request() -> SearchRequest {
    SearchRequest {
        query: "Hello".into(),
        replacement: "Goodbye".into(),
        options: SearchOptions::default(),
        scope: SearchScope::Prose,
        files: vec![
            SearchFile {
                path: "world.wl".into(),
                range: None,
            },
            SearchFile {
                path: "second.wl".into(),
                range: None,
            },
        ],
    }
}
#[test]
fn draft_overlay_deduplicates_and_protects_tokens() {
    let (_, project) = fixture("draft");
    let mut draft = project
        .open_source_writing_buffer(PathBuf::from("world.wl").as_path())
        .unwrap();
    draft.replace_source(draft.source().replace("Hello 世界", "freshneedle 世界"));
    let mut req = request();
    req.query = "freshneedle".into();
    assert_eq!(
        project
            .search_drafts(&req, &[draft.clone(), draft.clone()])
            .unwrap()
            .len(),
        1
    );
    let mut conflicting = draft.clone();
    conflicting.replace_source("event conflict\n  freshneedle\n".into());
    assert!(project
        .search_drafts(&req, &[draft.clone(), conflicting])
        .is_err());
    let plan = project
        .preview_search_replace(&request(), &[draft])
        .unwrap();
    assert_eq!(plan.hits.len(), 3);
    let all = plan
        .changes
        .iter()
        .map(|c| c.after.as_str())
        .collect::<String>();
    assert!(all.contains("[[character:hero|Hello]]"));
    assert!(all.contains("#line:hello"));
    assert!(all.contains("direction \"Hello private\""));
    assert!(all.contains("{count}"));
}
#[test]
fn atomic_multi_file_stale_disk_cancel_and_undo_without_save() {
    let (root, mut project) = fixture("atomic");
    let before = project.clone();
    let disk = fs::read(root.join("world.wl")).unwrap();
    let plan = project.preview_search_replace(&request(), &[]).unwrap();
    assert_eq!(project.content_baseline(), before.content_baseline());
    project.apply_search_replace(&plan, &[]).unwrap();
    assert!(project
        .document(&root.join("second.wl"))
        .unwrap()
        .contains("Goodbye"));
    assert_eq!(fs::read(root.join("world.wl")).unwrap(), disk);
    let changed = project.clone();
    assert!(project.restore(before));
    assert!(project.restore(changed));
    assert!(project.apply_search_replace(&plan, &[]).is_err());
    let plan = project
        .preview_search_replace(
            &SearchRequest {
                query: "Goodbye".into(),
                replacement: "Next".into(),
                ..request()
            },
            &[],
        )
        .unwrap();
    let baseline = project.content_baseline();
    fs::write(root.join("second.wl"), "event external\n  外部\n").unwrap();
    assert!(project.apply_search_replace(&plan, &[]).is_err());
    assert_eq!(project.content_baseline(), baseline);
}
#[test]
fn unicode_selection_empty_and_protected_source_rejection() {
    assert_eq!(
        literal_matches(
            "Été été 猫咪猫 咪",
            "été",
            SearchOptions {
                case_sensitive: false,
                whole_word: true
            }
        ),
        vec![0..5, 6..11]
    );
    assert!(literal_matches(
        "猫咪猫",
        "咪",
        SearchOptions {
            case_sensitive: true,
            whole_word: true
        }
    )
    .is_empty());
    assert!(literal_matches("abc", "", SearchOptions::default()).is_empty());
    let (_, project) = fixture("tokens");
    let mut req = request();
    req.scope = SearchScope::Source;
    req.query = "hero".into();
    assert!(!project.search_drafts(&req, &[]).unwrap().is_empty());
    assert!(project.preview_search_replace(&req, &[]).is_err());
    req = request();
    req.files[0].range = Some(0..2);
    req.files.truncate(1);
    assert!(project.search_drafts(&req, &[]).unwrap().is_empty());
    req = request();
    req.replacement = "{evil}".into();
    assert!(project.preview_search_replace(&req, &[]).is_err());
}
#[test]
fn current_draft_replace_and_stale_generation_do_not_apply_project() {
    let (_, project) = fixture("local");
    let mut buffer = project
        .open_source_writing_buffer(PathBuf::from("world.wl").as_path())
        .unwrap();
    let mut req = request();
    req.files.truncate(1);
    let baseline = project.content_baseline();
    let plan = project
        .preview_search_replace(&req, std::slice::from_ref(&buffer))
        .unwrap();
    let next = project
        .replace_search_draft(&plan, std::slice::from_ref(&buffer), &buffer)
        .unwrap();
    assert!(next.source().contains("Goodbye"));
    assert_eq!(project.content_baseline(), baseline);
    buffer.replace_source(format!("{}\n", buffer.source()));
    assert!(project
        .replace_search_draft(&plan, std::slice::from_ref(&buffer), &buffer)
        .is_err());
}
#[test]
fn unknown_capability_read_only_allows_find_but_never_replace() {
    let (root, _) = fixture("readonly");
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"required_features":["future.unknown"]}"#,
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    assert!(!project.search_drafts(&request(), &[]).unwrap().is_empty());
    assert!(project.preview_search_replace(&request(), &[]).is_err());
}
#[test]
fn clean_old_buffer_never_hides_new_project_and_invalid_draft_remains_searchable() {
    let (_, mut project) = fixture("clean");
    let path = project.root.join("world.wl");
    let buffer = project.open_source_writing_buffer(&path).unwrap();
    project
        .set_text(&path, "event start\n  latestneedle\n".into())
        .unwrap();
    let mut req = request();
    req.query = "latestneedle".into();
    req.replacement = "updated".into();
    assert_eq!(
        project
            .search_drafts(&req, std::slice::from_ref(&buffer))
            .unwrap()
            .len(),
        1
    );
    assert!(project.preview_search_replace(&req, &[buffer]).is_ok());
    let mut broken = project.open_source_writing_buffer(&path).unwrap();
    broken.replace_source("event start\n  latestneedle {unfinished\n".into());
    req.scope = SearchScope::Source;
    assert_eq!(
        project
            .search_drafts(&req, std::slice::from_ref(&broken))
            .unwrap()
            .len(),
        1
    );
    assert!(project.preview_search_replace(&req, &[broken]).is_err());
}
#[test]
fn disabled_reason_is_static_replaceable_text_with_literal_braces_and_links() {
    let (root, _) = fixture("disabled");
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.12","required_features":[]}"#,
    )
    .unwrap();
    fs::write(root.join("world.wl"),"event start\n  choice \"继续\" enable false disabled \"缺少 {freshneedle} [[entity:freshneedle|文字]] \\\"引号\\\"\"\n    -> END\n").unwrap();
    let mut project = Project::open(&root).unwrap();
    let mut req = request();
    req.files.truncate(1);
    req.query = "freshneedle".into();
    req.replacement = "{静态替换}".into();
    let hits = project.search_drafts(&req, &[]).unwrap();
    assert_eq!(hits.len(), 2);
    let plan = project.preview_search_replace(&req, &[]).unwrap();
    project.apply_search_replace(&plan, &[]).unwrap();
    let result = project.compile();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let source = project.document(&root.join("world.wl")).unwrap();
    assert!(source.contains("{{静态替换}}"));
    assert!(source.contains("[[entity:{静态替换}|文字]]"));
    assert!(source.contains("\\\"引号\\\""));
}

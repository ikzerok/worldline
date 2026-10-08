use super::tests::{chapter, fixture, request};
use super::*;
use crate::project::Project;
use serde_json::json;

fn on_disk() -> Project {
    let mut project = fixture(vec![chapter("one", None), chapter("two", None)]);
    project
        .documents
        .get_mut(&project.entry)
        .unwrap()
        .text
        .insert_str(0, "asset art image \"art.png\"\n");
    for (path, document) in &project.documents {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &document.text).unwrap();
    }
    for (path, document) in &project.authoring_documents {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, document.bytes()).unwrap();
    }
    std::fs::write(project.root.join("art.png"), b"fixture image bytes").unwrap();
    project.mark_saved();
    project.refresh().unwrap();
    project
}

#[test]
fn manuscript_query_metadata_refresh_invalidates_cache_without_breaking_noop_or_undo() {
    let mut project = on_disk();
    let baseline = project.content_baseline();
    let undo_generation = project.search_refresh_generation();
    let independent_previous = Project::open_read_only(&project.root).unwrap();
    assert_eq!(independent_previous.content_baseline(), baseline);
    assert_eq!(
        independent_previous.language_version(),
        project.language_version()
    );
    assert_eq!(
        independent_previous.source_selection(),
        project.source_selection()
    );
    assert_eq!(
        serde_json::to_value(independent_previous.authoring_diagnostics()).unwrap(),
        serde_json::to_value(project.authoring_diagnostics()).unwrap(),
    );
    let readonly = |project: &Project| {
        project
            .authoring_documents
            .iter()
            .map(|(path, document)| (path.clone(), document.is_read_only()))
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(readonly(&independent_previous), readonly(&project));
    let staged_candidate = project.clone();
    let before_key = project.manuscript_query_key(&[], &[]);
    let before = project.manuscript_query_snapshot(&[], &[]).unwrap();
    let mut query = request();
    query.limit = 1;
    query.cursor = before.query(&query).unwrap().next_cursor;
    std::fs::remove_file(project.root.join("art.png")).unwrap();
    assert_eq!(
        project.manuscript_query_key(&[], &[]),
        before_key,
        "纯依赖key无每帧IO"
    );
    let rebuilt_without_refresh = project.manuscript_query_snapshot(&[], &[]).unwrap();
    assert_ne!(
        before.key(),
        rebuilt_without_refresh.key(),
        "真实内容key绑定本次附件诊断"
    );
    assert_eq!(
        rebuilt_without_refresh.query(&query).unwrap_err().code,
        "STALE_CURSOR"
    );
    assert!(project.refresh().unwrap().is_empty());
    let changed_key = project.manuscript_query_key(&[], &[]);
    assert_ne!(before_key, changed_key);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.search_refresh_generation(), undo_generation);
    assert_eq!(
        staged_candidate.manuscript_observation_key(),
        project.manuscript_observation_key(),
        "旧候选共享最新观测"
    );
    let current_observation = project.manuscript_observation_key();
    assert!(
        project.restore(independent_previous),
        "仅附件变化不能使正文撤销失效"
    );
    assert_eq!(
        project.manuscript_observation_key(),
        current_observation,
        "restore不能恢复独立旧Project的附件观测"
    );
    assert_eq!(project.manuscript_query_key(&[], &[]), changed_key);
    let changed = project.manuscript_query_snapshot(&[], &[]).unwrap();
    assert!(changed
        .query(&request())
        .unwrap()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "A215"));
    project.refresh().unwrap();
    assert_eq!(
        project.manuscript_query_key(&[], &[]),
        changed_key,
        "真正无变化refresh保持cache key"
    );
    assert!(project.restore(staged_candidate));
    assert_eq!(project.manuscript_observation_key(), current_observation);
    std::fs::remove_dir_all(&project.root).unwrap();
}

#[test]
fn manuscript_query_key_keeps_equivalent_tracked_path_spelling() {
    let project = fixture(vec![chapter("one", None)]);
    let mut equivalent = project.clone();
    let path = project.root.join(".world/manuscripts/novel.json");
    let alias = project.root.join(".world//manuscripts/./novel.json");
    assert_eq!(path, alias);
    assert_ne!(
        path.as_os_str().as_encoded_bytes(),
        alias.as_os_str().as_encoded_bytes()
    );
    let document = equivalent.authoring_documents.remove(&path).unwrap();
    equivalent.authoring_documents.insert(alias, document);
    assert_eq!(
        equivalent.manuscript_query_key(&[], &[]),
        project.manuscript_query_key(&[], &[]),
    );
}

#[test]
fn manuscript_query_refresh_failure_invalidates_trusted_page_and_recovery_is_explicit() {
    let mut project = on_disk();
    let original_key = project.manuscript_query_key(&[], &[]);
    let baseline = project.content_baseline();
    let bad = project.root.join("unreadable.wl");
    std::fs::write(&bad, [0xff, 0xfe]).unwrap();
    assert!(project.refresh().is_err());
    assert_ne!(project.manuscript_query_key(&[], &[]), original_key);
    assert_eq!(project.content_baseline(), baseline);
    let failed = project
        .manuscript_query_snapshot(&[], &[])
        .unwrap()
        .query(&request())
        .unwrap();
    assert!(!failed.complete);
    assert!(
        failed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "MAN004"
                && diagnostic.message.contains("观测未完成"))
    );
    std::fs::remove_file(bad).unwrap();
    project.refresh().unwrap();
    assert!(
        project
            .manuscript_query_snapshot(&[], &[])
            .unwrap()
            .query(&request())
            .unwrap()
            .complete
    );
    std::fs::remove_dir_all(&project.root).unwrap();
}

#[test]
fn manuscript_query_typed_draft_cannot_hide_distinct_bad_kind_original_entry() {
    let project = fixture(vec![
        chapter("one", None),
        json!({"id":"bad","kind":"future_kind","title":"不可识别原项","future":{"keep":true}}),
    ]);
    let before = project.snapshot_state().unwrap();
    let snapshot = project.manuscript_query_snapshot(&[], &[]).unwrap();
    assert!(!snapshot.query(&request()).unwrap().complete);
    let input = ManuscriptQueryDraft {
        expected_baseline: project.content_baseline(),
        draft: ManuscriptDraft::from_index(&snapshot.indices()["novel"]),
    };
    assert_eq!(input.draft.entries.len(), 1);
    assert_eq!(
        project
            .manuscript_query_snapshot(&[], &[input])
            .unwrap_err()
            .code,
        "INVALID_DRAFT"
    );
    assert_eq!(project.snapshot_state().unwrap(), before);
}

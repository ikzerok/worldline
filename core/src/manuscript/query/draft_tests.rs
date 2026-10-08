use super::tests::{chapter, fixture, request};
use super::*;
use crate::manuscript::{ManuscriptReferenceStatus, WritingBuffer};
use crate::project::Project;
use serde_json::json;

fn writing(project: &Project) -> WritingBuffer {
    project.open_source_writing_buffer(&project.entry).unwrap()
}
fn draft(project: &Project) -> ManuscriptQueryDraft {
    let snapshot = project.manuscript_query_snapshot(&[], &[]).unwrap();
    ManuscriptQueryDraft {
        expected_baseline: project.content_baseline(),
        draft: ManuscriptDraft::from_index(&snapshot.indices()["novel"]),
    }
}

#[test]
fn manuscript_query_writing_change_cancel_and_invalid_draft_never_borrow_applied_stats() {
    let project = fixture(vec![chapter("one", None)]);
    let baseline = project.content_baseline();
    let mut buffer = writing(&project);
    let original = buffer.source().to_owned();
    let applied = project.manuscript_query_snapshot(&[], &[]).unwrap();
    let applied_page = applied.query(&request()).unwrap();
    assert_eq!(applied_page.source, ManuscriptQuerySource::Applied);
    buffer.replace_source(original.replace("hello world", "hello world new words"));
    let current = project
        .manuscript_query_snapshot(&[buffer.clone()], &[])
        .unwrap();
    let current_page = current.query(&request()).unwrap();
    assert_eq!(current_page.source, ManuscriptQuerySource::Draft);
    assert_eq!(
        current_page.rows[0]
            .entry
            .source
            .as_ref()
            .unwrap()
            .stats
            .unwrap()
            .words,
        4
    );
    assert_eq!(
        applied_page.rows[0]
            .entry
            .source
            .as_ref()
            .unwrap()
            .stats
            .unwrap()
            .words,
        2
    );
    buffer.replace_source("event start\n  set missing =\n".into());
    let invalid = project
        .manuscript_query_snapshot(&[buffer.clone()], &[])
        .unwrap();
    let invalid_page = invalid.query(&request()).unwrap();
    assert!(!invalid_page.complete);
    assert_eq!(invalid_page.recognized_chapters, 1);
    assert_eq!(invalid_page.source, ManuscriptQuerySource::Draft);
    let source = invalid_page.rows[0].entry.source.as_ref().unwrap();
    assert_eq!(source.status, ManuscriptReferenceStatus::Unresolved);
    assert!(source.stats.is_none() && source.location.is_none());
    assert!(invalid_page.rows[0].perspective_display.is_none());
    assert!(invalid_page
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == crate::Severity::Error));
    buffer.replace_source(original);
    let cancelled = project
        .manuscript_query_snapshot(&[buffer.clone()], &[])
        .unwrap();
    let cancelled_page = cancelled.query(&request()).unwrap();
    assert_eq!(cancelled_page.source, ManuscriptQuerySource::Applied);
    assert!(cancelled_page.complete);
    assert_ne!(cancelled.key(), invalid.key());
    assert_ne!(cancelled.key(), applied.key()); // 取消仍保留真实编辑代次。
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.is_dirty());
}

#[test]
fn manuscript_query_stale_or_conflicting_writing_drafts_fail_without_fallback() {
    let mut project = fixture(vec![chapter("one", None)]);
    let mut first = writing(&project);
    first.replace_source(first.source().replace("hello world", "first draft"));
    let mut second = writing(&project);
    second.replace_source(second.source().replace("hello world", "second draft"));
    assert_eq!(
        project
            .manuscript_query_snapshot(&[first.clone(), second], &[])
            .unwrap_err()
            .code,
        "CONFLICTING_DRAFT"
    );
    assert!(project
        .manuscript_query_snapshot(&[first.clone(), first.clone()], &[])
        .is_ok());
    project
        .documents
        .get_mut(&project.entry)
        .unwrap()
        .text
        .push_str("// current\n");
    let baseline = project.content_baseline();
    assert_eq!(
        project
            .manuscript_query_snapshot(&[first.clone()], &[])
            .unwrap_err()
            .code,
        "STALE_DRAFT"
    );
    assert_eq!(project.content_baseline(), baseline);
    assert!(first.source().contains("first draft"));
}

#[test]
fn manuscript_query_unchanged_stale_writing_buffer_does_not_override_new_project() {
    let mut project = fixture(vec![chapter("one", None)]);
    let unchanged = writing(&project);
    project.documents.get_mut(&project.entry).unwrap().text = unchanged
        .source()
        .replace("hello world", "one two three four five");
    let snapshot = project
        .manuscript_query_snapshot(&[unchanged], &[])
        .unwrap();
    let page = snapshot.query(&request()).unwrap();
    assert_eq!(
        page.rows[0]
            .entry
            .source
            .as_ref()
            .unwrap()
            .stats
            .unwrap()
            .words,
        5
    );
    assert_eq!(page.source, ManuscriptQuerySource::Applied);
}

#[test]
fn manuscript_query_explicit_arrangement_draft_preserves_unknown_fields_and_original_index() {
    let project = fixture(vec![chapter("one", None)]);
    let mut input = draft(&project);
    input.draft.entries[0].title = "新标题".into();
    input.draft.entries[0].summary = Some("新的摘要".into());
    let baseline = project.content_baseline();
    let snapshot = project
        .manuscript_query_snapshot(&[], &[input.clone()])
        .unwrap();
    let page = snapshot.query(&request()).unwrap();
    assert_eq!(page.source, ManuscriptQuerySource::Draft);
    assert_eq!(page.rows[0].entry.title, "新标题");
    assert_eq!(
        snapshot.applied_indices()["novel"].entries[0].title,
        "同一标题"
    );
    assert_eq!(
        snapshot.indices()["novel"].source_document().unwrap()["future"],
        json!({"keep":true})
    );
    let serialized = serde_json::to_value(&input).unwrap();
    assert_eq!(
        serde_json::from_value::<ManuscriptQueryDraft>(serialized).unwrap(),
        input
    );
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.is_dirty());
    let cancelled = project.manuscript_query_snapshot(&[], &[]).unwrap();
    assert_ne!(snapshot.key(), cancelled.key());
    assert_eq!(
        cancelled.query(&request()).unwrap().rows[0].entry.title,
        "同一标题"
    );
}

#[test]
fn manuscript_query_arrangement_stale_duplicate_absent_readonly_and_bad_base_are_explicit_errors() {
    let mut project = fixture(vec![chapter("one", None)]);
    let input = draft(&project);
    assert_eq!(
        project
            .manuscript_query_snapshot(&[], &[input.clone(), input.clone()])
            .unwrap_err()
            .code,
        "DUPLICATE_DRAFT"
    );
    let mut missing = input.clone();
    missing.draft.id = "absent".into();
    assert_eq!(
        project
            .manuscript_query_snapshot(&[], &[missing])
            .unwrap_err()
            .code,
        "MANUSCRIPT_NOT_FOUND"
    );
    let mut stale = input.clone();
    stale.expected_baseline.push('x');
    assert_eq!(
        project
            .manuscript_query_snapshot(&[], &[stale])
            .unwrap_err()
            .code,
        "STALE_DRAFT"
    );
    let path = project.root.join(".world/manuscripts/novel.json");
    project
        .authoring_documents
        .get_mut(&path)
        .unwrap()
        .read_only = true;
    assert_eq!(
        project
            .manuscript_query_snapshot(&[], &[input])
            .unwrap_err()
            .code,
        "READ_ONLY_MANUSCRIPT"
    );
    let project = fixture(vec![chapter("same", None), chapter("same", None)]);
    let input = draft(&project);
    assert_eq!(
        project
            .manuscript_query_snapshot(&[], &[input])
            .unwrap_err()
            .code,
        "INVALID_DRAFT"
    );
}

#[test]
fn manuscript_query_key_covers_full_buffers_unknown_bytes_capabilities_and_readonly() {
    let project = fixture(vec![chapter("one", None)]);
    let base = project.manuscript_query_key(&[], &[]);
    let mut changed = project.clone();
    changed.root.push("elsewhere");
    assert_ne!(changed.manuscript_query_key(&[], &[]), base);
    changed = project.clone();
    changed.entry.set_file_name("other.wl");
    assert_ne!(changed.manuscript_query_key(&[], &[]), base);
    changed = project.clone();
    changed
        .documents
        .get_mut(&project.entry)
        .unwrap()
        .text
        .push_str("// metadata\n");
    assert_ne!(changed.manuscript_query_key(&[], &[]), base);
    let path = project.root.join(".world/manuscripts/novel.json");
    changed = project.clone();
    changed
        .authoring_documents
        .get_mut(&path)
        .unwrap()
        .read_only = true;
    assert_ne!(changed.manuscript_query_key(&[], &[]), base);
    changed = project.clone();
    changed.authoring_documents.get_mut(&path).unwrap().deleted = true;
    assert_ne!(changed.manuscript_query_key(&[], &[]), base);
    changed = project.clone();
    changed
        .authoring_documents
        .get_mut(&path)
        .unwrap()
        .bytes
        .push(b' ');
    assert_ne!(changed.manuscript_query_key(&[], &[]), base);
    let manifest = project.root.join(".world/project.json");
    for extra in [
        json!({"unknown_optional":1}),
        json!({"language_version":"1.13"}),
        json!({"required_features":["presentation.manuscripts.v1","unknown.required"]}),
        json!({"source_config":{"mode":"explicit","active":["world.wl"],"archived":[]}}),
    ] {
        changed = project.clone();
        let document = changed.authoring_documents.get_mut(&manifest).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&document.bytes).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        document.bytes = serde_json::to_vec(&value).unwrap();
        assert_ne!(changed.manuscript_query_key(&[], &[]), base);
    }
    let mut buffer = writing(&project);
    let original_key = project.manuscript_query_key(&[buffer.clone()], &[]);
    assert_ne!(original_key, base);
    buffer.replace_source(buffer.source().replace("hello", "goodbye"));
    let changed_key = project.manuscript_query_key(&[buffer.clone()], &[]);
    assert_ne!(original_key, changed_key);
    let input = draft(&project);
    let draft_key = project.manuscript_query_key(&[], std::slice::from_ref(&input));
    assert_ne!(draft_key, base);
    let mut newer = input.clone();
    newer.draft.entries[0].goal = Some("different".into());
    assert_ne!(project.manuscript_query_key(&[], &[newer]), draft_key);
    let mut newer = input;
    newer.expected_baseline.push('x');
    assert_ne!(project.manuscript_query_key(&[], &[newer]), draft_key);
}

#[test]
fn manuscript_query_key_refs_ignore_input_iteration_order_but_keep_duplicates() {
    let project = fixture(vec![chapter("one", None)]);
    let mut first = writing(&project);
    first.replace_source(first.source().replace("hello", "first"));
    let mut second = writing(&project);
    second.replace_source(second.source().replace("hello", "second"));
    let input = draft(&project);
    let mut other = input.clone();
    other.draft.title = "不同书名".into();
    let a = project.manuscript_query_key(
        &[first.clone(), second.clone()],
        &[input.clone(), other.clone()],
    );
    let b = project.manuscript_query_key_refs([&second, &first], &[other, input]);
    assert_eq!(a, b);
    assert_ne!(
        project.manuscript_query_key_refs([&first, &first], &[]),
        project.manuscript_query_key_refs([&first], &[])
    );
    let snapshot = project
        .manuscript_query_snapshot(&[first.clone()], &[])
        .unwrap();
    assert_eq!(
        snapshot.key(),
        project
            .manuscript_query_snapshot(&[first], &[])
            .unwrap()
            .key()
    );
    assert_ne!(
        snapshot.key(),
        project.manuscript_query_key(&[], &[]),
        "缓存依赖key和真实快照内容key分别绑定不同语义"
    );
}

#[test]
fn manuscript_query_creation_compiles_once_and_paging_never_compiles_or_mutates() {
    let project = fixture(vec![chapter("one", None), chapter("two", None)]);
    let input = draft(&project);
    let baseline = project.content_baseline();
    let state = project.snapshot_state().unwrap();
    let runs = crate::problems::COMPILE_RUNS.with(|count| count.get());
    let snapshot = project.manuscript_query_snapshot(&[], &[input]).unwrap();
    assert_eq!(
        crate::problems::COMPILE_RUNS.with(|count| count.get()),
        runs + 1
    );
    let mut query = request();
    query.limit = 1;
    for _ in 0..20 {
        query.cursor = snapshot.query(&query).unwrap().next_cursor;
    }
    assert_eq!(
        crate::problems::COMPILE_RUNS.with(|count| count.get()),
        runs + 1
    );
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.snapshot_state().unwrap(), state);
    assert!(!project.is_dirty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn manuscript_query_frozen_pages_ignore_disk_changes_and_absent_include_never_falls_back() {
    let mut project = fixture(vec![chapter("one", None)]);
    std::fs::create_dir_all(&project.root).unwrap();
    let disk_only = project.root.join("disk_only.wl");
    std::fs::write(
        &disk_only,
        "event secret\n  disk words must stay absent\n  -> END\n",
    )
    .unwrap();
    project
        .documents
        .get_mut(&project.entry)
        .unwrap()
        .text
        .insert_str(0, "include \"disk_only.wl\"\n");
    let key = project.manuscript_query_key(&[], &[]);
    let snapshot = project.manuscript_query_snapshot(&[], &[]).unwrap();
    let content_key = snapshot.key().to_owned();
    let page = snapshot.query(&request()).unwrap();
    assert!(!page.complete);
    assert!(page
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "A105"));
    let frozen = serde_json::to_vec(&page).unwrap();
    std::fs::remove_dir_all(&project.root).unwrap();
    assert_eq!(snapshot.key(), content_key);
    assert_eq!(project.manuscript_query_key(&[], &[]), key);
    assert_eq!(
        serde_json::to_vec(&snapshot.query(&request()).unwrap()).unwrap(),
        frozen
    );
    assert!(!project.documents.contains_key(&disk_only));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn manuscript_query_refresh_generation_invalidates_same_local_content_and_cursor() {
    let mut project = fixture(vec![chapter("one", None), chapter("two", None)]);
    for (path, document) in &project.documents {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &document.text).unwrap();
    }
    for (path, document) in &project.authoring_documents {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, document.bytes()).unwrap();
    }
    project
        .documents
        .get_mut(&project.entry)
        .unwrap()
        .text
        .push_str("// local draft\n");
    let baseline = project.content_baseline();
    let snapshot = project.manuscript_query_snapshot(&[], &[]).unwrap();
    let mut query = request();
    query.limit = 1;
    query.cursor = snapshot.query(&query).unwrap().next_cursor;
    std::fs::write(&project.entry, "event start\n  external words\n  -> END\n").unwrap();
    assert!(!project.refresh().unwrap().is_empty());
    assert_eq!(project.content_baseline(), baseline);
    let newer = project.manuscript_query_snapshot(&[], &[]).unwrap();
    assert_ne!(snapshot.key(), newer.key());
    assert_eq!(newer.query(&query).unwrap_err().code, "STALE_CURSOR");
    std::fs::remove_dir_all(&project.root).unwrap();
}

use super::*;
use crate::manuscript::ManuscriptReferenceStatus;
use crate::project::Project;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);

pub(super) fn root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "manuscript-query-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
pub(super) fn chapter(id: &str, parent: Option<&str>) -> Value {
    json!({"id":id,"kind":"chapter","parent_id":parent,"title":"同一标题",
        "target_ref":{"kind":"event","id":"start"},"summary":"海港 灯塔", "goal":"归乡",
        "pov":{"kind":"character","id":"lin"},"status":"draft"})
}
pub(super) fn fixture(entries: Vec<Value>) -> Project {
    let files = BTreeMap::from([
        (PathBuf::from("world.wl"), b"character lin as \"Lin\"\ncharacter another as \"Lin\"\nevent start\n  hello world\n  -> END\n".to_vec()),
        (PathBuf::from(".world/project.json"), serde_json::to_vec(&json!({
            "schema_version":1,"required_features":["presentation.manuscripts.v1"],
            "manuscripts":{"novel":".world/manuscripts/novel.json"}})).unwrap()),
        (PathBuf::from(".world/manuscripts/novel.json"), serde_json::to_vec(&json!({
            "schema_version":1,"id":"novel","title":"书", "future":{"keep":true},"entries":entries})).unwrap()),
    ]);
    Project::from_snapshot(&root(), Path::new("world.wl"), &files).unwrap()
}
pub(super) fn request() -> ManuscriptQueryRequest {
    ManuscriptQueryRequest {
        manuscript_id: "novel".into(),
        ..Default::default()
    }
}

#[test]
fn manuscript_query_thousand_chapters_filter_before_paging_and_clamp() {
    let mut entries = vec![json!({"id":"volume","kind":"section","title":"卷"})];
    for index in 0..1000 {
        let mut entry = chapter(&format!("chapter_{index}"), Some("volume"));
        if index == 999 {
            entry["status"] = json!("Review");
            entry["goal"] = json!("终点灯塔");
        }
        entries.push(entry);
    }
    let snapshot = fixture(entries)
        .manuscript_query_snapshot(&[], &[])
        .unwrap();
    let first = snapshot.query(&request()).unwrap();
    assert!(first.complete);
    assert_eq!(
        (
            first.recognized_chapters,
            first.matching_chapters,
            first.total_rows
        ),
        (1000, 1000, 1000)
    );
    assert_eq!(first.rows.len(), 100);
    assert_eq!(first.rows[0].entry.id, "chapter_0");
    let mut query = request();
    query.cursor = first.next_cursor;
    let second = snapshot.query(&query).unwrap();
    assert_eq!(second.offset, 100);
    assert_eq!(second.rows[0].entry.id, "chapter_100");
    query.cursor = None;
    query.offset = usize::MAX;
    assert_eq!(snapshot.query(&query).unwrap().offset, 900);
    query.text = "终点 灯塔 chapter_999".into();
    query.status = "review".into();
    query.pov = "CHARACTER:LIN".into();
    query.selected_id = Some("chapter_999".into());
    let filtered = snapshot.query(&query).unwrap();
    assert_eq!(
        (
            filtered.matching_chapters,
            filtered.total_rows,
            filtered.offset
        ),
        (1, 1, 0)
    );
    assert_eq!(filtered.selection_matches, Some(true));
    assert_eq!(filtered.selected_offset, Some(0));
    query.status = "final".into();
    let empty = snapshot.query(&query).unwrap();
    assert_eq!(empty.recognized_chapters, 1000);
    assert_eq!(empty.total_rows, 0);
    assert_eq!(empty.selection_matches, Some(false));
}

#[test]
fn manuscript_query_equal_titles_and_pov_names_keep_stable_identity() {
    let first = chapter("one", None);
    let mut second = chapter("two", None);
    second["pov"]["id"] = json!("another");
    let snapshot = fixture(vec![first, second])
        .manuscript_query_snapshot(&[], &[])
        .unwrap();
    let mut query = request();
    query.pov = "LIN".into();
    let page = snapshot.query(&query).unwrap();
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.rows[0].perspective_display.as_deref(), Some("Lin"));
    assert_eq!(page.rows[1].perspective_display.as_deref(), Some("Lin"));
    assert_ne!(page.rows[0].entry.id, page.rows[1].entry.id);
    query.pov = "character:another".into();
    assert_eq!(snapshot.query(&query).unwrap().rows[0].entry.id, "two");
    query.text = "同一标题 归乡 灯塔".into();
    assert_eq!(snapshot.query(&query).unwrap().matching_chapters, 1);
}

#[test]
fn manuscript_query_tree_context_collapse_empty_sections_and_scope() {
    let snapshot = fixture(vec![
        json!({"id":"outer","kind":"section","title":"外卷"}),
        json!({"id":"inner","kind":"section","title":"内卷","parent_id":"outer"}),
        chapter("one", Some("inner")),
        json!({"id":"empty","kind":"section","title":"空卷"}),
        chapter("two", None),
    ])
    .manuscript_query_snapshot(&[], &[])
    .unwrap();
    let mut query = request();
    query.view = ManuscriptQueryView::Tree;
    query.collapsed = vec!["outer".into()];
    query.selected_id = Some("one".into());
    let collapsed = snapshot.query(&query).unwrap();
    assert_eq!(collapsed.total_rows, 3);
    assert_eq!(collapsed.selection_matches, Some(true));
    assert_eq!(collapsed.selected_offset, None);
    assert!(collapsed.rows.iter().any(|row| row.entry.id == "empty"));
    query.text = "one".into();
    let expanded = snapshot.query(&query).unwrap();
    assert_eq!(expanded.total_rows, 3);
    assert!(expanded.rows[0].context_only && expanded.rows[1].context_only);
    assert!(!expanded.rows[2].context_only);
    assert_eq!(
        expanded.rows[2].section_path,
        vec![
            ManuscriptSectionPath {
                id: "outer".into(),
                title: "外卷".into()
            },
            ManuscriptSectionPath {
                id: "inner".into(),
                title: "内卷".into()
            }
        ]
    );
    assert_eq!(expanded.selected_offset, Some(2));
    query.section_id = Some("inner".into());
    assert_eq!(snapshot.query(&query).unwrap().total_rows, 2);
    query.section_id = Some("one".into());
    assert_eq!(snapshot.query(&query).unwrap_err().code, "INVALID_SECTION");
    query.section_id = Some("deleted".into());
    assert_eq!(snapshot.query(&query).unwrap_err().code, "INVALID_SECTION");
}

#[test]
fn manuscript_query_deep_hierarchy_is_iterative_and_paths_are_page_local() {
    let mut entries = Vec::new();
    for index in 0..1500 {
        entries.push(
            json!({"id":format!("s{index}"),"kind":"section","title":format!("卷{index}"),
            "parent_id":(index > 0).then(||format!("s{}",index-1))}),
        );
    }
    entries.push(chapter("bottom", Some("s1499")));
    let snapshot = fixture(entries)
        .manuscript_query_snapshot(&[], &[])
        .unwrap();
    assert!(snapshot.books["novel"]
        .rows
        .iter()
        .all(|row| row.row.section_path.is_empty()));
    let page = snapshot.query(&request()).unwrap();
    assert!(page.complete);
    assert_eq!(page.rows[0].section_path.len(), 1500);
    assert_eq!(page.rows[0].ordinal, 1500);
}

#[test]
fn manuscript_query_bad_parents_cycles_duplicates_remain_visible_and_ambiguous() {
    let snapshot = fixture(vec![
        json!({"id":"a","kind":"section","title":"A","parent_id":"b"}),
        json!({"id":"b","kind":"section","title":"B","parent_id":"a"}),
        chapter("in_cycle", Some("a")),
        chapter("orphan", Some("missing")),
        chapter("duplicate", None),
        chapter("duplicate", None),
        chapter("under_chapter", Some("orphan")),
        json!({"id":"section_dup","kind":"section","title":"一"}),
        json!({"id":"section_dup","kind":"section","title":"二"}),
        chapter("ambiguous_parent", Some("section_dup")),
    ])
    .manuscript_query_snapshot(&[], &[])
    .unwrap();
    let mut query = request();
    query.selected_id = Some("duplicate".into());
    let page = snapshot.query(&query).unwrap();
    assert!(!page.complete);
    assert_eq!(page.recognized_chapters, 6);
    assert_eq!(page.rows.len(), 6);
    assert_eq!(page.selection_matches, Some(false));
    assert!(page.selection_ambiguous);
    assert!(snapshot.entry_is_ambiguous("novel", "duplicate"));
    assert!(!snapshot.entry_is_ambiguous("novel", "orphan"));
    assert!(!snapshot.entry_is_ambiguous("absent", "duplicate"));
    assert_eq!(page.selected_offset, None);
    assert!(page
        .rows
        .iter()
        .filter(|row| row.entry.id == "duplicate")
        .all(|row| row.identity_ambiguous));
    for id in ["in_cycle", "orphan", "under_chapter", "ambiguous_parent"] {
        assert!(
            !page
                .rows
                .iter()
                .find(|row| row.entry.id == id)
                .unwrap()
                .path_complete
        );
    }
    assert_eq!(
        page.rows
            .iter()
            .find(|row| row.entry.id == "orphan")
            .unwrap()
            .entry
            .parent_id
            .as_deref(),
        Some("missing")
    );
    assert!(page
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAN007"));
    query.section_id = Some("section_dup".into());
    assert_eq!(snapshot.query(&query).unwrap_err().code, "INVALID_SECTION");
}

#[test]
fn manuscript_query_missing_source_missing_document_and_bad_json_are_not_empty_success() {
    let mut missing = chapter("missing", None);
    missing["target_ref"]["id"] = json!("gone");
    let mut project = fixture(vec![missing]);
    let page = project
        .manuscript_query_snapshot(&[], &[])
        .unwrap()
        .query(&request())
        .unwrap();
    assert!(!page.complete);
    assert_eq!(page.recognized_chapters, 1);
    let source = page.rows[0].entry.source.as_ref().unwrap();
    assert_eq!(source.status, ManuscriptReferenceStatus::Missing);
    assert!(source.stats.is_none() && source.location.is_none());
    let path = project.root.join(".world/manuscripts/novel.json");
    project.authoring_documents.get_mut(&path).unwrap().bytes = b"{bad".to_vec();
    let page = project
        .manuscript_query_snapshot(&[], &[])
        .unwrap()
        .query(&request())
        .unwrap();
    assert!(!page.complete);
    assert_eq!(page.recognized_chapters, 0);
    assert!(page
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAN001"));
    project.authoring_documents.remove(&path);
    let page = project
        .manuscript_query_snapshot(&[], &[])
        .unwrap()
        .query(&request())
        .unwrap();
    assert!(!page.complete);
    assert!(!page.diagnostics.is_empty());
}

#[test]
fn manuscript_query_cursor_rejects_tampering_and_every_query_dependency() {
    let mut project = fixture(
        (0..4)
            .map(|index| chapter(&format!("c{index}"), None))
            .collect(),
    );
    let snapshot = project.manuscript_query_snapshot(&[], &[]).unwrap();
    let mut base = request();
    base.limit = 1;
    let cursor = snapshot.query(&base).unwrap().next_cursor.unwrap();
    let mut changes = Vec::new();
    for field in [
        "text",
        "status",
        "pov",
        "section_id",
        "view",
        "collapsed",
        "selected_id",
        "limit",
    ] {
        let mut query = base.clone();
        query.cursor = Some(cursor.clone());
        match field {
            "text" => query.text = "c".into(),
            "status" => query.status = "draft".into(),
            "pov" => query.pov = "lin".into(),
            "section_id" => {
                continue;
            }
            "view" => query.view = ManuscriptQueryView::Tree,
            "collapsed" => query.collapsed = vec!["unknown".into()],
            "selected_id" => query.selected_id = Some("c0".into()),
            "limit" => query.limit = 2,
            _ => unreachable!(),
        }
        changes.push(query);
    }
    for query in changes {
        assert_eq!(snapshot.query(&query).unwrap_err().code, "STALE_CURSOR");
    }
    base.cursor = Some(cursor.replace(":1:", ":2:"));
    assert_eq!(snapshot.query(&base).unwrap_err().code, "STALE_CURSOR");
    base.cursor = Some(cursor);
    base.offset = 999;
    assert_eq!(snapshot.query(&base).unwrap().offset, 1);
    project
        .documents
        .get_mut(&project.entry)
        .unwrap()
        .text
        .push_str("// newer\n");
    let newer = project.manuscript_query_snapshot(&[], &[]).unwrap();
    assert_eq!(newer.query(&base).unwrap_err().code, "STALE_CURSOR");
}

#[test]
fn manuscript_query_request_schema_rejects_unknown_fields_and_bad_version() {
    let decoded: ManuscriptQueryRequest =
        serde_json::from_value(json!({"schema_version":1,"manuscript_id":"novel"})).unwrap();
    assert_eq!(decoded.limit, 100);
    assert_eq!(decoded.view, ManuscriptQueryView::Chapters);
    assert!(serde_json::from_value::<ManuscriptQueryRequest>(
        json!({"schema_version":1,"manuscript_id":"novel","typo":true})
    )
    .is_err());
    let snapshot = fixture(vec![chapter("one", None)])
        .manuscript_query_snapshot(&[], &[])
        .unwrap();
    let mut query = request();
    query.schema_version = 2;
    assert_eq!(
        snapshot.query(&query).unwrap_err().code,
        "UNSUPPORTED_QUERY_VERSION"
    );
    query = request();
    for limit in [0, 101, usize::MAX] {
        query.limit = limit;
        assert_eq!(snapshot.query(&query).unwrap_err().code, "INVALID_LIMIT");
        assert_eq!(query.validate().unwrap_err().code, "INVALID_LIMIT");
    }
    for limit in [1, 100] {
        query.limit = limit;
        assert_eq!(snapshot.query(&query).unwrap().limit, limit);
        assert!(query.validate().is_ok());
    }
    query.manuscript_id = "absent".into();
    assert_eq!(
        snapshot.query(&query).unwrap_err().code,
        "MANUSCRIPT_NOT_FOUND"
    );
}

#[test]
fn manuscript_query_bad_kind_duplicate_id_cannot_select_or_define_section_scope() {
    for bad_kind in ["future_kind", "typo"] {
        let project = fixture(vec![
            chapter("same", None),
            json!({"id":"same","kind":bad_kind,"title":"坏项"}),
        ]);
        let before = project.snapshot_state().unwrap();
        let snapshot = project.manuscript_query_snapshot(&[], &[]).unwrap();
        let mut selected = request();
        selected.selected_id = Some("same".into());
        let page = snapshot.query(&selected).unwrap();
        assert!(!page.complete);
        assert_eq!(page.rows.len(), 1, "只有可识别项投影，但坏项仍争用身份");
        assert!(page.rows[0].identity_ambiguous);
        assert!(snapshot.entry_is_ambiguous("novel", "same"));
        assert!(page.selection_ambiguous);
        assert_eq!(page.selection_matches, Some(false));
        assert_eq!(page.selected_offset, None);
        assert!(page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "MAN005"));
        assert_eq!(project.snapshot_state().unwrap(), before);
    }
    let project = fixture(vec![
        json!({"id":"section","kind":"section","title":"合法分节"}),
        json!({"id":"section","kind":"future_kind","title":"坏分节"}),
        chapter("child", Some("section")),
    ]);
    let snapshot = project.manuscript_query_snapshot(&[], &[]).unwrap();
    let mut query = request();
    query.view = ManuscriptQueryView::Tree;
    let page = snapshot.query(&query).unwrap();
    assert!(!page.complete);
    assert!(
        page.rows
            .iter()
            .find(|row| row.entry.id == "section")
            .unwrap()
            .identity_ambiguous
    );
    let child = page
        .rows
        .iter()
        .find(|row| row.entry.id == "child")
        .unwrap();
    assert!(!child.path_complete);
    assert!(
        child.section_path.is_empty(),
        "不能选合法同ID分节当作真实父项"
    );
    query.section_id = Some("section".into());
    assert_eq!(snapshot.query(&query).unwrap_err().code, "INVALID_SECTION");
}

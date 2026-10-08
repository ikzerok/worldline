use super::*;
use crate::manuscript::{ManuscriptDraft, ManuscriptQueryDraft, ManuscriptQueryView, ReviewKind};
use crate::project::Project;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const SOURCE: &str = "let gate = true\ncharacter lin as \"林\"\nentity harbor kind place as \"港\"\n  description \"实体审稿文字\"\nfragment hidden()\n  仅调用不应展开\n  return\nevent start // SECRET_COMMENT\n  开场😀 <script>alert(1)</script>\n  if gate\n    甲分支\n  else\n    乙分支\n  choice once \"选择一\" if true enable false disabled \"缺少钥匙\"\n    say lin \"对白与 {rnd(1, 2)} [[character:lin|朋友]]\"\n    call hidden()\n  choice \"选择二\"\n    -> END\n  scene inside\n    深层文字\n  -> END\n";

fn chapter(id: &str, kind: &str, target: &str) -> Value {
    json!({"id":id,"kind":"chapter","title":"同名章","status":"review","pov":{"kind":"character","id":"lin"},"summary":"海港", "target_ref":{"kind":kind,"id":target}})
}
fn fixture(entries: Vec<Value>) -> Project {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "manuscript-delivery-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let files = BTreeMap::from([
        (PathBuf::from("world.wl"), SOURCE.as_bytes().to_vec()),
        (PathBuf::from(".world/project.json"), serde_json::to_vec(&json!({"schema_version":1,"language_version":"1.12","required_features":["presentation.manuscripts.v1","content.choice_presentation.v1"],"manuscripts":{"book":".world/manuscripts/book.json"}})).unwrap()),
        (PathBuf::from(".world/manuscripts/book.json"), serde_json::to_vec(&json!({"schema_version":1,"id":"book","title":"审稿书","entries":entries,"private_comment":"NOT_SELECTED_COMMENT"})).unwrap()),
        (PathBuf::from("private.txt"), b"UNSELECTED_ATTACHMENT".to_vec()),
    ]);
    Project::from_snapshot(&root, Path::new("world.wl"), &files).unwrap()
}
fn request() -> ManuscriptDeliveryRequest {
    ManuscriptDeliveryRequest::new(ManuscriptQueryRequest {
        manuscript_id: "book".into(),
        ..Default::default()
    })
}
fn report(project: &Project, request: &ManuscriptDeliveryRequest) -> ManuscriptDeliveryReport {
    generate_manuscript_delivery(
        project
            .manuscript_delivery_snapshot(&[], &[], request)
            .unwrap(),
        &mut |_| true,
    )
    .unwrap()
}

#[test]
fn full_filtered_scope_ignores_navigation_page_and_tree_collapse_and_retains_occurrences() {
    let mut entries = vec![
        json!({"id":"outer","kind":"section","title":"外卷"}),
        json!({"id":"inner","kind":"section","title":"内卷","parent_id":"outer"}),
    ];
    for index in 0..125 {
        let mut entry = chapter(&format!("chapter_{index}"), "event", "start");
        entry["parent_id"] = json!("inner");
        if index % 2 == 0 {
            entry["status"] = json!("draft");
        }
        entries.push(entry);
    }
    let project = fixture(entries);
    let before = project.content_baseline();
    let mut input = request();
    input.query.text = "海港".into();
    input.query.status = "review".into();
    input.query.pov = "character:lin".into();
    input.query.section_id = Some("outer".into());
    input.query.view = ManuscriptQueryView::Tree;
    input.query.collapsed = vec!["outer".into()];
    input.query.offset = 50;
    input.query.limit = 2;
    let report = report(&project, &input);
    assert!(report.complete(), "{:?}", report.scope().diagnostics);
    assert_eq!(report.scope().selected_occurrences, 62);
    assert_eq!(report.scope().unique_sources, 1);
    assert_eq!(report.scope().repeated_source_occurrences, 61);
    assert_eq!(report.scope().chapters[0].entry.id, "chapter_1");
    assert_eq!(report.scope().chapters[61].entry.id, "chapter_123");
    assert_eq!(report.scope().chapters[0].section_path.len(), 2);
    assert_eq!(report.chapters().len(), 62);
    assert_eq!(
        report
            .chapters()
            .iter()
            .map(|chapter| chapter.review_bytes)
            .sum::<usize>(),
        report.usage().review_bytes
    );
    for chapter in report.chapters() {
        assert_eq!(
            chapter.review_bytes,
            serde_json::to_vec(chapter.review.as_deref().unwrap())
                .unwrap()
                .len()
        );
    }
    assert_eq!(report.markdown().unwrap().matches("甲分支").count(), 62);
    assert_eq!(project.content_baseline(), before);
}

#[test]
fn markdown_keeps_every_review_variant_and_no_unselected_call_content_html_paths_or_comments() {
    let project = fixture(vec![
        chapter("main", "event", "start"),
        chapter("description", "entity", "harbor"),
        chapter("fragment", "fragment", "hidden"),
    ]);
    let all = report(&project, &request());
    assert!(all.complete(), "{:?}", all.scope().diagnostics);
    fn kinds(nodes: &[crate::manuscript::ReviewNode], found: &mut Vec<ReviewKind>) {
        for node in nodes {
            found.push(node.kind);
            kinds(&node.children, found);
        }
    }
    let mut found = Vec::new();
    for chapter in all.chapters() {
        kinds(&chapter.review.as_ref().unwrap().nodes, &mut found);
    }
    for kind in [
        ReviewKind::Text,
        ReviewKind::Say,
        ReviewKind::If,
        ReviewKind::Branch,
        ReviewKind::ChoiceGroup,
        ReviewKind::Choice,
        ReviewKind::Scene,
        ReviewKind::Call,
        ReviewKind::Return,
        ReviewKind::Divert,
        ReviewKind::Structure,
        ReviewKind::Description,
    ] {
        assert!(found.contains(&kind), "缺少 {kind:?}");
    }
    let mut selected = request();
    selected.chapter_ids = Some(vec!["main".into()]);
    let report = report(&project, &selected);
    let markdown = report.markdown().unwrap();
    for text in [
        "甲分支",
        "乙分支",
        "选择一",
        "选择二",
        "对白与",
        "深层文字",
        "未求值",
        "未展开",
        "&lt;script&gt;",
    ] {
        assert!(markdown.contains(text), "缺少{text}");
    }
    for text in [
        "仅调用不应展开",
        "实体审稿文字",
        "<script>",
        "SECRET_COMMENT",
        "NOT_SELECTED_COMMENT",
        "UNSELECTED_ATTACHMENT",
        project.root.to_str().unwrap(),
    ] {
        assert!(!markdown.contains(text), "泄漏{text}");
    }
    assert_eq!(report.usage().markdown_bytes, markdown.len());
}

#[test]
fn explicit_selection_uses_book_order_and_rejects_duplicate_missing_or_ambiguous_ids() {
    let project = fixture(vec![
        chapter("first", "event", "start"),
        chapter("second", "event", "start"),
    ]);
    let mut input = request();
    input.chapter_ids = Some(vec!["second".into(), "first".into()]);
    let all = report(&project, &input);
    assert_eq!(all.scope().chapters[0].entry.id, "first");
    input.chapter_ids = Some(vec!["first".into(), "first".into()]);
    assert_eq!(
        project
            .manuscript_delivery_snapshot(&[], &[], &input)
            .unwrap_err()
            .code,
        "DUPLICATE_SELECTION"
    );
    input.chapter_ids = Some(vec!["missing".into()]);
    assert_eq!(
        project
            .manuscript_delivery_snapshot(&[], &[], &input)
            .unwrap_err()
            .code,
        "SELECTION_OUTSIDE_SCOPE"
    );
    let bad = fixture(vec![
        chapter("same", "event", "start"),
        chapter("same", "event", "start"),
    ]);
    input.chapter_ids = Some(vec!["same".into()]);
    assert_eq!(
        bad.manuscript_delivery_snapshot(&[], &[], &input)
            .unwrap_err()
            .code,
        "AMBIGUOUS_SELECTION"
    );
}

#[test]
fn missing_invalid_and_zero_chapters_are_distinct_and_never_silently_dropped() {
    let mut missing = chapter("missing", "event", "absent");
    missing["summary"] = json!("缺源");
    let project = fixture(vec![chapter("good", "event", "start"), missing]);
    let result = report(&project, &request());
    assert!(!result.complete());
    assert!(result.markdown().is_none());
    assert_eq!(result.chapters().len(), 2);
    assert!(result.chapters()[1].error.is_some());
    let project = fixture(vec![chapter("one", "event", "start")]);
    let mut empty = request();
    empty.query.text = "无匹配词".into();
    let result = report(&project, &empty);
    assert!(result.complete());
    assert_eq!(result.scope().selected_occurrences, 0);
    assert!(result.markdown().unwrap().contains("零章"));
    empty.query.text.clear();
    empty.chapter_ids = Some(vec![]);
    assert_eq!(report(&project, &empty).scope().selected_occurrences, 0);
    let mut buffer = project.open_source_writing_buffer(&project.entry).unwrap();
    buffer.replace_source("event start\n  if (\n".into());
    let snapshot = project
        .manuscript_delivery_snapshot(&[buffer], &[], &request())
        .unwrap();
    let result = generate_manuscript_delivery(snapshot, &mut |_| true).unwrap();
    assert!(!result.complete());
    assert!(result.markdown().is_none());
    assert_eq!(result.chapters().len(), 1);
}

#[test]
fn immutable_current_draft_scope_tracks_generation_and_does_not_apply() {
    let project = fixture(vec![chapter("one", "event", "start")]);
    let before = project.content_baseline();
    let original = project.document(&project.entry).unwrap().to_owned();
    let mut buffer = project.open_source_writing_buffer(&project.entry).unwrap();
    buffer.replace_source(original.replace("开场", "未应用新文字"));
    let query = Arc::new(
        project
            .manuscript_query_snapshot(&[buffer.clone()], &[])
            .unwrap(),
    );
    let mut input = request();
    input.expected_snapshot_key = Some(query.key().into());
    let snapshot = ManuscriptDeliverySnapshot::new(query.clone(), &input).unwrap();
    assert!(Arc::ptr_eq(snapshot.query_snapshot(), &query));
    let report = generate_manuscript_delivery(snapshot, &mut |_| true).unwrap();
    assert_eq!(report.scope().source, ManuscriptQuerySource::Draft);
    assert!(report.markdown().unwrap().contains("未应用新文字"));
    buffer.replace_source(buffer.source().replace("未应用新文字", "再次改稿"));
    assert_eq!(
        project
            .validate_manuscript_delivery(&[buffer.clone()], &[], &report)
            .unwrap_err()
            .code,
        "STALE_SNAPSHOT"
    );
    assert_eq!(
        project
            .manuscript_delivery_snapshot(&[buffer.clone(), buffer], &[], &request())
            .unwrap_err()
            .code,
        "DUPLICATE_DRAFT"
    );
    let new_query = Arc::new(project.manuscript_query_snapshot(&[], &[]).unwrap());
    assert_eq!(
        ManuscriptDeliverySnapshot::new(new_query, &input)
            .unwrap_err()
            .code,
        "STALE_SNAPSHOT"
    );
    assert_eq!(project.content_baseline(), before);
    assert_eq!(project.document(&project.entry).unwrap(), original);
}

#[test]
fn arrangement_draft_changes_same_scope_without_mutating_registered_document() {
    let project = fixture(vec![
        chapter("one", "event", "start"),
        chapter("two", "event", "start"),
    ]);
    let query = project.manuscript_query_snapshot(&[], &[]).unwrap();
    let mut draft = ManuscriptDraft::from_index(&query.indices()["book"]);
    draft.entries.reverse();
    draft.entries[0].title = "新编排标题".into();
    let draft = ManuscriptQueryDraft {
        expected_baseline: project.content_baseline(),
        draft,
    };
    let snapshot = project
        .manuscript_delivery_snapshot(&[], &[draft], &request())
        .unwrap();
    let report = generate_manuscript_delivery(snapshot, &mut |_| true).unwrap();
    assert_eq!(report.scope().chapters[0].entry.id, "two");
    assert!(report.markdown().unwrap().contains("新编排标题"));
    assert_eq!(query.indices()["book"].entries[0].id, "one");
}

#[test]
fn cumulative_budgets_cancellation_and_strict_dto_never_return_truncated_artifacts() {
    let project = fixture(vec![
        chapter("one", "event", "start"),
        chapter("two", "event", "start"),
    ]);
    let snapshot = project
        .manuscript_delivery_snapshot(&[], &[], &request())
        .unwrap();
    let mut job = ManuscriptDeliveryJob::new(snapshot.clone()).unwrap();
    assert!(job.advance(1).unwrap().is_none());
    assert_eq!(job.progress().completed, 1);
    job.cancel();
    assert_eq!(job.advance(1).unwrap_err().code, "CANCELLED");
    assert_eq!(
        generate_manuscript_delivery(snapshot, &mut |_| false)
            .unwrap_err()
            .code,
        "CANCELLED"
    );
    let mut input = request();
    input.limits.nodes = 1;
    assert_eq!(
        generate_manuscript_delivery(
            project
                .manuscript_delivery_snapshot(&[], &[], &input)
                .unwrap(),
            &mut |_| true
        )
        .unwrap_err()
        .code,
        "BUDGET_EXCEEDED"
    );
    input = request();
    input.limits.markdown_bytes = 32;
    assert!(ManuscriptDeliveryJob::new(
        project
            .manuscript_delivery_snapshot(&[], &[], &input)
            .unwrap()
    )
    .is_err());
    input = request();
    input.limits.chapters = 1;
    assert!(project
        .manuscript_delivery_snapshot(&[], &[], &input)
        .is_err());
    let raw = serde_json::to_string(&request()).unwrap();
    assert!(parse_manuscript_delivery_request(&raw).is_ok());
    assert!(parse_manuscript_delivery_request(&raw.replacen(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
        1
    ))
    .is_err());
    let mut value = json!(request());
    value["future"] = json!(true);
    assert!(parse_manuscript_delivery_request(&value.to_string()).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_new_target_is_atomic_identical_and_cancel_or_collision_leaves_no_target() {
    let project = fixture(vec![chapter("one", "event", "start")]);
    let report = report(&project, &request());
    let directory = project.root.with_extension("delivery");
    std::fs::create_dir_all(&directory).unwrap();
    let target = directory.join("review.md");
    write_manuscript_markdown_new(&project.root, &target, &report, &mut || Ok(())).unwrap();
    assert_eq!(
        std::fs::read(&target).unwrap(),
        report.markdown().unwrap().as_bytes()
    );
    assert!(
        write_manuscript_markdown_new(&project.root, &target, &report, &mut || Ok(())).is_err()
    );
    let cancelled = directory.join("cancelled.md");
    let mut calls = 0;
    assert!(
        write_manuscript_markdown_new(&project.root, &cancelled, &report, &mut || {
            calls += 1;
            if calls == 2 {
                Err("取消或过期".into())
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    assert!(!cancelled.exists());
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
    let raced = directory.join("raced.md");
    let mut calls = 0;
    assert!(
        write_manuscript_markdown_new(&project.root, &raced, &report, &mut || {
            calls += 1;
            if calls == 2 {
                std::fs::write(&raced, b"other author").unwrap();
            }
            Ok(())
        })
        .is_err()
    );
    assert_eq!(std::fs::read(&raced).unwrap(), b"other author");
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn delivery_action_rechecks_attachment_inventory_before_background_refresh() {
    let mut project = fixture(vec![chapter("one", "event", "start")]);
    for (path, document) in &project.documents {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &document.text).unwrap();
    }
    for (path, document) in &project.authoring_documents {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, document.bytes()).unwrap();
    }
    let attachment = project.root.join("unselected.txt");
    std::fs::write(&attachment, b"not included").unwrap();
    project.mark_saved();
    project.refresh().unwrap();
    let report = report(&project, &request());
    project
        .validate_manuscript_delivery(&[], &[], &report)
        .unwrap();
    let cached = project.manuscript_query_key(&[], &[]);
    std::fs::remove_file(&attachment).unwrap();
    assert_eq!(
        project.manuscript_query_key(&[], &[]),
        cached,
        "idle缓存没有偷偷读盘"
    );
    assert_eq!(
        project
            .validate_manuscript_delivery(&[], &[], &report)
            .unwrap_err()
            .code,
        "STALE_OBSERVATION"
    );
    let destination = project.root.with_extension("md");
    assert!(
        write_manuscript_markdown_new(&project.root, &destination, &report, &mut || {
            project
                .validate_manuscript_delivery(&[], &[], &report)
                .map_err(|error| error.to_string())
        })
        .is_err()
    );
    assert!(!destination.exists());
    std::fs::remove_dir_all(&project.root).unwrap();
}

#[cfg(target_arch = "wasm32")]
#[test]
fn browser_delivery_observes_mounted_snapshot_without_host_disk_access() {
    let root = Path::new("/delivery-browser-test");
    let files = BTreeMap::from([
        (PathBuf::from("world.wl"), b"event start\n  browser prose\n  -> END\n".to_vec()),
        (PathBuf::from(".world/project.json"), br#"{"schema_version":1,"required_features":["presentation.manuscripts.v1"],"manuscripts":{"book":".world/book.json"}}"#.to_vec()),
        (PathBuf::from(".world/book.json"), br#"{"schema_version":1,"id":"book","title":"Book","entries":[{"id":"one","kind":"chapter","title":"One","target_ref":{"kind":"event","id":"start"}}]}"#.to_vec()),
        (PathBuf::from("attachment.txt"), b"not delivered".to_vec()),
    ]);
    crate::file_access::mount(
        files
            .iter()
            .map(|(path, bytes)| (root.join(path), bytes.clone()))
            .collect(),
    );
    let project = Project::from_snapshot(root, Path::new("world.wl"), &files).unwrap();
    let report = report(&project, &request());
    project
        .validate_manuscript_delivery(&[], &[], &report)
        .unwrap();
    crate::file_access::mount(
        files
            .iter()
            .filter(|(path, _)| path.as_path() != Path::new("attachment.txt"))
            .map(|(path, bytes)| (root.join(path), bytes.clone()))
            .collect(),
    );
    assert_eq!(
        project
            .validate_manuscript_delivery(&[], &[], &report)
            .unwrap_err()
            .code,
        "STALE_OBSERVATION"
    );
}

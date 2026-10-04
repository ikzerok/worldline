use std::{fs, path::PathBuf, time::Instant};
use worldline_core::{project::Project, search_replace::*};

fn fixture(name: &str, source: &str) -> (PathBuf, Project, SearchRequest) {
    let root = std::env::temp_dir().join(format!("replace-scale-{name}-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    let project = Project::open(&root).unwrap();
    let request = SearchRequest {
        query: "needle".into(),
        replacement: "needle更正".into(),
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
fn one_thousand_and_ten_thousand_selections_reconcile_and_preview_bounded_contexts() {
    for count in [1000, 10000] {
        let source = format!("event start\n  {}\n  -> END\n", "needle ".repeat(count));
        let (root, project, request) = fixture(&count.to_string(), &source);
        let started = Instant::now();
        let original = project.search_drafts(&request, &[]).unwrap();
        let current = project.search_drafts(&request, &[]).unwrap();
        assert_eq!(current.len(), count);
        assert_eq!(current.last().unwrap().column, (3 + 7 * (count - 1)) as u32);
        let selected: Vec<_> = original.iter().rev().cloned().collect();
        let aligned = reconcile_search_selection(&current, &selected).unwrap();
        assert_eq!(aligned.len(), count);
        assert_eq!(
            aligned.first().unwrap().range,
            current.first().unwrap().range
        );
        assert_eq!(aligned.last().unwrap().range, current.last().unwrap().range);
        // 两次查找共享各自快照；此对齐不逐项反复深比较全文。
        let plan = project
            .preview_search_replace_selected(&request, &[], &aligned)
            .unwrap();
        assert_eq!(plan.hits.len(), count);
        assert_eq!(plan.occurrences.len(), count);
        assert_eq!(plan.changes[0].count, count);
        assert_eq!(plan.changes[0].after.matches("needle更正").count(), count);
        for hit in &plan.hits {
            let context = hit.context.as_ref().unwrap();
            assert!(context.text.chars().count() <= 160);
            assert!(hit.preview.len() <= 640);
            assert_eq!(&context.text[context.highlight.clone()], "needle");
        }
        for occurrence in &plan.occurrences {
            let context = &occurrence.after_context;
            assert!(context.text.chars().count() <= 160);
            assert_eq!(&context.text[context.highlight.clone()], "needle更正");
        }
        assert_eq!(project.document(&root.join("world.wl")).unwrap(), source);
        assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), source);
        eprintln!(
            "{count}同一行命中：两次查找、批量对齐、逐处预览 {:?}",
            started.elapsed()
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn ten_thousand_is_still_a_hard_limit_and_read_only_alignment_rejects_duplicates() {
    let source = format!("event start\n  {}\n  -> END\n", "needle ".repeat(10001));
    let (root, project, request) = fixture("limit", &source);
    assert!(project
        .search_drafts(&request, &[])
        .unwrap_err()
        .contains("10000"));
    assert!(project.preview_search_replace(&request, &[]).is_err());
    let mut smaller = request.clone();
    let start = source.find("needle").unwrap();
    smaller.files[0].range = Some(start..start + 6);
    let current = project.search_drafts(&smaller, &[]).unwrap();
    let hit = current[0].clone();
    assert!(reconcile_search_selection(&current, &[hit.clone(), hit.clone()]).is_err());
    assert!(
        reconcile_search_selection(&[hit.clone(), hit.clone()], std::slice::from_ref(&hit))
            .is_err()
    );
    assert!(reconcile_search_selection(&[], &[hit]).is_err());
    assert!(reconcile_search_selection(&current, &[])
        .unwrap()
        .is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unicode_crlf_multiline_and_long_queries_have_exact_public_contexts() {
    let prefix = "🦀".repeat(250);
    let source =
        format!("event start\r\n  {prefix}e\u{301}目标 e\u{301}目标\r\n  第二行\r\n  -> END\r\n");
    let (root, project, mut request) = fixture("unicode", &source);
    request.query = "e\u{301}目标".into();
    let hits = project.search_drafts(&request, &[]).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].line, 2);
    assert_eq!(hits[0].column, 253);
    assert_eq!(hits[1].column, 258);
    for hit in &hits {
        let context = hit.context.as_ref().unwrap();
        assert_eq!(context.text, source[context.source_range.clone()]);
        assert_eq!(&context.text[context.highlight.clone()], request.query);
        assert!(!context.text.contains(['\r', '\n']));
    }
    request.scope = SearchScope::Source;
    request.query = "目标\r\n  第二行".into();
    let hits = project.search_drafts(&request, &[]).unwrap();
    assert_eq!(hits.len(), 1);
    let context = hits[0].context.as_ref().unwrap();
    assert_eq!(&context.text[context.highlight.clone()], request.query);
    assert!(!hits[0].replaceable);
    assert!(project
        .preview_search_replace_selected(&request, &[], &hits)
        .is_err());
    request.query = prefix;
    let hits = project.search_drafts(&request, &[]).unwrap();
    let context = hits[0].context.as_ref().unwrap();
    assert!(context.match_omitted_after);
    assert!(!context.match_omitted_before);
    assert_eq!(context.text.chars().count(), 160);
    assert_eq!(context.text.len(), 640);
    fs::remove_dir_all(root).unwrap();
}

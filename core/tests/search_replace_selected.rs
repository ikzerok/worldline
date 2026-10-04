//! 精确离散命中的授权、过期、原子提交与持久化回归。
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use worldline_core::{manuscript::WritingBuffer, project::Project, search_replace::*};

const WORLD: &str = concat!(
    "character hero as \"英雄\"\nlet count = 1\nevent start\n",
    "  Hello 世界 Hello {count} [[character:hero|Hello]] #line:hello\n",
    "  say hero \"Hello {count}\" direction \"Hello private\"\n  -> END\n"
);
const SECOND: &str = "event next\n  Hello again Hello\n  -> END\n";

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-selected-replace-{}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".world")).unwrap();
        let root = Project::new(&root).root;
        fs::write(
            root.join(".world/project.json"),
            r#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#,
        )
        .unwrap();
        fs::write(root.join("world.wl"), WORLD).unwrap();
        fs::write(root.join("second.wl"), SECOND).unwrap();
        Self(root)
    }
    fn open(&self) -> Project {
        Project::open(&self.0).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn request() -> SearchRequest {
    SearchRequest {
        query: "Hello".into(),
        replacement: "Goodbye".into(),
        options: SearchOptions::default(),
        scope: SearchScope::Prose,
        files: ["world.wl", "second.wl"]
            .into_iter()
            .map(|path| SearchFile { path: path.into(), range: None })
            .collect(),
    }
}
fn one_file() -> SearchRequest {
    let mut req = request();
    req.files.truncate(1);
    req
}
fn texts(project: &Project) -> Vec<String> {
    ["world.wl", "second.wl"]
        .into_iter()
        .map(|path| project.document(&project.root.join(path)).unwrap().to_owned())
        .collect()
}
fn buffer(project: &Project) -> WritingBuffer {
    project.open_source_writing_buffer(Path::new("world.wl")).unwrap()
}
fn preview(
    project: &Project,
    req: &SearchRequest,
    drafts: &[WritingBuffer],
    selected: &[SearchMatch],
) -> Result<ReplacePlan, String> {
    project.preview_search_replace_selected(req, drafts, selected)
}
fn assert_context(source: &str, range: std::ops::Range<usize>, context: &SearchContext) {
    assert_eq!(context.text, source[context.source_range.clone()]);
    assert_eq!(&context.text[context.highlight.clone()], &source[range]);
    assert!(context.text.chars().count() <= 160);
    assert!(context.text.len() <= 640);
}

#[test]
fn selects_only_second_same_line_hit_and_changes_only_returned_draft() {
    let ws = Workspace::new();
    let project = ws.open();
    let original = buffer(&project);
    let drafts = [original.clone()];
    let req = one_file();
    let hits = project.search_drafts(&req, &drafts).unwrap();
    assert_eq!(hits.len(), 3);
    assert_eq!(hits[0].line, hits[1].line);
    assert_ne!(hits[0].column, hits[1].column);
    let plan = preview(&project, &req, &drafts, &hits[1..2]).unwrap();
    assert_eq!(plan.hits, hits[1..2]);
    assert_eq!(plan.changes.len(), 1);
    assert_eq!(plan.changes[0].count, 1);
    assert_eq!(plan.occurrences.len(), 1);
    let occurrence = &plan.occurrences[0];
    assert_context(WORLD, occurrence.before_range.clone(), &occurrence.before_context);
    assert_eq!(&occurrence.before_context.text[occurrence.before_context.highlight.clone()], "Hello");
    assert_context(&plan.changes[0].after, occurrence.after_range.clone(), &occurrence.after_context);
    let next = project.replace_search_draft(&plan, &drafts, &original).unwrap();
    assert!(next.source().contains("Hello 世界 Goodbye {count}"));
    assert!(next.source().contains("[[character:hero|Hello]] #line:hello"));
    assert!(next.source().contains("say hero \"Hello {count}\" direction \"Hello private\""));
    assert_eq!(next.generation(), original.generation() + 1);
    assert_eq!(original.source(), WORLD);
    assert_eq!(drafts[0].source(), WORLD);
    assert_eq!(texts(&project), [WORLD, SECOND]);
    assert_eq!(fs::read_to_string(ws.0.join("world.wl")).unwrap(), WORLD);
}

#[test]
fn selected_original_ranges_are_sorted_and_never_recursively_replaced() {
    let ws = Workspace::new();
    let mut project = ws.open();
    let mut req = one_file();
    req.replacement = "Hello Hello".into();
    let hits = project.search_drafts(&req, &[]).unwrap();
    let selected = [hits[1].clone(), hits[0].clone()];
    let plan = preview(&project, &req, &[], &selected).unwrap();
    assert_eq!(plan.hits, hits[..2]);
    assert_eq!(plan.occurrences.len(), 2);
    assert_eq!(plan.changes[0].count, 2);
    for (index, occurrence) in plan.occurrences.iter().enumerate() {
        assert_eq!(occurrence.before_range, hits[index].range);
        assert_eq!(occurrence.after_range.start, hits[index].range.start + index * 6);
        assert_eq!(&plan.changes[0].after[occurrence.after_range.clone()], "Hello Hello");
        assert_context(&plan.changes[0].after, occurrence.after_range.clone(), &occurrence.after_context);
    }
    let expected = WORLD.replacen("Hello 世界 Hello", "Hello Hello 世界 Hello Hello", 1);
    project.apply_search_replace(&plan, &[]).unwrap();
    assert_eq!(texts(&project), [expected, SECOND.into()]);
    assert!(project.apply_search_replace(&plan, &[]).is_err());
}

#[test]
fn cross_file_selected_drafts_commit_together_undo_and_save_reopen() {
    let ws = Workspace::new();
    let mut project = ws.open();
    let before = project.clone();
    let mut first = buffer(&project);
    first.replace_source(WORLD.replace("世界", "当前稿"));
    let mut second = project.open_source_writing_buffer(Path::new("second.wl")).unwrap();
    second.replace_source(SECOND.replace("again", "draft"));
    let drafts = [first, second];
    let mut req = request();
    req.files.reverse();
    let hits = project.search_drafts(&req, &drafts).unwrap();
    assert_eq!(hits.len(), 5);
    let selected = [hits[3].clone(), hits[0].clone()];
    let plan = preview(&project, &req, &drafts, &selected).unwrap();
    assert_eq!(plan.hits, [hits[0].clone(), hits[3].clone()]);
    assert_eq!(plan.changes.len(), 2);
    assert!(plan.changes.iter().all(|change| change.count == 1));
    assert_eq!(plan.occurrences[0].path, ws.0.join("second.wl"));
    assert_eq!(plan.occurrences[1].path, ws.0.join("world.wl"));
    project.apply_search_replace(&plan, &drafts).unwrap();
    let expected = vec![
        WORLD.replace("世界 Hello", "当前稿 Goodbye"),
        SECOND.replace("Hello again", "Goodbye draft"),
    ];
    assert_eq!(texts(&project), expected);
    assert!(project.is_dirty());
    assert_eq!(fs::read_to_string(ws.0.join("world.wl")).unwrap(), WORLD);
    assert_eq!(fs::read_to_string(ws.0.join("second.wl")).unwrap(), SECOND);
    assert!(!drafts[0].source().contains("Goodbye"));
    assert!(!drafts[1].source().contains("Goodbye"));
    let committed = project.clone();
    assert!(project.restore(before.clone()));
    assert_eq!(texts(&project), [WORLD, SECOND]);
    assert!(project.restore(committed));
    assert_eq!(texts(&project), expected);
    project.save().unwrap();
    assert!(!project.is_dirty());
    assert_eq!(texts(&ws.open()), expected);
    assert!(project.restore(before));
    assert_eq!(texts(&project), [WORLD, SECOND]);
    assert!(project.is_dirty());
    assert_eq!(texts(&ws.open()), expected);
}

#[test]
fn cross_file_failure_is_atomic_for_disk_conflicts_and_stale_project_sources() {
    for external in [false, true] {
        let ws = Workspace::new();
        let mut project = ws.open();
        let mut draft = buffer(&project);
        draft.replace_source(WORLD.replace("世界", "当前稿"));
        let drafts = [draft];
        let req = request();
        let hits = project.search_drafts(&req, &drafts).unwrap();
        let selected = [hits[0].clone(), hits[3].clone()];
        let plan = preview(&project, &req, &drafts, &selected).unwrap();
        let changed = SECOND.replace("again", "external");
        if external {
            fs::write(ws.0.join("second.wl"), &changed).unwrap();
        } else {
            project.set_text(&ws.0.join("second.wl"), changed).unwrap();
        }
        let before = texts(&project);
        let baseline = project.content_baseline();
        assert!(project.apply_search_replace(&plan, &drafts).is_err());
        assert_eq!(texts(&project), before);
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(drafts[0].source(), WORLD.replace("世界", "当前稿"));
        assert_eq!(fs::read_to_string(ws.0.join("world.wl")).unwrap(), WORLD);
    }
}

#[test]
fn empty_query_empty_selection_and_absent_query_cannot_authorize_a_plan() {
    let ws = Workspace::new();
    let project = ws.open();
    let mut req = request();
    let hits = project.search_drafts(&req, &[]).unwrap();
    assert!(preview(&project, &req, &[], &[]).is_err());
    req.query.clear();
    assert!(project.search_drafts(&req, &[]).unwrap().is_empty());
    assert!(preview(&project, &req, &[], &hits[..1]).is_err());
    assert!(preview(&project, &req, &[], &[]).is_err());
    assert!(project.preview_search_replace(&req, &[]).is_err());
    req.query = "absentneedle".into();
    let absent = project.search_drafts(&req, &[]).unwrap();
    assert!(absent.is_empty());
    assert!(preview(&project, &req, &[], &absent).is_err());
    assert!(preview(&project, &req, &[], &hits[..1]).is_err());
    assert_eq!(texts(&project), [WORLD, SECOND]);
}

#[test]
fn source_scope_can_select_safe_prose_but_protected_or_legacy_all_is_rejected() {
    let ws = Workspace::new();
    let mut project = ws.open();
    let mut req = one_file();
    req.scope = SearchScope::Source;
    let hits = project.search_drafts(&req, &[]).unwrap();
    let safe: Vec<_> = hits.iter().filter(|hit| hit.replaceable).cloned().collect();
    let protected: Vec<_> = hits.iter().filter(|hit| !hit.replaceable).cloned().collect();
    assert_eq!(safe.len(), 3);
    assert_eq!(protected.len(), 2);
    assert!(project.preview_search_replace(&req, &[]).is_err());
    assert!(preview(&project, &req, &[], &protected).is_err());
    assert!(preview(&project, &req, &[], &hits).is_err());
    let plan = preview(&project, &req, &[], &safe).unwrap();
    project.apply_search_replace(&plan, &[]).unwrap();
    let after = &texts(&project)[0];
    assert!(after.contains("Goodbye 世界 Goodbye {count}"));
    assert!(after.contains("[[character:hero|Hello]] #line:hello"));
    assert!(after.contains("say hero \"Goodbye {count}\" direction \"Hello private\""));
}

#[test]
fn duplicate_missing_identity_and_forged_selection_fields_are_rejected() {
    let ws = Workspace::new();
    let project = ws.open();
    let req = one_file();
    let hits = project.search_drafts(&req, &[]).unwrap();
    assert!(preview(&project, &req, &[], &[hits[0].clone(), hits[0].clone()]).is_err());
    for alteration in 0..10 {
        let mut hit = hits[0].clone();
        match alteration {
            0 => hit.range = hits[1].range.clone(),
            1 => hit.range = WORLD.len()..WORLD.len() + 5,
            2 => hit.identity = None,
            3 => hit.identity = hits[1].identity.clone(),
            4 => hit.context = None,
            5 => hit.context.as_mut().unwrap().highlight = 0..0,
            6 => hit.context.as_mut().unwrap().text.push('!'),
            7 => hit.preview.push('!'),
            8 => hit.replaceable = false,
            _ => hit.column += 1,
        }
        assert!(preview(&project, &req, &[], &[hit]).is_err(), "alteration {alteration}");
    }
    assert_eq!(texts(&project), [WORLD, SECOND]);
}

#[test]
fn public_plan_changes_hits_and_occurrences_cannot_change_authorized_selection() {
    let ws = Workspace::new();
    let mut project = ws.open();
    let req = one_file();
    let hits = project.search_drafts(&req, &[]).unwrap();
    let plan = preview(&project, &req, &[], &hits[..2]).unwrap();
    let subset = preview(&project, &req, &[], &hits[1..2]).unwrap();
    for alteration in 0..9 {
        let mut changed = plan.clone();
        match alteration {
            0 => { changed.hits.remove(0); }
            1 => changed.hits[0].identity = None,
            2 => changed.changes[0].after.push_str("  injected\n"),
            3 => changed.changes[0].count = 1,
            4 => changed.occurrences.clear(),
            5 => changed.occurrences[0].after_range = 0..0,
            6 => changed.occurrences[0].before_context.text.push('!'),
            7 => {
                changed.hits = subset.hits.clone();
                changed.changes = subset.changes.clone();
                changed.occurrences = subset.occurrences.clone();
            }
            _ => { changed.changes.clear(); }
        }
        assert!(project.apply_search_replace(&changed, &[]).is_err(), "alteration {alteration}");
        assert_eq!(texts(&project), [WORLD, SECOND]);
        let original = buffer(&project);
        assert!(project.replace_search_draft(&changed, &[], &original).is_err());
    }
    project.apply_search_replace(&plan, &[]).unwrap();
}

#[test]
fn query_options_scope_file_order_and_captured_range_bind_selection_identity() {
    let ws = Workspace::new();
    let project = ws.open();
    let req = request();
    let hits = project.search_drafts(&req, &[]).unwrap();
    for alteration in 0..7 {
        let mut changed = req.clone();
        match alteration {
            0 => changed.query = "Hell".into(),
            1 => changed.options.case_sensitive = false,
            2 => changed.options.whole_word = true,
            3 => changed.scope = SearchScope::Source,
            4 => changed.files[0].range = Some(0..WORLD.len()),
            5 => changed.files.reverse(),
            _ => changed.files.truncate(1),
        }
        assert!(preview(&project, &changed, &[], &hits[..1]).is_err(), "alteration {alteration}");
    }
    assert_eq!(texts(&project), [WORLD, SECOND]);
}

#[test]
fn replacement_change_requires_a_new_plan_even_when_original_hits_remain_valid() {
    let ws = Workspace::new();
    let mut project = ws.open();
    let mut req = one_file();
    let hits = project.search_drafts(&req, &[]).unwrap();
    let mut first = preview(&project, &req, &[], &hits[..1]).unwrap();
    req.replacement = "Welcome".into();
    let second = preview(&project, &req, &[], &hits[..1]).unwrap();
    assert_eq!(first.request().replacement, "Goodbye");
    first.hits = second.hits.clone();
    first.changes = second.changes.clone();
    first.occurrences = second.occurrences.clone();
    assert!(project.apply_search_replace(&first, &[]).is_err());
    assert_eq!(texts(&project), [WORLD, SECOND]);
    project.apply_search_replace(&second, &[]).unwrap();
    assert!(texts(&project)[0].contains("Welcome 世界 Hello"));
}

#[test]
fn newly_opened_clean_navigation_buffer_keeps_existing_hits_and_plan_valid() {
    let ws = Workspace::new();
    let mut project = ws.open();
    let req = one_file();
    let hits = project.search_drafts(&req, &[]).unwrap();
    let plan = preview(&project, &req, &[], &hits[..1]).unwrap();
    let opened = buffer(&project);
    assert_eq!(opened.generation(), 0);
    assert!(!opened.is_changed());
    let drafts = [opened.clone()];
    let after_navigation = preview(&project, &req, &drafts, &hits[..1]).unwrap();
    assert_eq!(after_navigation, plan);
    let next = project.replace_search_draft(&plan, &drafts, &opened).unwrap();
    assert!(next.source().contains("Goodbye 世界 Hello"));
    assert_eq!(opened.source(), WORLD);
    project.apply_search_replace(&plan, &drafts).unwrap();
}

#[test]
fn edited_and_returned_to_original_draft_invalidates_old_hits_and_plan() {
    for initially_changed in [false, true] {
        let ws = Workspace::new();
        let project = ws.open();
        let mut original = buffer(&project);
        if initially_changed {
            original.replace_source(WORLD.replace("世界", "草稿"));
        }
        let req = one_file();
        let hits = project.search_drafts(&req, std::slice::from_ref(&original)).unwrap();
        let plan = preview(&project, &req, std::slice::from_ref(&original), &hits[..1]).unwrap();
        let mut edited = original.clone();
        edited.replace_source(edited.source().replace("世界", "changed").replace("草稿", "changed"));
        assert!(preview(&project, &req, std::slice::from_ref(&edited), &hits[..1]).is_err());
        edited.replace_source(original.source().to_owned());
        assert_eq!(edited.source(), original.source());
        assert_eq!(edited.generation(), original.generation() + 2);
        assert!(preview(&project, &req, std::slice::from_ref(&edited), &hits[..1]).is_err());
        assert!(project.replace_search_draft(&plan, std::slice::from_ref(&edited), &edited).is_err());
        // 传旧草稿列表也不能将旧事务套到内容相同但代次不同的缓冲。
        assert!(project.replace_search_draft(&plan, std::slice::from_ref(&original), &edited).is_err());
        assert_eq!(texts(&project), [WORLD, SECOND]);
        let fresh = project.search_drafts(&req, std::slice::from_ref(&edited)).unwrap();
        assert!(preview(&project, &req, &[edited], &fresh[..1]).is_ok());
    }
}

#[test]
fn unselected_project_change_and_refresh_round_trip_invalidate_old_identity() {
    let ws = Workspace::new();
    let mut project = ws.open();
    let req = one_file();
    let hits = project.search_drafts(&req, &[]).unwrap();
    let plan = preview(&project, &req, &[], &hits[..1]).unwrap();
    let changed = SECOND.replace("again", "outside");
    project.set_text(&ws.0.join("second.wl"), changed.clone()).unwrap();
    assert!(preview(&project, &req, &[], &hits[..1]).is_err());
    assert!(project.apply_search_replace(&plan, &[]).is_err());
    project.set_text(&ws.0.join("second.wl"), SECOND.into()).unwrap();
    fs::write(ws.0.join("second.wl"), changed).unwrap();
    assert!(project.refresh().unwrap().is_empty());
    fs::write(ws.0.join("second.wl"), SECOND).unwrap();
    assert!(project.refresh().unwrap().is_empty());
    assert_eq!(texts(&project), [WORLD, SECOND]);
    assert!(preview(&project, &req, &[], &hits[..1]).is_err());
    assert!(project.apply_search_replace(&plan, &[]).is_err());
}

#[test]
fn cancelling_preview_and_invalid_drafts_preserve_all_original_state() {
    let ws = Workspace::new();
    let project = ws.open();
    let mut draft = buffer(&project);
    draft.replace_source(WORLD.replace("世界", "尚未应用"));
    let req = one_file();
    let source = draft.source().to_owned();
    let generation = draft.generation();
    let baseline = project.content_baseline();
    let hits = project.search_drafts(&req, std::slice::from_ref(&draft)).unwrap();
    let plan = preview(&project, &req, std::slice::from_ref(&draft), &hits[..1]).unwrap();
    drop(plan);
    assert_eq!(draft.source(), source);
    assert_eq!(draft.generation(), generation);
    assert_eq!(project.content_baseline(), baseline);
    draft.replace_source("event start\n  Hello {unfinished\n".into());
    let mut req = req;
    req.scope = SearchScope::Source;
    let hits = project.search_drafts(&req, std::slice::from_ref(&draft)).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(!hits[0].replaceable);
    let invalid_source = draft.source().to_owned();
    let invalid_generation = draft.generation();
    assert!(preview(&project, &req, std::slice::from_ref(&draft), &hits).is_err());
    assert_eq!(draft.source(), invalid_source);
    assert_eq!(draft.generation(), invalid_generation);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(texts(&project), [WORLD, SECOND]);
    assert_eq!(fs::read_to_string(ws.0.join("world.wl")).unwrap(), WORLD);
}

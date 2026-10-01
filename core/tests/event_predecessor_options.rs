use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::authoring::{EventDraft, EventPredecessorOption, EventPredecessorOptions};
use worldline_core::project::Project;
use worldline_core::timeline::TemporalOrderScope;

const SOURCE: &str = r#"period year as "风暴年"
period summer as "长潮季" within year
period autumn as "退潮季" within year
period early as "初秋" within autumn
period other as "另一纪年"
event arrival as "同名事件" during summer at 70
  -> archive
event archive as "同名事件" during early follows arrival at 20
  -> tower
event tower as "旧潮塔" during autumn follows archive at 90
  -> END
event free as "待定史事" during early
  -> END
event root_event as "年内史事" during year
  -> END
event remote during other
  -> END
event undated
  -> END
"#;

fn project(version: &str, source: &str) -> Project {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "wl-predecessor-options-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.documents.retain(|path, _| path == &entry);
    project.set_text(&entry, source.into()).unwrap();
    project
        .create_authoring_document(
            &root.join(".world/project.json"),
            format!(
                r#"{{"schema_version":1,"language_version":"{version}","required_features":[]}}"#
            )
            .into_bytes(),
        )
        .unwrap();
    project
}

fn options(project: &Project, draft: &EventDraft) -> EventPredecessorOptions {
    project.event_predecessor_options(
        &project.entry,
        Some(&draft.id),
        draft,
        &project.content_baseline(),
    )
}

fn entry<'a>(options: &'a EventPredecessorOptions, id: &str) -> &'a EventPredecessorOption {
    options.entries.iter().find(|entry| entry.id == id).unwrap()
}

fn unchanged(project: &mut Project, draft: &EventDraft) -> String {
    let baseline = project.content_baseline();
    let sources = project.sources();
    let path = project.entry.clone();
    let error = project
        .write_event(&path, Some(&draft.id), draft)
        .unwrap_err();
    assert_eq!(project.sources(), sources);
    assert_eq!(project.content_baseline(), baseline);
    error
}

#[test]
fn explicit_113_projects_same_root_siblings_ancestors_and_complete_selected_identity() {
    let project = project("1.13", SOURCE);
    let (_, draft) = project.event_draft("archive").unwrap();
    let baseline = project.content_baseline();
    let projection = options(&project, &draft);
    assert_eq!(projection.order_scope, TemporalOrderScope::RootPeriod);
    assert_eq!(projection.period.as_deref(), Some("early"));
    assert_eq!(projection.root.as_deref(), Some("year"));
    assert!(projection.blocked_reason.is_none());
    let arrival = entry(&projection, "arrival");
    assert!(arrival.selected && arrival.rejection.is_none());
    assert_eq!(arrival.display, "同名事件");
    assert_eq!(arrival.period.as_deref(), Some("summer"));
    assert_eq!(arrival.root.as_deref(), Some("year"));
    assert_eq!(arrival.file.as_deref(), project.entry.to_str());
    assert_eq!(arrival.line, Some(6));
    assert!(entry(&projection, "free").rejection.is_none());
    assert!(entry(&projection, "root_event").rejection.is_none());
    for id in ["archive", "tower", "remote", "undated"] {
        assert!(entry(&projection, id).rejection.is_some(), "{id}");
    }
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(draft.predecessors, ["arrival"]);
    assert!(projection.entries.windows(2).all(|w| w[0].id < w[1].id));
}

#[test]
fn old_language_versions_retain_direct_period_scope_and_do_not_upgrade() {
    let source = SOURCE
        .replace(" follows arrival", "")
        .replace(" follows archive", "");
    for version in ["1.9", "1.10", "1.11", "1.12"] {
        let mut project = project(version, &source);
        let (_, mut draft) = project.event_draft("archive").unwrap();
        draft.predecessors.push("arrival".into());
        let projection = options(&project, &draft);
        assert_eq!(projection.order_scope, TemporalOrderScope::DirectPeriod);
        assert!(projection.blocked_reason.is_none());
        assert!(entry(&projection, "free").rejection.is_none());
        assert!(entry(&projection, "arrival").selected);
        assert!(entry(&projection, "arrival").rejection.is_some());
        assert!(entry(&projection, "root_event").rejection.is_some());
        assert!(unchanged(&mut project, &draft).contains("A213"));
        assert_eq!(project.language_version(), version);
    }
}

#[test]
fn period_changes_keep_selected_edges_and_leave_runtime_and_narrative_order_unchanged() {
    let mut project = project("1.13", SOURCE);
    let before = project.compile();
    let (_, mut draft) = project.event_draft("archive").unwrap();
    draft.period = Some("summer".into());
    let projection = options(&project, &draft);
    assert!(entry(&projection, "arrival").selected);
    assert!(entry(&projection, "arrival").rejection.is_none());
    let baseline = project.content_baseline();
    let path = project.entry.clone();
    project
        .write_event_at_baseline(&path, Some("archive"), &draft, &baseline)
        .unwrap();
    let after = project.compile();
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    assert_eq!(before.program.entry, after.program.entry);
    let order = |result: &worldline_core::CompileResult| {
        result
            .program
            .events
            .iter()
            .map(|event| (event.name.clone(), event.order, event.storyline.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&before), order(&after));
    assert_eq!(after.analysis.timeline.edges.len(), 2);
    assert_eq!(
        project.event_draft("archive").unwrap().1.predecessors,
        ["arrival"]
    );
    // 之后只编辑正文、标题也不能丢掉既有跨时段关系。
    let (_, mut tower) = project.event_draft("tower").unwrap();
    tower.summary = "改名后的旧塔".into();
    tower.body = "重写正文\n-> END".into();
    project.write_event(&path, Some("tower"), &tower).unwrap();
    assert_eq!(
        project.event_draft("tower").unwrap().1.predecessors,
        ["archive"]
    );
}

#[test]
fn selected_invalid_missing_self_and_cyclic_edges_are_visible_and_rejected_atomically() {
    let mut project = project("1.13", SOURCE);
    let (_, mut draft) = project.event_draft("archive").unwrap();
    draft.predecessors = [
        "arrival", "missing", "archive", "tower", "remote", "undated",
    ]
    .map(str::to_owned)
    .into();
    let projection = options(&project, &draft);
    for id in &draft.predecessors {
        let option = entry(&projection, id);
        assert!(option.selected);
        if id != "arrival" {
            assert!(option.rejection.is_some(), "{id}");
        }
    }
    let missing = entry(&projection, "missing");
    assert_eq!(missing.display, "missing");
    assert!(missing.file.is_none() && missing.line.is_none());
    assert!(missing.period.is_none() && missing.root.is_none());
    assert!(entry(&projection, "tower")
        .rejection
        .as_ref()
        .unwrap()
        .contains("环"));
    for id in ["missing", "archive", "tower", "remote", "undated"] {
        draft.predecessors = vec![id.into()];
        assert!(unchanged(&mut project, &draft).contains("A213"));
    }
    for period in [None, Some("other".into()), Some("missing".into())] {
        draft.period = period;
        draft.predecessors = vec!["arrival".into()];
        let projection = options(&project, &draft);
        assert!(entry(&projection, "arrival").selected);
        assert!(entry(&projection, "arrival").rejection.is_some());
        assert!(unchanged(&mut project, &draft).contains("A213"));
    }
}

#[test]
fn full_candidate_compile_checks_successors_in_other_files_even_after_explicit_unselect() {
    let mut project = project("1.13", SOURCE);
    let entry_path = project.entry.clone();
    let second = project.add_file(Path::new("chapters/ending.wl")).unwrap();
    let source = project.document(&entry_path).unwrap().replace(
        "event tower as \"旧潮塔\" during autumn follows archive at 90\n  -> END\n",
        "",
    );
    project.set_text(&entry_path, source).unwrap();
    project
        .set_text(
            &second,
            "event tower during autumn follows archive at 90\n  -> END\n".into(),
        )
        .unwrap();
    assert!(!project.compile().has_errors());
    let (_, mut draft) = project.event_draft("archive").unwrap();
    draft.period = Some("other".into());
    draft.predecessors.clear(); // 作者明确取消入边，仍不可破坏另一文件的后继。
    assert!(unchanged(&mut project, &draft).contains("A213"));
    draft.period = None;
    assert!(unchanged(&mut project, &draft).contains("A213"));
}

#[test]
fn stale_baselines_partial_analysis_unknown_versions_and_readonly_workspaces_block_all_options() {
    let mut project = project("1.13", SOURCE);
    let (path, draft) = project.event_draft("archive").unwrap();
    let baseline = project.content_baseline();
    project
        .set_text(&path, format!("{SOURCE}\n// 新内容\n"))
        .unwrap();
    let stale = project.event_predecessor_options(&path, Some("archive"), &draft, &baseline);
    assert!(stale.blocked_reason.unwrap().contains("过期"));
    assert!(stale.entries.iter().all(|entry| entry.rejection.is_some()));
    let current = project.content_baseline();
    assert!(project
        .write_event_at_baseline(&path, Some("archive"), &draft, &baseline)
        .is_err());
    assert_eq!(current, project.content_baseline());
    project
        .set_text(&path, format!("{SOURCE}\nevent broken\n  -> absent\n"))
        .unwrap();
    let partial = options(&project, &draft);
    assert!(partial.blocked_reason.unwrap().contains("不完整"));
    assert!(partial
        .entries
        .iter()
        .all(|entry| entry.rejection.is_some()));
    assert!(entry(&options(&project, &draft), "arrival").selected);
    assert!(unchanged(&mut project, &draft).contains("不完整"));
    for manifest in [
        r#"{"schema_version":1,"language_version":"9.99","required_features":[]}"#,
        r#"{"schema_version":1,"language_version":"1.13","required_features":["unknown.required.v1"]}"#,
    ] {
        let mut readonly = super_project(manifest);
        let projection = options(&readonly, &draft);
        assert!(projection.blocked_reason.unwrap().contains("只读"));
        assert!(projection
            .entries
            .iter()
            .all(|entry| entry.rejection.is_some()));
        assert!(unchanged(&mut readonly, &draft).contains("只读"));
        std::fs::remove_dir_all(&readonly.root).unwrap();
    }
}

fn super_project(manifest: &str) -> Project {
    let project = project("1.13", SOURCE);
    std::fs::create_dir_all(project.root.join(".world")).unwrap();
    std::fs::write(&project.entry, SOURCE).unwrap();
    std::fs::write(project.root.join(".world/project.json"), manifest).unwrap();
    Project::open(&project.root).unwrap()
}

#[test]
fn outside_root_inactive_or_wrong_identity_requests_never_modify_sources() {
    let mut project = project("1.13", SOURCE);
    let (_, draft) = project.event_draft("archive").unwrap();
    let baseline = project.content_baseline();
    for path in [
        project.root.join("../outside.wl"),
        project.root.join("absent.wl"),
    ] {
        let projection =
            project.event_predecessor_options(&path, Some("archive"), &draft, &baseline);
        assert!(projection.blocked_reason.is_some());
        assert!(projection
            .entries
            .iter()
            .all(|entry| entry.rejection.is_some()));
        assert!(project.write_event(&path, Some("archive"), &draft).is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
    let wrong_id = EventDraft {
        id: "changed".into(),
        ..draft.clone()
    };
    assert!(options(&project, &wrong_id).blocked_reason.is_some());
    let file = project.add_file(Path::new("inactive.wl")).unwrap();
    let source = project
        .document(&project.entry)
        .unwrap()
        .replace("include \"inactive.wl\"", "");
    project.set_text(&project.entry.clone(), source).unwrap();
    let manifest = project.root.join(".world/project.json");
    project.set_authoring_document(&manifest, br#"{"schema_version":1,"language_version":"1.13","required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":["world.wl"],"archived":["inactive.wl"]}}"#.to_vec()).unwrap();
    let new = EventDraft {
        id: "new_event".into(),
        storyline: "main".into(),
        body: "-> END".into(),
        ..EventDraft::default()
    };
    let baseline = project.content_baseline();
    let projection = project.event_predecessor_options(&file, None, &new, &baseline);
    assert!(projection.blocked_reason.unwrap().contains("活动"));
    assert!(project.write_event(&file, None, &new).is_err());
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn new_event_projection_and_single_snapshot_undo_redo_preserve_all_edges_after_reopen() {
    let mut project = project("1.13", SOURCE);
    let path = project.entry.clone();
    let draft = EventDraft {
        id: "new_event".into(),
        storyline: "main".into(),
        period: Some("year".into()),
        predecessors: vec!["tower".into()],
        body: "-> END".into(),
        ..EventDraft::default()
    };
    let baseline = project.content_baseline();
    let projection = project.event_predecessor_options(&path, None, &draft, &baseline);
    assert!(projection.blocked_reason.is_none());
    assert!(entry(&projection, "tower").selected);
    assert!(entry(&projection, "tower").rejection.is_none());
    let mut self_reference = draft.clone();
    self_reference.predecessors = vec![self_reference.id.clone()];
    let projection = project.event_predecessor_options(&path, None, &self_reference, &baseline);
    assert!(entry(&projection, "new_event")
        .rejection
        .as_ref()
        .unwrap()
        .contains("自身"));
    assert!(project.write_event(&path, None, &self_reference).is_err());
    assert_eq!(project.content_baseline(), baseline);
    let before = project.clone();
    project
        .write_event_at_baseline(&path, None, &draft, &baseline)
        .unwrap();
    let after = project.clone();
    assert!(project.restore(before));
    assert_eq!(project.content_baseline(), baseline);
    assert!(project.restore(after));
    let edges = serde_json::to_value(&project.compile().analysis.timeline.edges).unwrap();
    project.save().unwrap();
    let mut reopened = Project::open(&project.root).unwrap();
    assert_eq!(
        serde_json::to_value(&reopened.compile().analysis.timeline.edges).unwrap(),
        edges
    );
    std::fs::remove_dir_all(&project.root).unwrap();
}

#[test]
fn every_candidate_agrees_with_full_compilation_including_transitive_cycles() {
    let source = "period root\nevent a during root\n  -> END\nevent b during root follows a\n  -> END\nevent c during root follows b\n  -> END\nevent d during root follows c\n  -> END\nevent independent during root\n  -> END\n";
    for version in ["1.9", "1.10", "1.11", "1.12", "1.13"] {
        let project = project(version, source);
        for id in ["a", "b", "c", "d", "independent"] {
            let (_, draft) = project.event_draft(id).unwrap();
            for option in options(&project, &draft).entries {
                let mut candidate = project.clone();
                let mut changed = draft.clone();
                changed.predecessors.push(option.id.clone());
                let accepted = candidate
                    .write_event(&project.entry, Some(id), &changed)
                    .is_ok();
                assert_eq!(
                    accepted,
                    option.rejection.is_none(),
                    "{version} {id} <- {}",
                    option.id
                );
            }
        }
    }
}

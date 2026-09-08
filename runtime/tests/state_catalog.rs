use worldline_core::catalog::TargetRef;
use worldline_core::compile_source;
use worldline_core::project::Project;
use worldline_core::states::StateDraft;

const WORLD: &str = r#"
tag calm as "平静"
tag alert as "警觉"
tag place as "地点"
character lin as "林舟"
state mood on character lin with calm as "心境"
state weather on tag place with [] as "天气"
mark state mood with place
period night
event a during night after has(mood, calm)
  effect on enter
    become mood with alert, calm, alert as "两种感受"
  effect on exit if has(mood, alert)
    become mood with []
  choice "观察"
    become weather with alert
    -> END
event b during night
  if has(mood, alert)
    become mood with calm
  -> END
"#;

#[test]
fn states_index_whole_targets_and_conditional_changes_without_ordering_events() {
    let result = compile_source("world.wl", WORLD);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let catalog = &result.analysis.catalog;
    let mood = &catalog.states["mood"];
    assert_eq!(mood.target, TargetRef::new("character", "lin"));
    assert_eq!(mood.tags, ["calm"]);
    assert_eq!(mood.changes.len(), 3);
    assert_eq!(mood.changes[0].timing, "enter");
    assert_eq!(mood.changes[0].tags, ["alert", "calm"]);
    assert_eq!(mood.changes[1].timing, "exit");
    assert!(mood.changes[1].tags.is_empty());
    assert!(!mood.changes[1].contexts.is_empty());
    assert!(!mood.changes[2].contexts.is_empty());
    assert_eq!(
        catalog.states["weather"].target,
        TargetRef::new("tag", "place")
    );
    assert!(!catalog.states["weather"].changes[0].contexts.is_empty());
    assert!(catalog
        .query("place", true)
        .iter()
        .any(|o| o.target == TargetRef::new("state", "mood")));
    assert!(result.analysis.timeline.edges.is_empty());
    assert!(catalog
        .references_to(&TargetRef::new("state", "mood"))
        .iter()
        .any(|r| r.source == TargetRef::new("event", "a")));
}

#[test]
fn invalid_states_and_malformed_predicates_are_diagnostics() {
    for source in [
        "state s on tag nowhere with []\nevent a\n  -> END",
        "tag t\nstate s on tag t with missing\nevent a\n  -> END",
        "tag t\nstate s on tag t with []\nstate s on tag t with []\nevent a\n  -> END",
        "event a\n  become missing with []\n  -> END",
        "event a after has()\n  -> END",
        "event a after has(1, true)\n  -> END",
        "event a after has(s, t, z)\n  -> END",
        "event a\n  become s with\n  -> END",
    ] {
        let result = compile_source("invalid.wl", source);
        assert!(result.has_errors(), "错误文本不应通过: {source}");
    }
}

#[test]
fn state_initial_values_and_changes_affect_fingerprints_but_tags_remain_metadata() {
    let fingerprint = |text: &str| compile_source("world.wl", text).analysis.fingerprint;
    assert_ne!(
        fingerprint(WORLD),
        fingerprint(&WORLD.replace("with calm as", "with alert as"))
    );
    assert_ne!(
        fingerprint(WORLD),
        fingerprint(&WORLD.replace("become mood with []", "become mood with calm"))
    );
    assert_eq!(
        fingerprint(WORLD),
        fingerprint(&WORLD.replace("tag calm as \"平静\"", "tag calm as \"安静\""))
    );
}

#[test]
fn state_form_edits_preserve_identity_and_rollback_broken_references() {
    let root = std::env::temp_dir().join(format!("worldline-state-draft-{}", std::process::id()));
    let mut project = Project::new(&root);
    project.documents.retain(|path, _| path == &project.entry);
    project
        .set_text(&project.entry.clone(), WORLD.into())
        .unwrap();
    let mut draft = StateDraft {
        id: "mood".into(),
        display: "新的心境".into(),
        target: TargetRef::new("tag", "place"),
        tags: vec!["alert".into()],
    };
    project
        .edit(|p| p.write_state(Some("mood"), &draft))
        .unwrap();
    let result = project.compile();
    assert_eq!(result.analysis.catalog.states["mood"].display, "新的心境");
    assert_eq!(result.analysis.catalog.states["mood"].changes.len(), 3);
    let before = project.sources();
    draft.tags = vec!["missing".into()];
    assert!(project
        .edit(|p| p.write_state(Some("mood"), &draft))
        .is_err());
    assert_eq!(project.sources(), before);
    draft.id = "new_id".into();
    assert!(project
        .edit(|p| p.write_state(Some("mood"), &draft))
        .is_err());
}

#[test]
fn exported_world_preserves_state_ids_and_relocates_file_targets() {
    let root = std::env::temp_dir().join(format!(
        "worldline-state-export-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut project = Project::new(&root.join("source"));
    let source_file = project.root.join("events/harbor.wl");
    project
        .edit(|p| {
            p.write_state(
                None,
                &StateDraft {
                    id: "file_status".into(),
                    display: "港口稿件进度".into(),
                    target: TargetRef::new("file", &source_file.to_string_lossy()),
                    tags: vec!["ready".into()],
                },
            )
        })
        .unwrap();
    let before = project.compile();
    let output = root.join("export");
    project.export(&output).unwrap();
    let reopened = worldline_core::compile_path(&output).unwrap();
    assert!(!reopened.has_errors(), "{:?}", reopened.diagnostics);
    assert_eq!(before.analysis.fingerprint, reopened.analysis.fingerprint);
    assert_eq!(reopened.analysis.catalog.states.len(), 4);
    assert_eq!(
        reopened.analysis.catalog.states["lin_mood"].changes.len(),
        2
    );
    assert_eq!(
        reopened.analysis.catalog.states["file_status"]
            .target
            .id
            .replace('\\', "/"),
        output
            .join("events/harbor.wl")
            .to_string_lossy()
            .replace('\\', "/")
    );
    assert!(output.join("world.wl").is_file());
    assert!(!output.join("spec/README.md").exists());
    assert!(root.is_absolute() && root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}

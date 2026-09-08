//! 效果表单往返源码时保留语义、条件、作者注释和正文结构。
use worldline_core::ast::EffectWhen;
use worldline_core::authoring::{CharacterDraft, EffectDraft, EventDraft};
use worldline_core::project::Project;
use worldline_core::CompileResult;
use worldline_runtime::Story;

fn project(source: &str) -> Project {
    // 仅使用内存文档，不写入共享工作区或临时文件。
    let mut project = Project::new(&std::env::temp_dir().join("worldline-effect-authoring"));
    let path = project.entry.clone();
    project.documents.retain(|p, _| p == &path);
    project.set_text(&path, source.into()).unwrap();
    checked(&mut project);
    project
}

fn checked(project: &mut Project) -> CompileResult {
    let result = project.compile();
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    result
}

#[test]
fn effect_drafts_roundtrip_comments_conditions_and_done() {
    let source = r#"character lin as "林舟"
storyline harbor
    event start
        grant ticket
        -> arrival
    event arrival as "抵港" with lin perm ticket after perm(ticket) // 事件说明
        // 正文说明：effect on enter 只是注释
        抵达港口。
        effect on enter if perm(ticket) /* 条件旁注 */ and not perm(early) // 前置说明
            // 动作说明
            grant early as "http://港口/*原文*/" // 授权说明
        if true
            缩进正文。
        effect on done if perm(early) // 自然完成说明
            /* 多行注释
               effect on enter
            */
            grant finished
        effect on enter if perm(early)
            grant second
        尾声。
    event other
        -> END
"#;
    let mut project = project(source);
    let before = checked(&mut project);
    let (path, draft) = project.event_draft("arrival").unwrap();
    assert_eq!(draft.perm, "ticket");
    assert_eq!(draft.after, "perm(ticket)");
    assert_eq!(draft.effects.len(), 3);
    assert_eq!(draft.effects[0].when, EffectWhen::Enter);
    assert_eq!(draft.effects[1].when, EffectWhen::Done);
    assert_eq!(draft.effects[1].condition, "perm(early)");
    assert_eq!(draft.effects[2].when, EffectWhen::Enter);
    assert!(draft.body.contains("if true\n    缩进正文。"));
    assert!(draft.body.contains("// 正文说明：effect on enter 只是注释"));
    assert!(!draft.body.contains("grant early"));
    assert!(!draft.body.contains("effect on done"));
    assert!(!draft.body.contains("event other"));
    project
        .edit(|p| p.write_event(&path, Some("arrival"), &draft))
        .unwrap();
    let after = checked(&mut project);
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    for comment in [
        "事件说明",
        "正文说明",
        "条件旁注",
        "前置说明",
        "动作说明",
        "授权说明",
        "自然完成说明",
        "多行注释",
    ] {
        assert_eq!(
            project.document(&path).unwrap().matches(comment).count(),
            1,
            "{comment}"
        );
    }
    assert!(project
        .document(&path)
        .unwrap()
        .contains("            缩进正文。"));
    let normalized = project.document(&path).unwrap().to_string();
    let (_, draft) = project.event_draft("arrival").unwrap();
    project
        .edit(|p| p.write_event(&path, Some("arrival"), &draft))
        .unwrap();
    assert_eq!(project.document(&path).unwrap(), normalized);
    let mut story = Story::new(&after.program, &after.analysis).unwrap();
    story.continue_story().unwrap();
    assert_eq!(story.perm_list(), ["early", "finished", "second", "ticket"]);
}

#[test]
fn state_effects_roundtrip_and_preserve_execution_order() {
    let mut project = project(
        r#"tag calm as "平静"
tag alert as "警觉"
character lin as "林舟"
state mood on character lin with calm as "心境"
event arrival with lin after has(mood, calm)
  effect on enter if has(mood, calm)
    become mood with alert as "听见警报" // 状态说明
  effect on done if has(mood, alert)
    become mood with calm
  effect on exit if has(mood, calm)
    become mood with [] as "离开港口"
  林舟进入港口。
"#,
    );
    let before = checked(&mut project);
    let (path, draft) = project.event_draft("arrival").unwrap();
    assert_eq!(draft.after, "has(mood, calm)");
    assert_eq!(draft.effects[2].when, EffectWhen::Exit);
    assert_eq!(draft.effects[0].condition, "has(mood, calm)");
    assert!(draft.effects[0].actions.contains("become mood with alert"));
    assert!(!draft.body.contains("become"));
    project
        .edit(|p| p.write_event(&path, Some("arrival"), &draft))
        .unwrap();
    let after = checked(&mut project);
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    let mut original_story = Story::new(&before.program, &before.analysis).unwrap();
    let mut edited_story = Story::new(&after.program, &after.analysis).unwrap();
    original_story.continue_story().unwrap();
    edited_story.continue_story().unwrap();
    assert_eq!(original_story.states(), edited_story.states());
    assert_eq!(original_story.state_history(), edited_story.state_history());
    assert_eq!(edited_story.state_history().len(), 3);
    assert!(edited_story.states()["mood"].is_empty());
}

#[test]
fn add_edit_and_remove_effects_without_duplicate_body_actions() {
    let mut project = project("event start\n  正文。\n  -> END\n");
    let (path, mut draft) = project.event_draft("start").unwrap();
    draft.effects = vec![
        EffectDraft {
            when: EffectWhen::Enter,
            condition: "false".into(),
            actions: "grant early".into(),
        },
        EffectDraft {
            when: EffectWhen::Done,
            condition: String::new(),
            actions: "grant natural".into(),
        },
        EffectDraft {
            when: EffectWhen::Exit,
            condition: "not perm(early)".into(),
            actions: "grant left".into(),
        },
    ];
    project
        .edit(|p| p.write_event(&path, Some("start"), &draft))
        .unwrap();
    let result = checked(&mut project);
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    story.continue_story().unwrap();
    assert_eq!(story.perm_list(), ["left"]); // END 触发 exit，不触发 done。
    let (_, mut draft) = project.event_draft("start").unwrap();
    assert_eq!(draft.effects.len(), 3);
    assert!(!draft.body.contains("grant"));
    draft.effects.remove(1);
    draft.effects[0].condition = "true".into();
    project
        .edit(|p| p.write_event(&path, Some("start"), &draft))
        .unwrap();
    let result = checked(&mut project);
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    story.continue_story().unwrap();
    assert_eq!(story.perm_list(), ["early"]);
    draft.effects.clear();
    project
        .edit(|p| p.write_event(&path, Some("start"), &draft))
        .unwrap();
    assert!(project.event_draft("start").unwrap().1.effects.is_empty());
    assert!(!project.document(&path).unwrap().contains("effect on"));
}

#[test]
fn new_event_effects_validate_and_invalid_edit_is_atomic() {
    let mut project = project("event start\n  -> END\n");
    let path = project.entry.clone();
    let mut draft = EventDraft {
        id: "new_event".into(),
        storyline: "main".into(),
        effects: vec![EffectDraft {
            when: EffectWhen::Done,
            condition: "true".into(),
            actions: "grant ready".into(),
        }],
        body: "新事件。".into(),
        ..Default::default()
    };
    project
        .edit(|p| p.write_event(&path, None, &draft))
        .unwrap();
    assert_eq!(
        project.event_draft("new_event").unwrap().1.effects,
        draft.effects
    );
    let saved = project.document(&path).unwrap().to_string();
    draft.effects[0].condition = "(".into();
    assert!(project
        .edit(|p| p.write_event(&path, Some("new_event"), &draft))
        .is_err());
    assert_eq!(project.document(&path).unwrap(), saved);
    draft.effects[0].condition.clear();
    draft.effects[0].actions.clear();
    assert!(project
        .edit(|p| p.write_event(&path, Some("new_event"), &draft))
        .is_err());
    assert_eq!(project.document(&path).unwrap(), saved);
}

#[test]
fn character_rename_updates_state_target_without_renaming_other_objects() {
    let mut project = project(
        r#"tag lin as "同名标签"
character lin as "林舟"
state mood on character lin with lin as "心境" // 保留状态说明
state label on tag lin with [] as "标签自身"
event start with lin
  effect on enter
    become mood with lin
  -> END
"#,
    );
    let path = project.entry.clone();
    let draft = CharacterDraft {
        id: "zhou".into(),
        display: "林舟".into(),
        ..Default::default()
    };
    project
        .edit(|p| p.write_character(&path, Some("lin"), &draft))
        .unwrap();
    let result = checked(&mut project);
    assert_eq!(result.analysis.catalog.states["mood"].target.id, "zhou");
    assert_eq!(result.analysis.catalog.states["mood"].tags, ["lin"]);
    assert_eq!(result.analysis.catalog.states["label"].target.id, "lin");
    assert!(project.document(&path).unwrap().contains("// 保留状态说明"));
    assert_eq!(
        project.event_draft("start").unwrap().1.effects[0].actions,
        "become mood with lin"
    );
}

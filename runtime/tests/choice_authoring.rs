use worldline_core::{
    authoring::{ChoiceDraft, EventDraft},
    project::Project,
};
use worldline_runtime::Story;

#[test]
fn branch_form_edits_preserve_nested_choices_comments_and_playback() {
    let mut p = Project::new(&std::env::temp_dir().join("wl-choice-form-memory"));
    p.documents.retain(|path, _| path == &p.entry);
    p.set_text(&p.entry.clone(), "event start\n  开始。\n  choice once \"旧文案\" if true // 保留注释\n    if true\n      choice \"内层\"\n        内层正文。\n    -> END // 出口注释\n  choice \"另一路\"\n    -> END\n  汇聚正文。\n".into()).unwrap();
    let (path, mut event) = p.event_draft("start").unwrap();
    let before = event.choices();
    assert_eq!(before.len(), 3);
    let mut choice = before[0].clone();
    assert_eq!(choice.target.as_deref(), Some("END"));
    choice.label = "新决定".into();
    event.write_choice(Some(choice.line), &choice).unwrap();
    assert!(event.body.contains("保留注释"));
    assert!(event.body.contains("出口注释"));
    assert!(event.body.contains("汇聚正文。"));
    assert_eq!(event.choices()[1].label, "内层");
    p.edit(|p| p.write_event(&path, Some("start"), &event))
        .unwrap();
    let result = p.compile();
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    story.continue_story().unwrap();
    assert_eq!(story.choices()[0].label, "新决定");
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    assert_eq!(story.choices()[0].label, "内层");
}

#[test]
fn add_and_remove_branch_preserves_existing_exit_and_conditional_body() {
    let mut event = EventDraft {
        body: "结束前。\n-> END".into(),
        ..Default::default()
    };
    let choice = ChoiceDraft {
        label: "去看看".into(),
        condition: "true".into(),
        target: Some("other".into()),
        ..Default::default()
    };
    event.write_choice(None, &choice).unwrap();
    assert!(event.body.ends_with("-> END"));
    let line = event.choices()[0].line;
    event.remove_choice(line).unwrap();
    assert_eq!(event.body, "结束前。\n-> END");
}

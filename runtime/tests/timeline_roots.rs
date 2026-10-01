use worldline_core::{compile_source_with_options, CompileOptions};
use worldline_runtime::{Output, Story};

const SOURCE: &str = "period year\nperiod summer within year\nperiod autumn within year\nevent frame during autumn follows flashback\n  现在\n  choice \"回忆\"\n    -> flashback\nevent flashback during summer\n  过去\n  -> END\n";

#[test]
fn static_cross_period_order_preserves_fingerprint_saves_visits_and_flashback_execution() {
    let ordered = compile_source_with_options("world.wl", SOURCE, CompileOptions::v1_13());
    let plain_source = SOURCE
        .replace(" follows flashback", "")
        .replace("during autumn", "during summer");
    let plain = compile_source_with_options("world.wl", &plain_source, CompileOptions::v1_12());
    assert!(!ordered.has_errors(), "{:?}", ordered.diagnostics);
    assert!(!plain.has_errors(), "{:?}", plain.diagnostics);
    assert_eq!(ordered.analysis.fingerprint, plain.analysis.fingerprint);
    assert_eq!(ordered.program.entry, plain.program.entry);
    let mut previous = Story::new_with_seed(&plain.program, &plain.analysis, 42).unwrap();
    previous.continue_story().unwrap();
    let saved = previous.save().unwrap();
    let mut restored = Story::load(&ordered.program, &ordered.analysis, &saved).unwrap();
    assert_eq!(restored.choices().len(), 1);
    previous.choose(0).unwrap();
    restored.choose(0).unwrap();
    let a = previous.continue_story().unwrap();
    let b = restored.continue_story().unwrap();
    let text = |items: Vec<Output>| {
        items
            .into_iter()
            .filter_map(|o| match o {
                Output::Text { content, .. } => Some(content),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(text(a), text(b));
    assert!(restored.is_ended());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&previous.save().unwrap()).unwrap(),
        serde_json::from_str::<serde_json::Value>(&restored.save().unwrap()).unwrap()
    );
}

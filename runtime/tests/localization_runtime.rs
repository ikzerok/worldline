#[path = "localization_runtime/consumers.rs"]
mod consumers;
#[path = "localization_runtime/fixture.rs"]
mod fixture;
#[path = "localization_runtime/persistence.rs"]
mod persistence;
#[path = "localization_runtime/provenance.rs"]
mod provenance;

use fixture::*;
use serde_json::{json, Value};
use worldline_core::localization::{LocalizationPresentationPolicy as Policy, LocalizationStatus};
use worldline_runtime::{Output, Story};

const SOURCE: &str = concat!(
    "world coast\ntag visited\ncharacter lin as \"林舟\"\nstate mood on world coast with []\nlet total = 0\n",
    "rule roll(n: num) -> num = n + 1\n",
    "fragment detail(value: num)\n  Details {value} / {value} #wl-localization:detail\n  return\n",
    "fragment visit(n: num)\n  local remembered: num = n\n  call detail(remembered)\n",
    "  say lin \"Say {roll(rnd(1, 1000000) + remembered)} / {rnd(1, 1000000)} [[event:start|Harbor]]\" #wl-localization:spoken\n",
    "  choice \"Hidden {rnd(1, 1000000)}\" if false #wl-localization:hidden\n    return\n",
    "  choice once \"Go {rnd(1, 1000000)} / {rnd(1, 1000000)}\" enable true disabled \"当前可选\" #wl-localization:go\n",
    "    set total = remembered\n    become mood add visited\n    return\n",
    "  choice \"Locked {rnd(1, 1000000)}\" enable false disabled \"需要钥匙\" #wl-localization:locked\n    return\n",
    "event start\n  Source {rnd(1, 1000000)} / {rnd(1, 1000000)} #wl-localization:body\n",
    "  Glue ~ #wl-localization:glue\n  Next {total} #wl-localization:next\n",
    "  call visit(7)\n  Done {total} #wl-localization:done\n  -> END\n",
);
const IDS: &[&str] = &[
    "detail", "spoken", "hidden", "go", "locked", "body", "glue", "next", "done",
];

fn localized_fixture(name: &str) -> Fixture {
    let mut f = fixture(name, SOURCE, IDS);
    for id in ["body", "go", "detail"] {
        f.translate(
            id,
            vec![
                text("译 "),
                placeholder("p1"),
                text(" / "),
                placeholder("p0"),
            ],
        );
    }
    f.translate(
        "spoken",
        vec![
            link("l0", "港口🌙"),
            text(" "),
            placeholder("p1"),
            text(" / "),
            placeholder("p0"),
        ],
    );
    f.translate("glue", vec![]);
    f
}

fn rng(story: &Story<'_>) -> Value {
    serde_json::from_str::<Value>(&story.save().unwrap()).unwrap()["rng"].clone()
}

#[test]
fn reversed_random_tokens_materialize_once_and_keep_all_source_semantics() {
    let mut f = localized_fixture("random-reorder");
    let c = f.project.compile();
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    let snapshot = f.presentation(Policy::Strict);
    let mut source = Story::new_with_seed(&c.program, &c.analysis, 314159).unwrap();
    let mut translated =
        Story::new_with_presentation(&c.program, &c.analysis, 314159, &snapshot).unwrap();
    let source_outputs = source.continue_story().unwrap();
    let translated_outputs = translated.continue_story().unwrap();
    assert_eq!(source.state_view(), translated.state_view());
    assert_eq!(rng(&source), rng(&translated));
    assert_eq!(source_outputs.len(), translated_outputs.len());
    for (source, translated) in source_outputs.iter().zip(&translated_outputs) {
        let (
            Output::Text {
                content: source_content,
                links: source_links,
                new_line: source_new_line,
                ..
            },
            Output::Text {
                content,
                links,
                new_line,
                localization: Some(metadata),
                ..
            },
        ) = (source, translated)
        else {
            panic!("expected source/translated text")
        };
        assert_eq!(source_content, &metadata.source_content);
        assert_eq!(json!(source_links), json!(metadata.source_links));
        assert_eq!(source_new_line, new_line);
        assert_eq!(metadata.status, LocalizationStatus::Translated);
        assert_eq!(metadata.source.file, "world.wl");
        assert_eq!(
            metadata.translation_pointer.as_deref(),
            Some(
                format!(
                    "/entries/{}/translation_parts",
                    metadata.id.as_deref().unwrap()
                )
                .as_str()
            )
        );
        if metadata.id.as_deref() == Some("body") {
            let values: Vec<_> = source_content
                .trim_start_matches("Source ")
                .split(" / ")
                .collect();
            assert_ne!(
                values[0], values[1],
                "fixture must expose accidental token evaluation reversal"
            );
            assert_eq!(content, &format!("译 {} / {}", values[1], values[0]));
        }
        if metadata.id.as_deref() == Some("spoken") {
            assert_eq!(&content[links[0].start..links[0].end], "港口🌙");
            assert_eq!(links[0].start, 0);
            assert_eq!(links[0].target.id, "start");
            assert_eq!(
                &metadata.source_content
                    [metadata.source_links[0].start..metadata.source_links[0].end],
                "Harbor"
            );
        }
        if metadata.id.as_deref() == Some("glue") {
            assert!(content.is_empty());
        }
        if metadata.id.as_deref() == Some("next") {
            assert!(!new_line);
        }
    }
    assert_eq!(source.choices()[0].id, translated.choices()[0].id);
    assert_eq!(source.choice_presentations().len(), 2);
    assert_eq!(
        translated.choice_presentations()[1]
            .disabled_reason
            .as_deref(),
        Some("需要钥匙")
    );
    let before = translated.save().unwrap();
    assert!(translated.choose_presentation(1).is_err());
    for _ in 0..3 {
        let _ = translated.choice_presentations();
        let _ = translated.choice_evidence();
    }
    assert_eq!(translated.save().unwrap(), before);
    let id = source.choices()[0].id.clone();
    source.choose_id(&id).unwrap();
    translated.choose_id(&id).unwrap();
    let _ = source.continue_story().unwrap();
    let _ = translated.continue_story().unwrap();
    assert_eq!(source.state_view(), translated.state_view());
    assert_eq!(rng(&source), rng(&translated));
    assert!(source.is_ended() && translated.is_ended());
    assert_eq!(source.states()["mood"], ["visited"]);
    assert_eq!(
        translated.replay_trace().steps[0].choice.label,
        source.replay_trace().steps[0].choice.label
    );
    let identity = translated.presentation_identity().cloned();
    translated.restart().unwrap();
    source.restart().unwrap();
    assert_eq!(translated.presentation_identity(), identity.as_ref());
    translated.continue_story().unwrap();
    source.continue_story().unwrap();
    assert_eq!(source.state_view(), translated.state_view());
}

#[test]
fn fallback_outputs_keep_precise_missing_stale_invalid_and_duplicate_reasons() {
    let source = concat!(
        "event start\n  No id\n  Missing #wl-localization:missing\n",
        "  Stale #wl-localization:stale\n  Invalid {rnd(1, 20)} #wl-localization:invalid\n",
        "  Duplicate #wl-localization:dup\n  Duplicate again #wl-localization:dup\n  -> END\n"
    );
    let mut f = fixture("fallback", source, &["stale", "invalid"]);
    let mut sidecar = f.sidecar();
    sidecar["entries"]["stale"]["source_revision"] = json!("fnv1a64:0000000000000000");
    sidecar["entries"]["invalid"]["translation_parts"] = json!([]);
    f.write_sidecar(sidecar);
    assert!(f
        .project
        .prepare_localization_presentation(&request(Policy::Strict))
        .is_err());
    let mut unknown = request(Policy::SourceFallback);
    unknown.target_locale = "unknown".into();
    assert!(f
        .project
        .prepare_localization_presentation(&unknown)
        .is_err());
    let snapshot = f.presentation(Policy::SourceFallback);
    let c = f.project.compile();
    let mut source = Story::new_with_seed(&c.program, &c.analysis, 9).unwrap();
    let mut translated =
        Story::new_with_presentation(&c.program, &c.analysis, 9, &snapshot).unwrap();
    let original = source.continue_story().unwrap();
    let output = translated.continue_story().unwrap();
    let statuses: Vec<_> = output
        .iter()
        .filter_map(|o| match o {
            Output::Text {
                localization: Some(metadata),
                content,
                ..
            } => {
                assert_eq!(content, &metadata.source_content);
                Some(metadata.status)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        statuses,
        [
            LocalizationStatus::MissingId,
            LocalizationStatus::MissingTranslation,
            LocalizationStatus::StaleSource,
            LocalizationStatus::InvalidTranslation,
            LocalizationStatus::DuplicateId,
            LocalizationStatus::DuplicateId
        ]
    );
    assert_eq!(source.state_view(), translated.state_view());
    assert_eq!(original.len(), output.len());
}

#[test]
fn legacy_source_output_and_persistence_omit_every_new_field() {
    assert_eq!(
        std::mem::size_of::<Option<Box<worldline_runtime::LocalizedPresentation>>>(),
        std::mem::size_of::<usize>(),
    );
    assert!(
        std::mem::size_of::<Output>() <= 200,
        "locale metadata must not inflate every source-only output"
    );
    let mut f = localized_fixture("legacy");
    let c = f.project.compile();
    let mut source = Story::new_with_seed(&c.program, &c.analysis, 5).unwrap();
    let output = source.continue_story().unwrap();
    assert!(!serde_json::to_string(&output)
        .unwrap()
        .contains("localization"));
    for json in [
        source.save().unwrap(),
        serde_json::to_string(&source.replay_trace()).unwrap(),
        serde_json::to_string(&source.checkpoint().unwrap()).unwrap(),
    ] {
        let value: Value = serde_json::from_str(&json).unwrap();
        assert!(value.get("presentation").is_none(), "{json}");
        assert!(value.get("presentation_pause_rng").is_none(), "{json}");
        assert!(!json.contains("runtime.localization.v1"));
    }
    let snapshot = f.presentation(Policy::Strict);
    assert!(Story::load_with_presentation(
        &c.program,
        &c.analysis,
        &source.save().unwrap(),
        &snapshot
    )
    .is_err());
}

#[test]
fn incompatible_snapshot_is_rejected_before_initialization() {
    let mut f = localized_fixture("snapshot-mismatch");
    let snapshot = f.presentation(Policy::Strict);
    let path = f.root.join("world.wl");
    f.project
        .set_text(
            &path,
            SOURCE.replace("#wl-localization:body", "#wl-localization:new_body"),
        )
        .unwrap();
    let changed = f.project.compile();
    assert!(
        Story::new_with_presentation(&changed.program, &changed.analysis, 7, &snapshot).is_err()
    );
}

#[test]
fn translated_order_keeps_source_error_order_and_random_consumption() {
    let source = "let divisor = 0\nevent start\n  Values {rnd(1, 1000000)} / {1 / divisor} / {rnd(1, 1000000)} #wl-localization:line\n  -> END\n";
    let mut f = fixture("error-order", source, &["line"]);
    f.translate(
        "line",
        vec![
            placeholder("p2"),
            text(" / "),
            placeholder("p1"),
            text(" / "),
            placeholder("p0"),
        ],
    );
    let c = f.project.compile();
    let presentation = f.presentation(Policy::Strict);
    let mut source = Story::new_with_seed(&c.program, &c.analysis, 44).unwrap();
    let mut translated =
        Story::new_with_presentation(&c.program, &c.analysis, 44, &presentation).unwrap();
    let first = source.continue_story().unwrap_err();
    let second = translated.continue_story().unwrap_err();
    assert_eq!(first.message, second.message);
    assert_eq!(source.state_view(), translated.state_view());
    assert_eq!(rng(&source), rng(&translated));
}

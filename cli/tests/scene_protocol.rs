use serde_json::{json, Value};
use std::io::Cursor;
#[path = "../../core/tests/support/scene_protocol_fixture.rs"]
mod fixture;
use fixture::Fixture;

fn run(args: Vec<String>) -> Result<(i32, String), String> {
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut Cursor::new(""))?;
    Ok((code, String::from_utf8(out).unwrap()))
}
fn preview(fixture: &Fixture) -> Vec<String> {
    vec![
        "scene".into(),
        "preview".into(),
        fixture.root.display().to_string(),
        "--request-json".into(),
        serde_json::to_string(&fixture.batch()).unwrap(),
        "--json".into(),
    ]
}
fn apply(fixture: &Fixture, baseline: &str, digest: &str) -> Vec<String> {
    let mut args = preview(fixture);
    args[1] = "apply".into();
    args.extend([
        "--baseline".into(),
        baseline.into(),
        "--plan-digest".into(),
        digest.into(),
    ]);
    args
}

#[test]
fn cli_preview_matches_core_apply_saves_and_export_roundtrips() {
    let fixture = Fixture::new("cli-success");
    let before = fixture.bytes();
    let (baseline, digest, expected) = fixture.preview(&fixture.batch());
    let (code, body) = run(preview(&fixture)).unwrap();
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["plan"], expected);
    assert_eq!(value["plan_digest"], digest);
    assert_eq!(fixture.bytes(), before);
    assert_eq!(run(apply(&fixture, &baseline, &digest)).unwrap().0, 0);
    let export = run(vec![
        "scene".into(),
        "export".into(),
        fixture.root.display().to_string(),
        "--map-id=atlas".into(),
        "--json".into(),
    ])
    .unwrap();
    assert_eq!(export.0, 0);
    let value: Value = serde_json::from_str(&export.1).unwrap();
    assert!(worldline_core::svg_import::preview_scene(value["svg"].as_str().unwrap()).is_ok());
    assert!(fixture.project().map_index().maps["atlas"].scene.is_some());
}

#[test]
fn cli_stale_baseline_digest_and_repeat_apply_leave_exact_bytes() {
    let fixture = Fixture::new("cli-stale");
    let before = fixture.bytes();
    let (baseline, digest, _) = fixture.preview(&fixture.batch());
    for (base, hash) in [("old", digest.as_str()), (baseline.as_str(), "tampered")] {
        let (code, body) = run(apply(&fixture, base, hash)).unwrap();
        assert_eq!(code, 1);
        assert!(body.contains("SCENE_STALE"));
        assert_eq!(fixture.bytes(), before);
    }
    let args = apply(&fixture, &baseline, &digest);
    assert_eq!(run(args.clone()).unwrap().0, 0);
    let after = fixture.bytes();
    assert_eq!(run(args).unwrap().0, 1);
    assert_eq!(fixture.bytes(), after);
}

#[test]
fn cli_rejects_unknown_duplicate_and_inappropriate_arguments_before_writes() {
    let fixture = Fixture::new("cli-args");
    let before = fixture.bytes();
    for extra in [
        vec!["--json"],
        vec!["--source", "<svg/>"],
        vec!["--baseline", "x"],
        vec!["--request-json", "{}"],
    ] {
        let mut args = preview(&fixture);
        args.extend(extra.into_iter().map(str::to_owned));
        assert!(run(args).is_err());
    }
    let mut args = preview(&fixture);
    args[4] = "{\"map_id\":\"a\",\"map_id\":\"b\"}".into();
    assert!(run(args).is_err());
    assert_eq!(fixture.bytes(), before);
}

#[test]
fn cli_svg_profile_rejection_and_cubic_preview_use_core_dto() {
    for (source, code) in [
        (
            "<svg width='100' height='100'><path d='M0 0 Q20 20 50 60'/></svg>",
            0,
        ),
        ("<svg width='100' height='100'><svg/></svg>", 1),
        ("<svg><script>bad()</script></svg>", 1),
    ] {
        let result = run(vec![
            "scene".into(),
            "svg-preview".into(),
            "--source".into(),
            source.into(),
            "--json".into(),
        ])
        .unwrap();
        assert_eq!(result.0, code, "{}", result.1);
        let value: Value = serde_json::from_str(&result.1).unwrap();
        assert_eq!(value["ok"], json!(code == 0));
        if code == 0 {
            assert!(result.1.contains("quadratic"));
        }
    }
}

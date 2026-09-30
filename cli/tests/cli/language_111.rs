use super::common::*;
use serde_json::Value;

const SOURCE: &str = "character doctor as \"林医生\"\nworld coast\ntag map\nstate evidence on world coast with map\nrule fee(n: num) -> num = n * 2\nfragment explain(place: str)\n  local cost: num = fee(count(members(state(evidence))))\n  say doctor \"{place}费用{cost}\" direction \"不可外发的备注\"\n  return\nevent start\n  call explain(\"医院\")\n  返回\n  -> END\n";

#[test]
fn explicit_111_cli_runs_rules_fragments_sets_and_typed_speech() {
    let path = temp_story("language-111.wl", SOURCE);
    let mut output = Vec::new();
    let code = wl::run(
        &[
            "play".into(),
            path.to_string_lossy().into_owned(),
            "--language-version=1.11".into(),
            "--json".into(),
            "--seed=31".into(),
        ],
        &mut output,
        &mut std::io::Cursor::new(Vec::new()),
    )
    .unwrap();
    let text = String::from_utf8(output.clone()).unwrap();
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("不可外发的备注"));
    let rows = output_rows(&output);
    let speech = rows
        .iter()
        .find(|r| r["speaker"]["id"] == "doctor")
        .expect("speaker输出");
    assert_eq!(speech["content"], "医院费用2");
    assert!(rows.iter().any(|r| r["content"] == "返回"));
    assert!(rows.iter().any(|r| r["type"] == "ended"));
}

#[test]
fn implicit_legacy_cli_retains_new_keyword_lines_as_text() {
    let path = temp_story(
        "language-legacy-keywords.wl",
        "event start\n  call explain()\n  return\n  say doctor \"正文\"\n  -> END\n",
    );
    let mut output = Vec::new();
    assert_eq!(
        wl::run(
            &[
                "play".into(),
                path.to_string_lossy().into_owned(),
                "--json".into()
            ],
            &mut output,
            &mut std::io::Cursor::new(Vec::new())
        )
        .unwrap(),
        0
    );
    let rows = output_rows(&output);
    assert!(rows.iter().any(|r| r["content"] == "call explain()"));
    assert!(rows.iter().all(|r| r.get("speaker").is_none()));
}

fn output_rows(output: &[u8]) -> Vec<Value> {
    json_lines(output)
        .iter()
        .filter_map(|turn| turn.get("outputs").and_then(Value::as_array))
        .flatten()
        .cloned()
        .collect()
}

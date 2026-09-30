use super::*;

#[test]
fn condition_evidence_is_opt_in_actual_only_and_preserves_default_shape() {
    let source = "event start\n  choice \"blocked\" if not (true and false) and false\n    -> END\n  choice \"finish\"\n    -> END\n";
    let (_, responses) = exchange(&[
        req(1, "compile", json!({"source": source})),
        req(2, "session.open", json!({"story_id":"s1", "seed":7})),
        req(
            3,
            "session.explain_choices",
            json!({"session_id":"c1", "include_evidence":true}),
        ),
        req(4, "session.continue", json!({"session_id":"c1"})),
        req(5, "session.save", json!({"session_id":"c1"})),
        req(6, "session.explain_choices", json!({"session_id":"c1"})),
        req(
            7,
            "session.explain_choices",
            json!({"session_id":"c1", "include_evidence":true}),
        ),
        req(
            8,
            "session.explain_choices",
            json!({"session_id":"c1", "include_evidence":false}),
        ),
        req(9, "session.save", json!({"session_id":"c1"})),
        req(
            10,
            "session.explain_choices",
            json!({"session_id":"c1", "include_evidence":"yes"}),
        ),
    ]);
    assert_eq!(responses[2]["result"]["choices"], json!([]));
    let default = &responses[5]["result"];
    assert!(default["choices"][0]["condition"].get("evidence").is_none());
    assert_eq!(default, &responses[7]["result"]);
    let condition = &responses[6]["result"]["choices"][0]["condition"];
    assert_eq!(condition["evidence"]["nodes"][0]["status"], "evaluated");
    assert_eq!(condition["result"], false);
    assert!(condition["evidence"]["display_expression"]
        .as_str()
        .unwrap()
        .contains("(true and false)"));
    assert_eq!(responses[4]["result"], responses[8]["result"]);
    assert_eq!(responses[9]["error"]["code"], -32602);
}

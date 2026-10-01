use serde_json::{json, Value};
const CAPABILITY: &str = "runtime.bounded_continue.v1";
fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn exchange(requests: Vec<Value>) -> Vec<Value> {
    let input = requests
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = Vec::new();
    assert_eq!(
        worldline_agent::run(&mut std::io::Cursor::new(input), &mut output),
        0
    );
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}

#[test]
fn legacy_loop_is_a_story_failure_with_committed_state_not_a_protocol_error() {
    let rows = exchange(vec![
        request(
            1,
            "compile",
            json!({"source":"event start\n  -> next\nevent next\n  -> start\n"}),
        ),
        request(2, "session.open", json!({"story_id":"s1"})),
        request(3, "session.continue", json!({"session_id":"c1"})),
        request(4, "session.save", json!({"session_id":"c1"})),
    ]);
    assert_eq!(rows[0]["result"]["ok"], true);
    let result = &rows[2]["result"];
    assert!(rows[2].get("error").is_none());
    assert_eq!(result["ok"], false);
    assert!(result["run_error"]["message"]
        .as_str()
        .unwrap()
        .contains("预算"));
    assert_eq!(result["ended"], false);
    assert_eq!(result["paused"], false);
    assert!(result.get("outcome").is_none());
    assert!(result["state"]["visits"]["start"].as_u64().unwrap() > 1);
    assert!(rows[3]["result"]["save"].is_string());
}

#[test]
fn negotiated_budget_cancel_and_resume_deliver_partial_outputs_and_rng_once() {
    let source = "event start\n  左:{rnd(1,100)}。~\n  右:{rnd(1,100)}。\n  -> END\n";
    let rows = exchange(vec![
        request(1, "initialize", json!({})),
        request(2, "compile", json!({"source":source})),
        request(
            3,
            "session.open",
            json!({"story_id":"s1","seed":21,"capabilities":[CAPABILITY,"runtime.choice_presentation.v1"],"max_steps":1,"time_budget_ms":u64::MAX}),
        ),
        request(4, "session.continue", json!({"session_id":"c1"})),
        request(5, "session.cancel", json!({"session_id":"c1"})),
        request(6, "session.continue", json!({"session_id":"c1"})),
        request(
            7,
            "session.continue",
            json!({"session_id":"c1","max_steps":100}),
        ),
        request(8, "session.save", json!({"session_id":"c1"})),
        request(9, "session.open", json!({"story_id":"s1","seed":21})),
        request(10, "session.continue", json!({"session_id":"c2"})),
        request(11, "session.save", json!({"session_id":"c2"})),
    ]);
    assert!(rows[0]["result"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!(CAPABILITY)));
    assert_eq!(
        rows[2]["result"]["capabilities"].as_array().unwrap().len(),
        2
    );
    let partial = &rows[3]["result"];
    assert_eq!(partial["outcome"], "step_budget_exceeded");
    assert_eq!(partial["executed_steps"], 1);
    assert_eq!(partial["ok"], false);
    assert_eq!(partial["outputs"].as_array().unwrap().len(), 1);
    assert!(partial.get("choice_presentation").is_some());
    assert_eq!(rows[4]["result"]["cancel_pending"], true);
    let cancelled = &rows[5]["result"];
    assert_eq!(cancelled["outcome"], "cancelled");
    assert_eq!(cancelled["executed_steps"], 0);
    assert_eq!(cancelled["outputs"], json!([]));
    assert_eq!(cancelled["state"], partial["state"]);
    let ended = &rows[6]["result"];
    assert_eq!(ended["outcome"], "ended");
    assert_eq!(ended["outputs"][0]["new_line"], false);
    let mut outputs = partial["outputs"].as_array().unwrap().clone();
    outputs.extend(ended["outputs"].as_array().unwrap().clone());
    assert_eq!(Value::Array(outputs), rows[9]["result"]["outputs"]);
    assert_eq!(ended["state"], rows[9]["result"]["state"]);
    let parsed = |row: usize| {
        serde_json::from_str::<Value>(rows[row]["result"]["save"].as_str().unwrap()).unwrap()
    };
    assert_eq!(parsed(7), parsed(10));
    assert!(rows[9]["result"].get("outcome").is_none());
}

#[test]
fn budget_parameters_require_negotiation_and_invalid_values_do_not_advance() {
    let rows = exchange(vec![
        request(
            1,
            "compile",
            json!({"source":"event start\n  正文\n  -> END\n"}),
        ),
        request(2, "session.open", json!({"story_id":"s1","max_steps":2})),
        request(
            3,
            "session.open",
            json!({"story_id":"s1","capabilities":[CAPABILITY],"max_steps":-1}),
        ),
        request(4, "session.open", json!({"story_id":"s1"})),
        request(
            5,
            "session.continue",
            json!({"session_id":"c1","max_steps":2}),
        ),
        request(6, "session.cancel", json!({"session_id":"c1"})),
        request(
            7,
            "session.open",
            json!({"story_id":"s1","capabilities":[CAPABILITY],"time_budget_ms":u64::MAX}),
        ),
        request(
            8,
            "session.continue",
            json!({"session_id":"c2","max_steps":"2"}),
        ),
        request(
            9,
            "session.continue",
            json!({"session_id":"c2","time_budget_ms":1.5}),
        ),
        request(
            10,
            "session.continue",
            json!({"session_id":"c2","max_steps":0}),
        ),
        request(
            11,
            "session.continue",
            json!({"session_id":"c2","max_steps":100}),
        ),
    ]);
    for row in [1, 2, 4, 5, 7, 8] {
        assert_eq!(rows[row]["error"]["code"], -32602, "{}", rows[row]);
    }
    assert_eq!(rows[9]["result"]["outcome"], "step_budget_exceeded");
    assert_eq!(rows[9]["result"]["executed_steps"], 0);
    assert_eq!(rows[10]["result"]["outputs"][0]["content"], "正文");
    assert_eq!(rows[10]["result"]["ended"], true);
}

#[test]
fn choice_end_and_restart_keep_zero_budget_and_cancel_states_unambiguous() {
    let rows = exchange(vec![
        request(
            1,
            "compile",
            json!({"source":"event start\n  choice \"离开\"\n    -> END\n"}),
        ),
        request(
            2,
            "session.open",
            json!({"story_id":"s1","capabilities":[CAPABILITY],"max_steps":100,"time_budget_ms":u64::MAX}),
        ),
        request(3, "session.continue", json!({"session_id":"c1"})),
        request(4, "session.cancel", json!({"session_id":"c1"})),
        request(
            5,
            "session.continue",
            json!({"session_id":"c1","max_steps":0}),
        ),
        request(6, "session.choose", json!({"session_id":"c1","index":0})),
        request(7, "session.continue", json!({"session_id":"c1"})),
        request(8, "session.cancel", json!({"session_id":"c1"})),
        request(9, "session.restart", json!({"session_id":"c1"})),
        request(10, "session.continue", json!({"session_id":"c1"})),
    ]);
    assert_eq!(rows[2]["result"]["outcome"], "choice");
    assert_eq!(rows[4]["result"]["outcome"], "choice");
    assert_eq!(rows[4]["result"]["executed_steps"], 0);
    assert_eq!(rows[4]["result"]["state"], rows[2]["result"]["state"]);
    assert_eq!(rows[6]["result"]["outcome"], "ended");
    assert_eq!(rows[9]["result"]["outcome"], "choice");
}

#[test]
fn dated_entry_hint_is_contextual_and_only_added_to_negotiated_open() {
    let rows = exchange(vec![
        request(
            1,
            "compile",
            json!({"source":"period past\nevent history during past\n  一段历史。\n"}),
        ),
        request(2, "session.open", json!({"story_id":"s1"})),
        request(
            3,
            "session.open",
            json!({"story_id":"s1","capabilities":[CAPABILITY]}),
        ),
        request(4, "session.continue", json!({"session_id":"c2"})),
    ]);
    assert_eq!(rows[0]["result"]["diagnostics"], json!([]));
    assert!(rows[1]["result"].get("execution_diagnostics").is_none());
    assert_eq!(
        rows[2]["result"]["execution_diagnostics"][0]["code"],
        "A202"
    );
    assert_eq!(rows[3]["result"]["outcome"], "ended");
}

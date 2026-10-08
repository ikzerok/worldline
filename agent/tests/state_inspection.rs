use serde_json::{json, Value};
fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn exchange(rows: Vec<Value>) -> Vec<Value> {
    let input = rows
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
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
#[test]
fn rpc_inspection_keeps_save_trace_and_real_observation_contract() {
    let rows = exchange(vec![
        request(1, "initialize", json!({})),
        request(
            2,
            "compile",
            json!({"source":"let n = 0\nevent start\n  set n = 1\n  choice \"继续\"\n    set n = 2\n    -> END\n"}),
        ),
        request(3, "session.open", json!({"story_id":"s1","seed":5})),
        request(4, "session.inspect", json!({"session_id":"c1"})),
        request(5, "session.continue", json!({"session_id":"c1"})),
        request(6, "session.save", json!({"session_id":"c1"})),
        request(7, "session.trace", json!({"session_id":"c1"})),
        request(
            8,
            "session.inspect",
            json!({"session_id":"c1","query":{"text":"n"}}),
        ),
        request(
            9,
            "session.inspect",
            json!({"session_id":"c1","query":{"changed_only":true}}),
        ),
        request(10, "session.save", json!({"session_id":"c1"})),
        request(11, "session.trace", json!({"session_id":"c1"})),
    ]);
    assert!(rows[0]["result"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "runtime.state_inspection.v1"));
    let first = &rows[3]["result"]["inspection"];
    assert!(first["first_observation"].is_null());
    assert_eq!(first["items"][0]["first"]["status"], "unrecorded");
    let paused = &rows[7]["result"]["inspection"];
    assert_eq!(paused["current_observation"], 1);
    assert!(paused["previous_observation"].is_null());
    assert_eq!(paused["items"][0]["first"]["value"]["Num"], 1.0);
    assert_eq!(rows[5]["result"], rows[9]["result"]);
    assert_eq!(rows[6]["result"], rows[10]["result"]);
    assert_eq!(rows[8]["result"]["inspection"]["total_matches"], 0);
    assert_eq!(rows[8]["result"]["inspection"]["incomparable_items"], 1);
}
#[test]
fn rpc_inspection_rejects_unknown_oversized_or_stale_queries() {
    let stamp = json!({"run_id":"0","compiled_snapshot":"0","fingerprint":"0","trace_generation":"0","revision":"0"});
    let mut unknown_stamp = stamp.clone();
    unknown_stamp["future"] = json!(true);
    let rows = exchange(vec![
        request(1, "compile", json!({"source":"event start\n  -> END\n"})),
        request(2, "session.open", json!({"story_id":"s1","seed":1})),
        request(3, "session.inspect", json!([])),
        request(
            4,
            "session.inspect",
            json!({"session_id":"c1","future":true}),
        ),
        request(
            5,
            "session.inspect",
            json!({"session_id":"c1","query":{"limit":0}}),
        ),
        request(
            6,
            "session.inspect",
            json!({"session_id":"c1","query":{"expected_stamp":unknown_stamp}}),
        ),
        request(
            7,
            "session.inspect",
            json!({"session_id":"c1","query":{"text":"x".repeat(65536)}}),
        ),
        request(
            8,
            "session.inspect",
            json!({"session_id":"c1","query":{"expected_stamp":stamp}}),
        ),
    ]);
    for row in &rows[2..7] {
        assert_eq!(row["error"]["code"], -32602, "{row}");
    }
    assert_eq!(rows[7]["result"]["error"]["code"], "STALE_INSPECTION");
}

struct LiveAgent {
    child: std::process::Child,
    input: std::process::ChildStdin,
    output: std::io::BufReader<std::process::ChildStdout>,
    next_id: u64,
}
impl LiveAgent {
    fn start() -> Self {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_wl-agent"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take().unwrap(),
            output: std::io::BufReader::new(child.stdout.take().unwrap()),
            child,
            next_id: 0,
        }
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        use std::io::{BufRead, Write};
        self.next_id += 1;
        writeln!(self.input, "{}", request(self.next_id, method, params)).unwrap();
        self.input.flush().unwrap();
        let mut line = String::new();
        assert_ne!(
            self.output.read_line(&mut line).unwrap(),
            0,
            "agent在响应前退出"
        );
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], self.next_id);
        response
    }
}
impl Drop for LiveAgent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn rpc_inspection_live_decimal_stamp_json_echo_is_exact_and_read_only_then_stale() {
    let mut agent = LiveAgent::start();
    let compiled = agent.call(
        "compile",
        json!({"source":"let n = 0\nevent start\n  choice \"End\"\n    -> END\n"}),
    );
    assert_eq!(compiled["result"]["ok"], true);
    let fingerprint = compiled["result"]["fingerprint"].as_u64().unwrap();
    assert!(
        fingerprint > (1u64 << 53),
        "此回归须覆盖JavaScript整数精度边界"
    );
    assert_eq!(
        agent.call("session.open", json!({"story_id":"s1","seed":5}))["result"]["session_id"],
        "c1"
    );
    let initial = agent.call("session.inspect", json!({"session_id":"c1"}));
    let stamp = initial["result"]["inspection"]["stamp"].clone();
    for field in [
        "run_id",
        "compiled_snapshot",
        "fingerprint",
        "trace_generation",
        "revision",
    ] {
        assert!(stamp[field].is_string(), "field={field}");
    }
    assert_eq!(stamp["fingerprint"], fingerprint.to_string());
    // 实际stdio响应已JSON解码，再编码、解码后在同一进程/会话回送；绝不重建身份。
    let echoed: Value = serde_json::from_str(&serde_json::to_string(&stamp).unwrap()).unwrap();
    let before_save = agent.call("session.save", json!({"session_id":"c1"}))["result"].clone();
    let before_trace = agent.call("session.trace", json!({"session_id":"c1"}))["result"].clone();
    let checked = agent.call(
        "session.inspect",
        json!({"session_id":"c1","query":{"expected_stamp":echoed}}),
    );
    assert_eq!(checked["result"]["ok"], true);
    assert_eq!(
        checked["result"]["inspection"],
        initial["result"]["inspection"]
    );
    for invalid in [
        json!(0),
        json!(fingerprint),
        json!("+1"),
        json!("01"),
        json!("1e3"),
        json!("18446744073709551616"),
    ] {
        let mut broken = stamp.clone();
        broken["fingerprint"] = invalid;
        let response = agent.call(
            "session.inspect",
            json!({"session_id":"c1","query":{"expected_stamp":broken}}),
        );
        assert_eq!(response["error"]["code"], -32602, "{response}");
    }
    let mut unknown = stamp.clone();
    unknown["future"] = json!("1");
    assert_eq!(
        agent.call(
            "session.inspect",
            json!({"session_id":"c1","query":{"expected_stamp":unknown}})
        )["error"]["code"],
        -32602
    );
    assert_eq!(
        agent.call("session.save", json!({"session_id":"c1"}))["result"],
        before_save
    );
    assert_eq!(
        agent.call("session.trace", json!({"session_id":"c1"}))["result"],
        before_trace
    );
    agent.call("session.continue", json!({"session_id":"c1"}));
    let stale = agent.call(
        "session.inspect",
        json!({"session_id":"c1","query":{"expected_stamp":stamp}}),
    );
    assert_eq!(stale["result"]["error"]["code"], "STALE_INSPECTION");
    let fresh = agent.call("session.inspect", json!({"session_id":"c1"}));
    let fresh_stamp = fresh["result"]["inspection"]["stamp"].clone();
    assert_eq!(
        agent.call(
            "session.inspect",
            json!({"session_id":"c1","query":{"expected_stamp":fresh_stamp}})
        )["result"]["ok"],
        true
    );
    agent.call("shutdown", json!({}));
    assert!(agent.child.wait().unwrap().success());
}

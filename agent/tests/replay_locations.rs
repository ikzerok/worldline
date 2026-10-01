//! RPC 原始 trace、实际定位与重放判定使用同一 runtime 路径。
use serde_json::{json, Value};
use worldline_core::{compile_source_with_options, CompileOptions};
use worldline_runtime::{ReplayBudget, ReplayCancellation, ReplayTrace};

const SOURCE: &str = "fragment outer(file: str)\n  call gate(file)\n  return\nfragment gate(file: str)\n  local line: num = 7\n  choice \"继续\"\n    return\nevent start\n  call outer(\"地图\")\n  结束。\n  -> END\n";

fn request(id: usize, method: &str, params: Value) -> Value {
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
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn rpc_replays_old_locations_and_reports_current_locations_and_semantic_differences() {
    let rows = exchange(vec![
        request(
            1,
            "compile",
            json!({"source":SOURCE,"file_name":"original/story.wl","language_version":"1.11"}),
        ),
        request(2, "session.open", json!({"story_id":"s1","seed":31})),
        request(3, "session.continue", json!({"session_id":"c1"})),
        request(4, "session.trace", json!({"session_id":"c1"})),
        request(5, "session.choose", json!({"session_id":"c1","index":0})),
        request(6, "session.continue", json!({"session_id":"c1"})),
        request(7, "session.trace", json!({"session_id":"c1"})),
    ]);
    assert!(rows.iter().all(|row| row.get("error").is_none()));
    assert_eq!(rows[0]["result"]["ok"], true);
    for trace_row in [3, 6] {
        let raw = &rows[trace_row]["result"]["trace"];
        let trace: ReplayTrace = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(
            raw["initial_observation"]["state"]["calls"][1]["file"],
            "original/story.wl"
        );
        assert_eq!(raw["initial_observation"]["state"]["calls"][1]["line"], 6);
        for semantic_change in [false, true] {
            let source = format!("\n// 调整源码位置\n{SOURCE}");
            let source = if semantic_change {
                source.replace("\"地图\"", "\"海图\"")
            } else {
                source
            };
            let replay_rows = exchange(vec![
                request(
                    1,
                    "compile",
                    json!({"source":source,"file_name":"copied/story.wl","language_version":"1.11"}),
                ),
                request(
                    2,
                    "trace.replay",
                    json!({"story_id":"s1","trace":raw,"max_steps":1000,"time_budget_ms":5000}),
                ),
            ]);
            assert!(replay_rows[1].get("error").is_none());
            assert_eq!(replay_rows[1]["result"]["ok"], true);
            let result = &replay_rows[1]["result"]["replay"];
            assert_eq!(
                result["status"]["status"],
                if semantic_change {
                    "diverged"
                } else {
                    "replayed"
                }
            );
            let compiled =
                compile_source_with_options("copied/story.wl", &source, CompileOptions::v1_11());
            assert!(!compiled.has_errors());
            let expected = ReplayTrace::replay(
                &compiled.program,
                &compiled.analysis,
                &trace,
                ReplayBudget::new(1_000, 5_000),
                &ReplayCancellation::new(),
            )
            .unwrap();
            assert_eq!(*result, serde_json::to_value(expected).unwrap());
            if !trace.complete || semantic_change {
                assert_eq!(
                    result["current_state"]["calls"][1]["file"],
                    "copied/story.wl"
                );
                assert_eq!(result["current_state"]["calls"][1]["line"], 8);
            }
            if semantic_change {
                assert_eq!(result["status"]["step_index"], 0);
                assert_eq!(
                    result["current_state"]["calls"][1]["locals"]["file"]["Str"],
                    "海图"
                );
            } else {
                assert_eq!(result["status"]["complete"], trace.complete);
                assert_eq!(replay_rows[0]["result"]["fingerprint"], raw["fingerprint"]);
            }
            assert_eq!(serde_json::to_value(&trace).unwrap(), *raw);
        }
    }
}

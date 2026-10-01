//! CLI 重放直接复用 runtime 的定位归一化与语义判定。
use serde_json::Value;
use std::{fs, path::PathBuf};
use worldline_core::project::Project;
use worldline_runtime::{ReplayBudget, ReplayCancellation, ReplayTrace};

const SOURCE: &str = "fragment outer(file: str)\n  call gate(file)\n  return\nfragment gate(file: str)\n  local line: num = 7\n  choice \"继续\"\n    return\nevent start\n  call outer(\"地图\")\n  结束。\n  -> END\n";

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("wl-cli-replay-locations-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for directory in ["original", "copied"] {
            fs::create_dir_all(root.join(directory).join(".world")).unwrap();
            fs::write(
                root.join(directory).join(".world/project.json"),
                r#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#,
            )
            .unwrap();
            fs::write(root.join(directory).join("world.wl"), SOURCE).unwrap();
        }
        Self(root)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn invoke(args: &[String], input: &str) -> (i32, Vec<u8>) {
    let mut output = Vec::new();
    let code = wl::run(args, &mut output, &mut std::io::Cursor::new(input)).unwrap();
    (code, output)
}

#[test]
fn cli_replays_relocated_nested_calls_and_keeps_real_semantic_divergence() {
    let workspace = Workspace::new();
    let original = workspace.0.join("original");
    let copied = workspace.0.join("copied");
    let trace_path = workspace.0.join("trace.json");
    for complete in [false, true] {
        let (code, output) = invoke(
            &[
                "play".into(),
                original.display().to_string(),
                "--seed=31".into(),
                "--json".into(),
                format!("--trace-output={}", trace_path.display()),
            ],
            if complete { "0\n" } else { "" },
        );
        assert_eq!(code, 0, "{}", String::from_utf8_lossy(&output));
        let raw = fs::read_to_string(&trace_path).unwrap();
        let trace: ReplayTrace = serde_json::from_str(&raw).unwrap();
        let original_frame = &trace.initial_observation.as_ref().unwrap().state["calls"][1];
        assert!(original_frame["file"]
            .as_str()
            .unwrap()
            .contains("original"));
        assert_eq!(original_frame["line"], 6);
        for semantic_change in [false, true] {
            let source = format!("\n// 只改变当前源码定位\n{SOURCE}");
            fs::write(
                copied.join("world.wl"),
                if semantic_change {
                    source.replace("= 7", "= 8")
                } else {
                    source
                },
            )
            .unwrap();
            let (code, output) = invoke(
                &[
                    "replay".into(),
                    copied.display().to_string(),
                    "--json".into(),
                    format!("--trace-json={raw}"),
                    "--max-steps=1000".into(),
                    "--time-budget-ms=5000".into(),
                ],
                "",
            );
            assert_eq!(code, i32::from(semantic_change));
            let result: Value = serde_json::from_slice(&output).unwrap();
            assert_eq!(
                result["status"]["status"],
                if semantic_change {
                    "diverged"
                } else {
                    "replayed"
                }
            );
            let mut project = Project::open(&copied).unwrap();
            let compiled = project.compile();
            let expected = ReplayTrace::replay(
                &compiled.program,
                &compiled.analysis,
                &trace,
                ReplayBudget::new(1_000, 5_000),
                &ReplayCancellation::new(),
            )
            .unwrap();
            assert_eq!(result, serde_json::to_value(expected).unwrap());
            if !complete || semantic_change {
                assert_eq!(result["current_state"]["calls"][1]["line"], 8);
                assert_eq!(
                    result["current_state"]["calls"][1]["file"],
                    project.root.join("world.wl").to_string_lossy().as_ref()
                );
            }
            if semantic_change {
                assert_eq!(result["status"]["step_index"], 0);
                assert_eq!(
                    result["current_state"]["calls"][1]["locals"]["line"]["Num"],
                    8.0
                );
            } else {
                assert_eq!(result["status"]["complete"], complete);
            }
            assert_eq!(fs::read_to_string(&trace_path).unwrap(), raw);
        }
    }
}

use serde_json::{json, Value};
use std::fs;
fn invoke(args: Vec<String>) -> (i32, Value) {
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::new())).unwrap();
    (code, serde_json::from_slice(&out).unwrap())
}
#[test]
fn cli_search_pages_path_kind_and_budget_share_core_semantics() {
    let root = std::env::temp_dir().join(format!("wl-object-search-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let source = (0..105)
        .map(|n| format!("event e{n:03} as \"相同标题\"\n  -> END\n"))
        .collect::<String>();
    fs::write(root.join("world.wl"), &source).unwrap();
    let base = vec![
        "object-search".into(),
        root.to_string_lossy().into_owned(),
        "--query".into(),
        "world.wl".into(),
        "--filter-json".into(),
        json!({"allowed_kinds":["event"],"match_source_path":true}).to_string(),
        "--json".into(),
    ];
    let mut args = base.clone();
    args.extend([
        "--options-json".into(),
        json!({"offset":100,"limit":20}).to_string(),
    ]);
    let (code, result) = invoke(args);
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["page"]["total"], 105);
    assert_eq!(result["page"]["items"].as_array().unwrap().len(), 5);
    let mut args = base.clone();
    args.extend([
        "--options-json".into(),
        json!({"max_candidates":1}).to_string(),
    ]);
    let (code, result) = invoke(args);
    assert_eq!(code, 1);
    assert_eq!(result["error"]["code"], "CANDIDATE_BUDGET_EXCEEDED");
    let mut args = base.clone();
    args.extend(["--expected-baseline".into(), "old".into()]);
    let (code, result) = invoke(args);
    assert_eq!(code, 1);
    assert_eq!(result["error"]["code"], "STALE_BASELINE");
    let mut args = base;
    args[5] = "{\"allowed_kinds\":[],\"allowed_kinds\":[]}".into();
    assert_eq!(invoke(args).0, 2);
    assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), source);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}

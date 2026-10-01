use super::semantic_state;
use serde_json::{json, Value};

fn state() -> Value {
    json!({
        "calls": [
            {"file":"old.wl","line":4,"fragment":"outer","caller":"start",
             "statement":2,"call_statement":0,"params":{"file":"地图","line":7},
             "locals":{"file":{"Str":"地图"},"line":{"Num":7}},
             "extension":{"file":"语义文件值","line":9}},
            {"file":"old.wl","line":8,"fragment":"inner","caller":"fragment:outer",
             "statement":1,"call_statement":1,"locals":{}},
        ],
        "vars":{"file":{"Str":"实际文件"},"line":{"Num":3}},
        "rng":31,"unknown":{"file":"不能删","line":10},
        "coverage":{"selected_choices":[{"id":"choice","line":8,"count":1}]}
    })
}

#[test]
fn only_defined_call_frame_locations_are_ignored_without_mutating_raw_state() {
    let raw = state();
    let before = raw.clone();
    let mut relocated = raw.clone();
    for frame in relocated["calls"].as_array_mut().unwrap() {
        frame["file"] = json!("moved.wl");
        frame["line"] = json!(99);
    }
    relocated["coverage"]["selected_choices"][0]["line"] = json!(100);
    assert_eq!(semantic_state(&raw), semantic_state(&relocated));
    assert_eq!(raw, before);
    assert_eq!(relocated["calls"][0]["file"], "moved.wl");
    let normalized = semantic_state(&raw);
    assert_eq!(normalized["calls"][0]["locals"], raw["calls"][0]["locals"]);
    assert_eq!(normalized["calls"][0]["params"], raw["calls"][0]["params"]);
    assert_eq!(
        normalized["calls"][0]["extension"],
        raw["calls"][0]["extension"]
    );
}

#[test]
fn all_semantic_and_unknown_fields_and_stack_order_still_participate() {
    let raw = state();
    for path in [
        "/calls/0/fragment",
        "/calls/0/caller",
        "/calls/0/statement",
        "/calls/0/call_statement",
        "/calls/0/params/file",
        "/calls/0/params/line",
        "/calls/0/locals/file/Str",
        "/calls/0/locals/line/Num",
        "/calls/0/extension/file",
        "/calls/0/extension/line",
        "/calls/1/caller",
        "/vars/file/Str",
        "/vars/line/Num",
        "/rng",
        "/unknown/file",
        "/unknown/line",
        "/coverage/selected_choices/0/count",
    ] {
        let mut changed = raw.clone();
        *changed.pointer_mut(path).unwrap() = json!("changed");
        assert_ne!(semantic_state(&raw), semantic_state(&changed), "{path}");
    }
    let mut reordered = raw.clone();
    reordered["calls"].as_array_mut().unwrap().reverse();
    assert_ne!(semantic_state(&raw), semantic_state(&reordered));
    reordered["calls"].as_array_mut().unwrap().pop();
    assert_ne!(semantic_state(&raw), semantic_state(&reordered));
    let mut extended = raw.clone();
    extended["calls"][0]["future_field"] = json!({"file":"value","line":1});
    assert_ne!(semantic_state(&raw), semantic_state(&extended));
}

#[test]
fn absent_or_unrecognized_call_shapes_are_not_reinterpreted() {
    for raw in [
        json!({"vars":{"file":"value","line":7}}),
        json!({"calls":{"file":"unknown object","line":7}}),
        json!({"calls":[null,17,"file",["line"]]}),
    ] {
        assert_eq!(semantic_state(&raw), raw);
    }
}

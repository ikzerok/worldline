use serde_json::{json, Value};
use worldline_core::compile_source;
use worldline_runtime::{
    decode_replay_trace, encode_replay_trace, ChoiceIdentity, ReplayBudget, ReplayCancellation,
    ReplayObservation, ReplayOrigin, ReplayStep, ReplayTrace, Story, MAX_REPLAY_EXCHANGE_BYTES,
    MAX_REPLAY_EXCHANGE_STEPS, REPLAY_SCHEMA_VERSION,
};

fn minimal() -> ReplayTrace {
    ReplayTrace {
        presentation: None,
        schema_version: REPLAY_SCHEMA_VERSION,
        runtime_version: env!("CARGO_PKG_VERSION").into(),
        fingerprint: 1,
        origin: ReplayOrigin::Entry { seed: 42 },
        complete: false,
        steps: vec![],
        initial_observation: Some(ReplayObservation {
            outputs: vec![],
            choices: vec![],
            choice_presentation: vec![],
            state: json!({}),
        }),
    }
}

#[test]
fn actual_long_trace_roundtrips_without_losing_observations() {
    let body = "雾岸记事，潮声与灯火。".repeat(200);
    let c = compile_source("long.wl", &format!("event start\n  {body}\n  choice \"再来\"\n    -> start\n  choice \"结束\"\n    -> END\n"));
    assert!(!c.has_errors());
    let mut story = Story::new_with_seed(&c.program, &c.analysis, 19).unwrap();
    story.continue_story().unwrap();
    for _ in 0..200 {
        story.choose(0).unwrap();
        story.continue_story().unwrap();
    }
    story.choose(1).unwrap();
    story.continue_story().unwrap();
    let trace = story.replay_trace();
    let before = story.save().unwrap();
    let bytes = encode_replay_trace(&trace).unwrap();
    assert!(bytes.len() > 1024 * 1024);
    assert!(bytes.len() <= MAX_REPLAY_EXCHANGE_BYTES);
    let decoded = decode_replay_trace(bytes.as_bytes()).unwrap();
    assert_eq!(trace, decoded);
    assert_eq!(encode_replay_trace(&decoded).unwrap(), bytes);
    let replay = ReplayTrace::replay(
        &c.program,
        &c.analysis,
        &decoded,
        ReplayBudget::new(100_000, 30_000),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert!(matches!(
        replay.status,
        worldline_runtime::ReplayStatus::Replayed {
            ended: true,
            complete: true
        }
    ));
    assert_eq!(story.save().unwrap(), before);
}

#[test]
fn exact_utf8_wire_boundary_includes_json_overhead_and_escaping() {
    let mut trace = minimal();
    trace.initial_observation.as_mut().unwrap().state = json!("");
    let overhead = encode_replay_trace(&trace).unwrap().len();
    let fill = MAX_REPLAY_EXCHANGE_BYTES - overhead;
    trace.initial_observation.as_mut().unwrap().state = json!("a".repeat(fill));
    let encoded = encode_replay_trace(&trace).unwrap();
    assert_eq!(encoded.len(), MAX_REPLAY_EXCHANGE_BYTES);
    assert_eq!(decode_replay_trace(encoded.as_bytes()).unwrap(), trace);
    let mut over = encoded.into_bytes();
    over.push(b' ');
    assert_eq!(decode_replay_trace(&over).unwrap_err().code, "input_limit");
    trace.initial_observation.as_mut().unwrap().state = json!("雾".repeat(fill / 3 + 1));
    assert_eq!(
        encode_replay_trace(&trace).unwrap_err().code,
        "output_limit"
    );
    trace.initial_observation.as_mut().unwrap().state = json!("\"".repeat(fill / 2 + 1));
    assert_eq!(
        encode_replay_trace(&trace).unwrap_err().code,
        "output_limit"
    );
}

#[test]
fn every_json_layer_rejects_duplicate_keys_and_invalid_inputs() {
    let plain = encode_replay_trace(&minimal()).unwrap();
    for duplicate in [
        plain.replacen(
            "\"schema_version\":1",
            "\"schema_version\":2,\"schema_version\":1",
            1,
        ),
        plain.replace("\"state\":{}", "\"state\":{\"重复\":1,\"重复\":2}"),
        plain.replace("\"state\":{}", "\"state\":{\"outer\":[{\"x\":1,\"x\":1}]}"),
        plain.replacen('{', "{\"future\":{\"x\":1,\"x\":2},", 1),
    ] {
        assert_eq!(
            decode_replay_trace(duplicate.as_bytes()).unwrap_err().code,
            "invalid_json"
        );
    }
    for bytes in [b"".as_slice(), b"{} trailing", &[0xff], b"{\"x\":1e999}"] {
        assert_eq!(decode_replay_trace(bytes).unwrap_err().code, "invalid_json");
    }
    let deep = format!("{}0{}", "[".repeat(140), "]".repeat(140));
    assert_eq!(
        decode_replay_trace(deep.as_bytes()).unwrap_err().code,
        "invalid_json"
    );
    let mut trace = minimal();
    let mut value = json!(0);
    for _ in 0..140 {
        value = Value::Array(vec![value]);
    }
    trace.initial_observation.as_mut().unwrap().state = value;
    assert_eq!(
        encode_replay_trace(&trace).unwrap_err().code,
        "invalid_json"
    );
}

#[test]
fn old_runtime_unknown_optional_fields_and_pretty_input_remain_readable() {
    let mut trace = minimal();
    trace.runtime_version = "0.1.0".into();
    trace.initial_observation.as_mut().unwrap().state = json!({"中文":"引号\"\n\\尾"});
    let mut value = serde_json::to_value(&trace).unwrap();
    value["optional_future"] = json!({"note":"兼容"});
    let pretty = serde_json::to_string_pretty(&value).unwrap();
    assert_eq!(decode_replay_trace(pretty.as_bytes()).unwrap(), trace);
    let compact = encode_replay_trace(&trace).unwrap();
    assert_eq!(decode_replay_trace(compact.as_bytes()).unwrap(), trace);
    let c = compile_source("old.wl", "event start\n  -> END\n");
    assert!(ReplayTrace::replay(
        &c.program,
        &c.analysis,
        &trace,
        ReplayBudget::default(),
        &ReplayCancellation::new()
    )
    .is_err());
    trace.schema_version += 1;
    assert_eq!(
        encode_replay_trace(&trace).unwrap_err().code,
        "unsupported_schema"
    );
}

#[test]
fn steps_large_single_string_and_many_elements_have_explicit_bounded_outcomes() {
    let mut trace = minimal();
    let step = ReplayStep {
        choice: ChoiceIdentity {
            id: "x".into(),
            node: "n".into(),
            line: 1,
            offset: 0,
            label: "选".into(),
        },
        observation: None,
    };
    trace.steps = vec![step.clone(); MAX_REPLAY_EXCHANGE_STEPS];
    let encoded = encode_replay_trace(&trace).unwrap();
    assert_eq!(
        decode_replay_trace(encoded.as_bytes()).unwrap().steps.len(),
        MAX_REPLAY_EXCHANGE_STEPS
    );
    trace.steps.push(step);
    assert_eq!(encode_replay_trace(&trace).unwrap_err().code, "step_limit");
    assert_eq!(
        decode_replay_trace(serde_json::to_string(&trace).unwrap().as_bytes())
            .unwrap_err()
            .code,
        "step_limit"
    );
    trace.steps.clear();
    trace.initial_observation.as_mut().unwrap().state = json!("潮".repeat(100_000));
    assert_eq!(
        decode_replay_trace(encode_replay_trace(&trace).unwrap().as_bytes()).unwrap(),
        trace
    );
    trace.initial_observation.as_mut().unwrap().state = Value::Array(vec![Value::Null; 100_000]);
    assert_eq!(
        decode_replay_trace(encode_replay_trace(&trace).unwrap().as_bytes()).unwrap(),
        trace
    );
    trace.initial_observation.as_mut().unwrap().state = Value::Array(vec![Value::Null; 900_000]);
    assert_eq!(
        encode_replay_trace(&trace).unwrap_err().code,
        "output_limit"
    );
}

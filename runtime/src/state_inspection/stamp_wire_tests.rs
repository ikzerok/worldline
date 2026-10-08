use crate::{InspectionStamp, StateInspectionQuery, Story};
use serde_json::{json, Value};

const FIELDS: [&str; 5] = [
    "run_id",
    "compiled_snapshot",
    "fingerprint",
    "trace_generation",
    "revision",
];
fn stamp(values: [u64; 5]) -> InspectionStamp {
    InspectionStamp {
        run_id: values[0],
        compiled_snapshot: values[1],
        fingerprint: values[2],
        trace_generation: values[3],
        revision: values[4],
    }
}
fn zero() -> Value {
    serde_json::to_value(stamp([0; 5])).unwrap()
}

#[test]
fn inspection_stamp_decimal_strings_preserve_every_u64_boundary_field() {
    for boundary in [
        0,
        1,
        (1u64 << 53) - 1,
        1u64 << 53,
        (1u64 << 53) + 1,
        u64::MAX - 1,
        u64::MAX,
    ] {
        for index in 0..FIELDS.len() {
            let mut values = [0, 1, (1u64 << 53) + 1, u64::MAX, (1u64 << 53) - 1];
            values[index] = boundary;
            let original = stamp(values);
            let wire = serde_json::to_string(&original).unwrap();
            let json: Value = serde_json::from_str(&wire).unwrap();
            for (field, expected) in FIELDS.iter().zip(values) {
                assert_eq!(
                    json[field],
                    expected.to_string(),
                    "field={field}, wire={wire}"
                );
            }
            assert_eq!(
                serde_json::from_str::<InspectionStamp>(&wire).unwrap(),
                original
            );
            assert_eq!(
                serde_json::from_value::<InspectionStamp>(json.clone()).unwrap(),
                original
            );
            let query: StateInspectionQuery =
                serde_json::from_value(json!({"expected_stamp":json})).unwrap();
            assert_eq!(query.expected_stamp, Some(original));
        }
    }
}

#[test]
fn inspection_stamp_rejects_numbers_noncanonical_strings_and_overflow_in_every_field() {
    let mut invalid = vec![
        json!(0),
        json!(1),
        json!(u64::MAX),
        json!(-1),
        json!(1.0),
        json!(true),
        Value::Null,
        json!([]),
        json!({}),
    ];
    invalid.extend(
        [
            "",
            "+0",
            "+1",
            "-0",
            "-1",
            "00",
            "01",
            "0001",
            " 1",
            "1 ",
            "1\n",
            "1.0",
            "0.0",
            "1e3",
            "1E3",
            "0x1",
            "１",
            "١",
            "18446744073709551616",
            "99999999999999999999",
        ]
        .into_iter()
        .map(|text| json!(text)),
    );
    invalid.push(json!("0".repeat(100)));
    for field in FIELDS {
        for invalid in &invalid {
            let mut encoded = zero();
            encoded[field] = invalid.clone();
            assert!(
                serde_json::from_value::<InspectionStamp>(encoded.clone()).is_err(),
                "field={field}, value={invalid}"
            );
            assert!(
                serde_json::from_str::<InspectionStamp>(&encoded.to_string()).is_err(),
                "field={field}, value={invalid}"
            );
            assert!(serde_json::from_value::<StateInspectionQuery>(
                json!({"expected_stamp":encoded})
            )
            .is_err());
        }
    }
}

#[test]
fn inspection_stamp_requires_all_fields_and_rejects_unknown_fields() {
    for field in FIELDS {
        let mut encoded = zero();
        encoded.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<InspectionStamp>(encoded).is_err());
    }
    let mut encoded = zero();
    encoded["future"] = json!("1");
    assert!(serde_json::from_value::<InspectionStamp>(encoded.clone()).is_err());
    assert!(
        serde_json::from_value::<StateInspectionQuery>(json!({"expected_stamp":encoded})).is_err()
    );
}

#[test]
fn inspection_stamp_encoding_does_not_change_existing_persistent_or_state_shapes() {
    let compiled = worldline_core::compile_source(
        "stamp.wl",
        "let n = 0\nevent start\n  choice \"结束\"\n    -> END\n",
    );
    assert!(!compiled.has_errors());
    let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, 5).unwrap();
    story.continue_story().unwrap();
    let trace = serde_json::to_value(story.replay_trace()).unwrap();
    let checkpoint = serde_json::to_value(story.checkpoint().unwrap()).unwrap();
    let save: Value = serde_json::from_str(&story.save().unwrap()).unwrap();
    for value in [&trace, &checkpoint, &save] {
        assert!(value["fingerprint"].is_u64());
        assert_eq!(
            value["fingerprint"].as_u64(),
            Some(compiled.analysis.fingerprint)
        );
    }
    assert!(story.state_view()["vars"]["n"]["Num"].is_number());
    let page = story.inspect_state(&Default::default()).unwrap();
    let encoded = serde_json::to_value(&page).unwrap();
    assert_eq!(
        encoded["stamp"]["fingerprint"],
        compiled.analysis.fingerprint.to_string()
    );
    assert!(encoded["current_observation"].is_u64());
}

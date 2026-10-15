use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::{compile_path_with_options, CompileOptions};
use worldline_runtime::{InspectionStamp, StateInspectionQuery, Story};

const SOURCE: &str = "world w as \"同名对象\"\ncharacter person as \"同名对象\"\ntag initial\ntag changed\nstate mood on world w with initial\nstate other_mood on character person with initial\nlet w_shadow = true\nevent start\n  become mood with changed\n  choice \"结束\"\n    -> END\n";
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".scratch")
            .join(format!(
                "object-inspection-cli-{}-{nonce}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("world.wl"), SOURCE).unwrap();
        Self(root)
    }
    fn play(&self, input: &str) -> (i32, Vec<Value>) {
        let args = [
            "play".into(),
            self.0.join("world.wl").to_string_lossy().into_owned(),
            "--json".into(),
            "--language-version=1.13".into(),
            "--seed=42".into(),
        ];
        let mut output = Vec::new();
        let code = wl::run(&args, &mut output, &mut std::io::Cursor::new(input)).unwrap();
        let rows = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        (code, rows)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn cli_owner_inspection_is_exact_and_static_initial_is_not_first_observation() {
    let fixture = Fixture::new();
    let (code, rows) = fixture.play("inspect {\"owner\":{\"kind\":\"world\",\"id\":\"w\"}}\ninspect {\"owner\":{\"kind\":\"world\",\"id\":\"w\"}}\n0\n");
    assert_eq!(code, 0, "{rows:?}");
    let inspections: Vec<_> = rows
        .iter()
        .filter(|row| row["type"] == "inspection")
        .collect();
    assert_eq!(inspections.len(), 2);
    assert_eq!(inspections[0], inspections[1]);
    assert_eq!(inspections[0]["ok"], true);
    let page = &inspections[0]["inspection"];
    assert_eq!(page["owner"]["target"], json!({"kind":"world","id":"w"}));
    assert_eq!(page["total_matches"], 1);
    let items = page["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["key"]["group"], "state");
    assert_eq!(items[0]["key"]["name"], "mood");
    assert_eq!(
        items[0]["state_definition"]["owner"],
        json!({"kind":"world","id":"w"})
    );
    assert_eq!(
        items[0]["state_definition"]["initial"]["value"],
        json!({"TagSet":["initial"]})
    );
    assert_eq!(items[0]["first"]["value"], json!({"TagSet":["changed"]}));
    assert_eq!(items[0]["current"]["value"], json!({"TagSet":["changed"]}));
    assert_eq!(rows.last().unwrap()["type"], "ended");
    assert_eq!(rows.last().unwrap()["state"]["turns"], 1);
    assert_eq!(
        fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
        SOURCE
    );
}

#[test]
fn invalid_and_missing_owner_inspection_keeps_waiting_for_the_same_choice() {
    let fixture = Fixture::new();
    let (code, rows) = fixture.play(concat!(
        "inspect {\"owner\":{\"kind\":\"world\",\"id\":\"w\",\"display\":\"guess\"}}\n",
        "inspect {\"owner\":{\"kind\":\"world\",\"id\":\"w\"},\"group\":\"global\"}\n",
        "inspect {\"owner\":{\"kind\":\"world\",\"id\":\"missing\"}}\n",
        "inspect {\"owner\":{\"kind\":\"character\",\"id\":\"person\"}}\n0\n"
    ));
    assert_eq!(code, 0, "{rows:?}");
    let inspections: Vec<_> = rows
        .iter()
        .filter(|row| row["type"] == "inspection")
        .collect();
    assert_eq!(inspections.len(), 4);
    for row in &inspections[..2] {
        assert_eq!(row["ok"], false);
        assert_eq!(row["error"]["code"], "INVALID_INSPECTION_QUERY");
    }
    assert_eq!(
        inspections[2]["error"]["code"],
        "INSPECTION_OWNER_NOT_FOUND"
    );
    assert_eq!(inspections[3]["ok"], true);
    assert_eq!(
        inspections[3]["inspection"]["items"][0]["key"]["name"],
        "other_mood"
    );
    assert_eq!(rows.last().unwrap()["type"], "ended");
    assert_eq!(rows.last().unwrap()["state"]["turns"], 1);
}

fn normalize_run_identity(mut row: Value, expected_stamp: InspectionStamp) -> Value {
    assert_eq!(row["inspection"]["stamp"], json!(expected_stamp));
    let stamp = row["inspection"]["stamp"].as_object_mut().unwrap();
    // 两个独立 Story 只有瞬态运行/编译实例身份不同；其余 DTO 字段逐项保留。
    assert_eq!(
        stamp.remove("run_id"),
        Some(json!(expected_stamp.run_id.to_string()))
    );
    assert_eq!(
        stamp.remove("compiled_snapshot"),
        Some(json!(expected_stamp.compiled_snapshot.to_string()))
    );
    row
}

#[test]
fn cli_owner_inspection_matches_direct_runtime_full_dto_with_run_identity_guard() {
    let fixture = Fixture::new();
    let compiled =
        compile_path_with_options(&fixture.0.join("world.wl"), CompileOptions::v1_13()).unwrap();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, 42).unwrap();
    story.continue_story().unwrap();
    let direct_stamp = story.inspection_stamp();
    let saved = story.save().unwrap();
    let trace = story.replay_trace();
    let queries = [
        json!({"owner":{"kind":"world","id":"w"}}),
        json!({"owner":{"kind":"character","id":"person"}}),
        json!({"owner":{"kind":"world","id":"w"},"group":"state","text":"changed","limit":1}),
        json!({"owner":{"kind":"world","id":"w"},"offset":1,"limit":1}),
        json!({"owner":{"kind":"world","id":"w"},"changed_only":true}),
        json!({"owner":{"kind":"world","id":"missing"}}),
        json!({"owner":{"kind":"world","id":"w"},"group":"global"}),
        json!({"owner":{"kind":"world","id":"w"}}),
    ];
    let mut input = String::new();
    let expected: Vec<Value> = queries
        .iter()
        .map(|query| {
            input.push_str(&format!("inspect {query}\n"));
            let query: StateInspectionQuery = serde_json::from_value(query.clone()).unwrap();
            match story.inspect_state(&query) {
                Ok(page) => json!({"type":"inspection","ok":true,"inspection":page}),
                Err(error) => json!({"type":"inspection","ok":false,"error":error}),
            }
        })
        .collect();
    let foreign_query = json!({
        "owner":{"kind":"world","id":"w"},
        "expected_stamp":direct_stamp,
    });
    input.push_str(&format!("inspect {foreign_query}\n0\n"));
    let (code, rows) = fixture.play(&input);
    assert_eq!(code, 0, "{rows:?}");
    let inspections: Vec<_> = rows
        .iter()
        .filter(|row| row["type"] == "inspection")
        .collect();
    assert_eq!(inspections.len(), queries.len() + 1);
    // 严格反序列化检查五项身份的规范十进制字符串及完整字段集合。
    let cli_stamp: InspectionStamp =
        serde_json::from_value(inspections[0]["inspection"]["stamp"].clone()).unwrap();
    assert!(cli_stamp.run_id > 0 && direct_stamp.run_id > 0);
    assert!(cli_stamp.compiled_snapshot > 0 && direct_stamp.compiled_snapshot > 0);
    assert_ne!(cli_stamp.run_id, direct_stamp.run_id);
    assert_ne!(cli_stamp.compiled_snapshot, direct_stamp.compiled_snapshot);
    for (actual, expected) in inspections.iter().zip(expected) {
        if expected["ok"] == true {
            assert_eq!(
                normalize_run_identity((*actual).clone(), cli_stamp),
                normalize_run_identity(expected, direct_stamp)
            );
        } else {
            assert_eq!(**actual, expected);
        }
    }
    assert_eq!(inspections[0], inspections[queries.len() - 1]);
    let mut foreign_query: StateInspectionQuery = serde_json::from_value(foreign_query).unwrap();
    foreign_query.expected_stamp = Some(cli_stamp);
    let stale = story.inspect_state(&foreign_query).unwrap_err();
    assert_eq!(stale.code, "STALE_INSPECTION");
    assert_eq!(
        *inspections.last().unwrap(),
        &json!({"type":"inspection","ok":false,"error":stale})
    );
    assert_eq!(story.save().unwrap(), saved);
    assert_eq!(story.replay_trace(), trace);
    assert_eq!(story.inspection_stamp(), direct_stamp);
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    assert_eq!(rows.last().unwrap()["type"], "ended");
    assert_eq!(rows.last().unwrap()["state"], json!(story.state_view()));
    assert_eq!(
        fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
        SOURCE
    );
}

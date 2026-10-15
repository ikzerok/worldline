use serde_json::{json, Value};
use worldline_core::{compile_source_with_options, CompileOptions, TargetRef};
use worldline_runtime::{StateInspectionPage, StateInspectionQuery, Story};

const SOURCE: &str =
    include_str!("../../runtime/tests/fixtures/object_state_inspection_v036/world.wl");
const FILE: &str = "object-rpc.wl";

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
fn query(kind: &str, id: &str) -> Value {
    json!({"owner":{"kind":kind,"id":id}})
}
fn compile_request() -> Value {
    request(
        2,
        "compile",
        json!({"source":SOURCE,"file_name":FILE,"language_version":"1.13"}),
    )
}

#[test]
fn rpc_object_inspection_matches_typed_runtime_pages_and_preserves_save_trace() {
    let rows = exchange(vec![
        request(1, "initialize", json!({})),
        compile_request(),
        request(
            3,
            "session.inspect",
            json!({"session_id":"c1","query":query("character", "shared")}),
        ),
        request(4, "session.open", json!({"story_id":"s1","seed":7})),
        request(5, "session.continue", json!({"session_id":"c1"})),
        request(6, "session.save", json!({"session_id":"c1"})),
        request(7, "session.trace", json!({"session_id":"c1"})),
        request(
            8,
            "session.inspect",
            json!({"session_id":"c1","query":{"owner":{"kind":"character","id":"shared"},"limit":1}}),
        ),
        request(
            9,
            "session.inspect",
            json!({"session_id":"c1","query":{"owner":{"kind":"character","id":"shared"},"limit":1,"offset":1}}),
        ),
        request(
            10,
            "session.inspect",
            json!({"session_id":"c1","query":query("world", "shared")}),
        ),
        request(
            11,
            "session.inspect",
            json!({"session_id":"c1","query":query("entity", "empty")}),
        ),
        request(
            12,
            "session.inspect",
            json!({"session_id":"c1","query":query("character", "missing")}),
        ),
        request(13, "session.save", json!({"session_id":"c1"})),
        request(14, "session.trace", json!({"session_id":"c1"})),
    ]);
    assert!(rows[0]["result"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("runtime.object_state_inspection.v1")));
    assert_eq!(rows[1]["result"]["ok"], true, "{}", rows[1]);
    assert_eq!(rows[2]["error"]["code"], -32602);
    assert_eq!(rows[3]["result"]["session_id"], "c1");
    let c = compile_source_with_options(FILE, SOURCE, CompileOptions::v1_13());
    assert!(!c.has_errors());
    let mut story = Story::new_with_seed(&c.program, &c.analysis, 7).unwrap();
    story.continue_story().unwrap();
    for (response, kind, id, offset, limit) in [
        (7, "character", "shared", 0, 1),
        (8, "character", "shared", 1, 1),
        (9, "world", "shared", 0, 50),
        (10, "entity", "empty", 0, 50),
    ] {
        let actual: StateInspectionPage =
            serde_json::from_value(rows[response]["result"]["inspection"].clone()).unwrap();
        let mut expected = story
            .inspect_state(&StateInspectionQuery {
                owner: Some(TargetRef::new(kind, id)),
                offset,
                limit,
                ..Default::default()
            })
            .unwrap();
        // 每次真实运行身份独立；值、归属、来源、状态和分页字段须完全相同。
        assert_ne!(actual.stamp.run_id, expected.stamp.run_id);
        expected.stamp = actual.stamp;
        assert_eq!(actual, expected);
    }
    assert_eq!(
        rows[7]["result"]["inspection"]["stamp"],
        rows[8]["result"]["inspection"]["stamp"]
    );
    assert_eq!(rows[11]["result"]["ok"], false);
    assert_eq!(
        rows[11]["result"]["error"]["code"],
        "INSPECTION_OWNER_NOT_FOUND"
    );
    assert_eq!(rows[5]["result"], rows[12]["result"]);
    assert_eq!(rows[6]["result"], rows[13]["result"]);
}

#[test]
fn rpc_object_inspection_rejects_invalid_owner_but_keeps_stale_as_business_failure() {
    let mut requests = vec![
        compile_request(),
        request(3, "session.open", json!({"story_id":"s1","seed":7})),
    ];
    for (index, query) in [
        json!({"owner":{}}),
        json!({"owner":{"id":"shared"}}),
        json!({"owner":{"kind":"character","id":"shared","name":"同名"}}),
        json!({"owner":{"kind":"unknown","id":"shared"}}),
        json!({"owner":{"kind":"character","id":""}}),
        json!({"owner":{"kind":"character","id":"shared"},"group":"global"}),
        json!({"owner":{"kind":"character","id":"a".repeat(16 * 1024)}}),
        json!({"owner":{"kind":"character","id":"shared"},"limit":101}),
    ]
    .into_iter()
    .enumerate()
    {
        requests.push(request(
            index as u64 + 10,
            "session.inspect",
            json!({"session_id":"c1","query":query}),
        ));
    }
    requests.push(request(30, "session.inspect", json!({"session_id":"c1","query":{
        "owner":{"kind":"character","id":"missing"},
        "expected_stamp":{"run_id":"0","compiled_snapshot":"0","fingerprint":"0","trace_generation":"0","revision":"0"}
    }})));
    requests.push(request(
        31,
        "session.inspect",
        json!({"session_id":"c1","query":query("character", "shared")}),
    ));
    let rows = exchange(requests);
    for row in &rows[2..10] {
        assert_eq!(row["error"]["code"], -32602, "{row}");
    }
    assert_eq!(rows[10]["result"]["error"]["code"], "STALE_INSPECTION");
    let initial = &rows[11]["result"]["inspection"];
    assert!(initial["first_observation"].is_null());
    let mood = &initial["items"][1];
    assert_eq!(
        mood["state_definition"]["initial"]["value"]["TagSet"],
        json!(["calm"])
    );
    assert_eq!(mood["current"]["value"]["TagSet"], json!(["alert"]));
    assert_eq!(mood["first"]["status"], "unrecorded");
}

#[test]
fn rpc_draft_object_inspection_keeps_its_frozen_initial_and_validates_owner_first() {
    use worldline_core::{draft_rehearsal::DraftRehearsalRequest, project::Project};
    let root = std::env::temp_dir().join(format!(
        "object-inspection-rpc-v036-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let mut project = Project::new(&root);
    project
        .set_text(
            &project.entry.clone(),
            include_str!("../../runtime/tests/fixtures/object_state_inspection_v036/applied.wl")
                .into(),
        )
        .unwrap();
    project.save().unwrap();
    let before = std::fs::read(root.join("world.wl")).unwrap();
    let mut buffer = project.open_source_writing_buffer(&project.entry).unwrap();
    buffer.replace_source(
        include_str!("../../runtime/tests/fixtures/object_state_inspection_v036/draft.wl").into(),
    );
    let input =
        DraftRehearsalRequest::from_writing_buffers(&project, &[buffer], vec![], false).unwrap();
    let mut draft = json!({"input":input,"seed":7,"inspection":query("world", "room")});
    let valid = request(
        2,
        "project.draft_rehearsal",
        json!({"project_id":"p1","request":draft}),
    );
    draft["inspection"]["group"] = json!("local");
    let rows = exchange(vec![
        request(1, "project.open", json!({"path":root})),
        valid,
        request(
            3,
            "project.draft_rehearsal",
            json!({"project_id":"p1","request":draft}),
        ),
        request(4, "project.analyze", json!({"project_id":"p1"})),
    ]);
    let output = &rows[1]["result"];
    assert_eq!(output["ok"], true, "{output}");
    assert_eq!(output["applied"], false);
    assert_eq!(output["saved"], false);
    let page = &output["result"]["inspection"];
    assert_eq!(page["owner"]["display"], "草稿地点");
    assert_eq!(
        page["items"][0]["state_definition"]["initial"]["value"]["TagSet"],
        json!(["alert"])
    );
    assert_eq!(
        page["items"][0]["first"]["value"]["TagSet"],
        json!(["calm"])
    );
    assert_eq!(rows[2]["error"]["code"], -32602);
    assert_eq!(rows[0]["result"]["baseline"], rows[3]["result"]["baseline"]);
    assert_eq!(std::fs::read(root.join("world.wl")).unwrap(), before);
    std::fs::remove_dir_all(root).unwrap();
}

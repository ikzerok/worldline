use super::*;

fn request(id: Value, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"manuscript.review","params":params})
}
#[test]
fn rpc_id_limits_use_encoded_unicode_and_escaped_bytes() {
    let server = Server::default();
    for id in [json!("中".repeat(1024)), json!("\"\\\n".repeat(512))] {
        let response = dispatch(&server, &request(id, json!({}))).unwrap();
        assert_eq!(response["id"], Value::Null);
        assert_eq!(response["error"]["code"], -32600);
        assert!(serde_json::to_vec(&response).unwrap().len() < MAX_RESPONSE);
    }
    let id = json!("a".repeat(3070));
    assert_eq!(serde_json::to_vec(&id).unwrap().len(), 3072);
    let response = dispatch(&server, &request(id.clone(), json!({}))).unwrap();
    assert_eq!(response["id"], id);
    assert_eq!(response["error"]["code"], -32602);
    assert!(serde_json::to_vec(&response).unwrap().len() < MAX_RESPONSE);
}

#[test]
fn rpc_matches_core_and_does_not_mutate_project() {
    let mut project = Project::new(&std::env::temp_dir().join("review-rpc-test"));
    let path = project.entry.clone();
    project.documents.retain(|entry, _| entry == &path);
    project
        .set_text(
            &path,
            "event start\n  if true\n    甲。\n  else\n    乙。\n  -> END\n".into(),
        )
        .unwrap();
    let baseline = project.content_baseline();
    let expected = review_projection(
        &project.compile_read_only().unwrap(),
        &TargetRef::new("event", "start"),
    )
    .unwrap();
    let mut server = Server::default();
    server.projects.insert(
        "p1".into(),
        ProjectUnit {
            project,
            entry: path,
            scene_revision: Default::default(),
            problems_report: None,
            reconciliation: Default::default(),
        },
    );
    let params = json!({"project_id":"p1","target":{"kind":"event","id":"start"}});
    let response = dispatch(&server, &request(json!(1), params.clone())).unwrap();
    assert_eq!(response["result"]["ok"], true);
    assert_eq!(response["result"]["review"], json!(expected));
    assert_eq!(server.projects["p1"].project.content_baseline(), baseline);
    assert!(dispatch(
        &server,
        &json!({"jsonrpc":"2.0","method":"manuscript.review","params":params})
    )
    .is_none());
    let response = dispatch(
        &server,
        &request(
            json!(2),
            json!({"project_id":"p1","target":{"kind":"event","id":"absent"}}),
        ),
    )
    .unwrap();
    assert_eq!(response["result"]["ok"], false);
    assert!(response.get("error").is_none());
}

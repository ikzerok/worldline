use super::*;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const SOURCE: &str = "character actor\ncharacter other\nevent start\n  say actor \"原台词🙂\" direction \"PRIVATE_DIRECTION\"\n  -> END\n";
fn fixture() -> (PathBuf, Server, String) {
    let root = std::env::temp_dir().join(format!(
        "v034-rpc-dialogue-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), SOURCE).unwrap();
    fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.11","entry":"world.wl","required_features":[]}"#).unwrap();
    let mut server = Server::default();
    let opened = rpc(&mut server, "project.open", json!({"path":root}));
    assert_eq!(opened["result"]["ok"], true, "{opened}");
    let id = opened["result"]["project_id"].as_str().unwrap().to_owned();
    (root, server, id)
}
fn rpc(server: &mut Server, method: &str, params: Value) -> Value {
    server
        .dispatch(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string())
        .unwrap()
}
fn query(server: &mut Server, id: &str) -> Value {
    let value = rpc(
        server,
        "dialogue.query",
        json!({"project_id":id,"target":{"kind":"event","id":"start"}}),
    );
    assert_eq!(value["result"]["ok"], true, "{value}");
    value["result"]["projection"].clone()
}
fn request(projection: &Value, text: &str) -> Value {
    let statement = &projection["statements"][0];
    let mut draft = statement["draft"].clone();
    draft["parts"] = json!([{"kind":"literal","text":text}]);
    json!({"schema_version":1,"expected_baseline":projection["baseline"],"target":projection["target"],"generation":0,
        "operation":{"kind":"update","statement_id":statement["id"],"draft":draft}})
}

#[test]
fn applies_current_project_only_then_explicit_save_and_rejects_old_digest() {
    let (root, mut server, id) = fixture();
    let projection = query(&mut server, &id);
    let request = request(&projection, "新台词\n原样{文字}🙂");
    let preview = rpc(
        &mut server,
        "dialogue.edit.preview",
        json!({"project_id":id,"request":request}),
    );
    assert_eq!(preview["result"]["ok"], true, "{preview}");
    let apply = json!({"project_id":id,"request":request,"plan_digest":preview["result"]["plan"]["plan_digest"]});
    let applied = rpc(&mut server, "dialogue.edit.apply", apply.clone());
    assert_eq!(applied["result"]["applied"], true, "{applied}");
    assert_eq!(applied["result"]["saved"], false);
    assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), SOURCE);
    let fresh = query(&mut server, &id);
    assert_eq!(
        fresh["statements"][0]["draft"]["parts"][0]["text"],
        "新台词\n原样{文字}🙂"
    );
    assert_eq!(
        rpc(&mut server, "dialogue.edit.apply", apply)["result"]["ok"],
        false
    );
    let saved = rpc(
        &mut server,
        "project.save",
        json!({"project_id":id,"expected_baseline":applied["result"]["baseline"]}),
    );
    assert_eq!(saved["result"]["ok"], true, "{saved}");
    assert_ne!(fs::read_to_string(root.join("world.wl")).unwrap(), SOURCE);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn direction_loss_requires_new_explicitly_confirmed_plan() {
    let (root, mut server, id) = fixture();
    let projection = query(&mut server, &id);
    let mut request = json!({"schema_version":1,"expected_baseline":projection["baseline"],"target":projection["target"],"generation":0,
        "operation":{"kind":"convert","statement_id":projection["statements"][0]["id"],"to":"text"}});
    let preview = rpc(
        &mut server,
        "dialogue.edit.preview",
        json!({"project_id":id,"request":request}),
    );
    assert_eq!(preview["result"]["plan"]["can_apply"], false, "{preview}");
    let old_digest = preview["result"]["plan"]["plan_digest"].clone();
    let denied = rpc(
        &mut server,
        "dialogue.edit.apply",
        json!({"project_id":id,"request":request,"plan_digest":old_digest}),
    );
    assert_eq!(denied["result"]["ok"], false);
    assert_eq!(denied["result"]["applied"], false);
    request["operation"]["allow_direction_loss"] = json!(true);
    let stale = rpc(
        &mut server,
        "dialogue.edit.apply",
        json!({"project_id":id,"request":request,"plan_digest":old_digest}),
    );
    assert_eq!(stale["result"]["ok"], false);
    let confirmed = rpc(
        &mut server,
        "dialogue.edit.preview",
        json!({"project_id":id,"request":request}),
    );
    assert_eq!(
        confirmed["result"]["plan"]["can_apply"], true,
        "{confirmed}"
    );
    let applied = rpc(
        &mut server,
        "dialogue.edit.apply",
        json!({"project_id":id,"request":request,"plan_digest":confirmed["result"]["plan"]["plan_digest"]}),
    );
    assert_eq!(applied["result"]["ok"], true, "{applied}");
    assert_eq!(query(&mut server, &id)["statements"][0]["kind"], "text");
    assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), SOURCE);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn no_op_protocol_and_external_change_guards_preserve_current_memory() {
    let (root, mut server, id) = fixture();
    let projection = query(&mut server, &id);
    let request = request(&projection, "原台词🙂");
    let preview = rpc(
        &mut server,
        "dialogue.edit.preview",
        json!({"project_id":id,"request":request}),
    );
    assert_eq!(preview["result"]["plan"]["no_change"], true, "{preview}");
    let applied = rpc(
        &mut server,
        "dialogue.edit.apply",
        json!({"project_id":id,"request":request,"plan_digest":preview["result"]["plan"]["plan_digest"]}),
    );
    assert_eq!(applied["result"]["applied"], false, "{applied}");
    assert_eq!(applied["result"]["baseline"], projection["baseline"]);
    let mut invalid = request.clone();
    invalid["operation"]["draft"]["speaker"]["injected"] = json!(true);
    assert_eq!(
        rpc(
            &mut server,
            "dialogue.edit.preview",
            json!({"project_id":id,"request":invalid})
        )["error"]["code"],
        -32602
    );
    assert_eq!(
        rpc(
            &mut server,
            "dialogue.query",
            json!({"project_id":id,"target":{"kind":"event","id":"start","extra":1}})
        )["error"]["code"],
        -32602
    );
    let duplicate = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"dialogue.query","params":{{"project_id":"{id}","project_id":"{id}","target":{{"kind":"event","id":"start"}}}}}}"#
    );
    assert_eq!(
        server.dispatch(&duplicate).unwrap()["error"]["code"],
        -32700
    );
    let oversized_id =
        json!({"jsonrpc":"2.0","id":"x".repeat(3073),"method":"dialogue.edit.apply","params":{}});
    assert_eq!(
        server.dispatch(&oversized_id.to_string()).unwrap()["error"]["code"],
        -32600
    );
    fs::write(root.join("world.wl"), format!("{SOURCE}#外部保存\n")).unwrap();
    let rejected = rpc(
        &mut server,
        "dialogue.edit.preview",
        json!({"project_id":id,"request":request}),
    );
    assert_eq!(rejected["result"]["ok"], false, "{rejected}");
    assert_eq!(
        server.projects[&id]
            .project
            .document(&root.join("world.wl"))
            .unwrap(),
        SOURCE
    );
    assert!(fs::read_to_string(root.join("world.wl"))
        .unwrap()
        .contains("外部保存"));
    fs::remove_dir_all(root).unwrap();
}

use super::*;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

const SOURCE: &str = "let courage = 0\r\nlet ready = true\r\nevent start\r\n  choice once \"出发🙂\" if ((ready)) // 原注释\r\n    -> END\r\n";
struct Fixture {
    root: PathBuf,
    server: Server,
    id: String,
}
impl Fixture {
    fn new() -> Self {
        Self::with_source(SOURCE, None)
    }
    fn with_source(source: &str, manifest: Option<&str>) -> Self {
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
                "condition-rpc-{}-{nonce}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("world.wl"), source).unwrap();
        if let Some(manifest) = manifest {
            fs::create_dir(root.join(".world")).unwrap();
            fs::write(root.join(".world/project.json"), manifest).unwrap();
        }
        let mut server = Server::default();
        let opened = rpc(&mut server, "project.open", json!({"path":root}));
        assert_eq!(opened["result"]["ok"], true, "{opened}");
        let id = opened["result"]["project_id"].as_str().unwrap().to_owned();
        Self { root, server, id }
    }
    fn query(&mut self) -> Value {
        rpc(
            &mut self.server,
            "condition.authoring.query",
            json!({"project_id":self.id,"selector":selector()}),
        )
    }
    fn request(&mut self) -> Value {
        let query = self.query();
        assert_eq!(query["result"]["ok"], true, "{query}");
        json!({"schema_version":1,"expected_baseline":query["result"]["baseline"],
            "expected_generation":query["result"]["projection"]["generation"],"selector":selector(),
            "replacement":{"kind":"compare","variable":"courage","op":"ge",
                "value":{"kind":"num","value":1},"literal_left":false}})
    }
    fn preview(&mut self, request: &Value) -> Value {
        rpc(
            &mut self.server,
            "condition.authoring.preview",
            json!({"project_id":self.id,"request":request}),
        )
    }
    fn apply(&mut self, request: &Value, digest: &Value) -> Value {
        rpc(
            &mut self.server,
            "condition.authoring.apply",
            json!({"project_id":self.id,"request":request,"plan_digest":digest}),
        )
    }
    fn memory(&self) -> &str {
        self.server.projects[&self.id]
            .project
            .document(&self.root.join("world.wl"))
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn selector() -> Value {
    json!({"schema_version":1,"owner":{"target":{"kind":"event","id":"start"},"choice_line":4},"slot":"choice_display"})
}
fn rpc(server: &mut Server, method: &str, params: Value) -> Value {
    server
        .dispatch(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string())
        .unwrap()
}

#[test]
fn rpc_core_parity_atomic_memory_apply_explicit_save_and_stale_plan() {
    let mut fixture = Fixture::new();
    let request = fixture.request();
    let core_request = parse_condition_authoring_request(&request.to_string()).unwrap();
    let core_plan = fixture.server.projects[&fixture.id]
        .project
        .preview_condition_authoring(&core_request)
        .unwrap();
    let preview = fixture.preview(&request);
    assert_eq!(preview["result"]["plan"], json!(core_plan));
    assert_eq!(preview["result"]["applied"], false);
    assert_eq!(preview["result"]["saved"], false);
    assert_eq!(fixture.memory(), SOURCE);
    assert_eq!(
        fs::read(fixture.root.join("world.wl")).unwrap(),
        SOURCE.as_bytes()
    );
    let applied = fixture.apply(&request, &json!(core_plan.plan_digest));
    assert_eq!(applied["result"]["ok"], true, "{applied}");
    assert_eq!(applied["result"]["applied"], true);
    assert_eq!(applied["result"]["saved"], false);
    assert_eq!(applied["result"]["plan"], preview["result"]["plan"]);
    assert_eq!(
        fixture.server.projects[&fixture.id]
            .scene_revision
            .content_generation,
        1
    );
    assert_ne!(fixture.memory(), SOURCE);
    assert_eq!(
        fs::read(fixture.root.join("world.wl")).unwrap(),
        SOURCE.as_bytes()
    );
    let query = fixture.query();
    assert_eq!(
        query["result"]["projection"]["tree"],
        json!(core_request.replacement)
    );
    let stale = fixture.apply(&request, &json!(core_plan.plan_digest));
    assert_eq!(stale["result"]["ok"], false, "{stale}");
    assert_eq!(stale["result"]["applied"], false);
    let saved = rpc(
        &mut fixture.server,
        "project.save",
        json!({"project_id":fixture.id,"expected_baseline":applied["result"]["baseline"]}),
    );
    assert_eq!(saved["result"]["ok"], true, "{saved}");
    assert_eq!(
        fs::read_to_string(fixture.root.join("world.wl")).unwrap(),
        fixture.memory()
    );
    assert!(!fixture.root.join(".world/project.json").exists());
    assert_eq!(
        fixture.server.projects[&fixture.id]
            .project
            .compile_read_only()
            .unwrap()
            .options
            .language_version
            .as_str(),
        "1.9"
    );
}

#[test]
fn query_and_no_change_apply_never_rewrite_raw_source_or_increment_generation() {
    let mut fixture = Fixture::new();
    let mut request = fixture.request();
    request["replacement"] = json!({"kind":"global_bool","variable":"ready"});
    let preview = fixture.preview(&request);
    assert_eq!(preview["result"]["ok"], true, "{preview}");
    assert_eq!(preview["result"]["plan"]["changed"], false);
    assert_eq!(preview["result"]["plan"]["patch"], Value::Null);
    let applied = fixture.apply(&request, &preview["result"]["plan"]["plan_digest"]);
    assert_eq!(applied["result"]["ok"], true, "{applied}");
    assert_eq!(applied["result"]["applied"], false);
    assert_eq!(applied["result"]["saved"], false);
    assert_eq!(
        fixture.server.projects[&fixture.id]
            .scene_revision
            .content_generation,
        0
    );
    assert_eq!(fixture.memory(), SOURCE);
    assert_eq!(
        fs::read(fixture.root.join("world.wl")).unwrap(),
        SOURCE.as_bytes()
    );
}

#[test]
fn strict_unknown_nested_fields_and_bad_parameter_shapes_are_protocol_errors() {
    let mut fixture = Fixture::new();
    let original = fixture.request();
    for pointer in [
        "/unknown",
        "/selector/unknown",
        "/selector/owner/unknown",
        "/selector/owner/target/display",
        "/replacement/unknown",
        "/replacement/value/unknown",
    ] {
        let mut request = original.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        request.pointer_mut(parent).unwrap()[key] = json!(true);
        let failed = fixture.preview(&request);
        assert_eq!(failed["error"]["code"], -32602, "{failed}");
        assert!(failed.get("result").is_none());
    }
    for params in [
        json!({"path":fixture.root,"selector":selector()}),
        json!({"project_id":fixture.id,"path":fixture.root,"selector":selector()}),
        json!({"project_id":fixture.id,"selector":selector(),"save":true}),
        json!({"project_id":fixture.id,"selector":selector(),"request":original}),
        json!({"project_id":fixture.id,"selector":null}),
        json!({"project_id":"","selector":selector()}),
        json!({"project_id":"missing","selector":selector()}),
        json!({"project_id":true,"selector":selector()}),
        json!([]),
    ] {
        let failed = rpc(&mut fixture.server, "condition.authoring.query", params);
        assert_eq!(failed["error"]["code"], -32602, "{failed}");
    }
    let failed = rpc(
        &mut fixture.server,
        "condition.authoring.apply",
        json!({"project_id":fixture.id,"request":original}),
    );
    assert_eq!(failed["error"]["code"], -32602);
    assert_eq!(fixture.memory(), SOURCE);
}

#[test]
fn duplicate_keys_invalid_ids_and_oversized_ids_cannot_apply() {
    let mut fixture = Fixture::new();
    let request = fixture.request();
    let plan = fixture.preview(&request);
    let params = json!({"project_id":fixture.id,"request":request,"plan_digest":plan["result"]["plan"]["plan_digest"]});
    let duplicate =
        json!({"jsonrpc":"2.0","id":1,"method":"condition.authoring.apply","params":params})
            .to_string()
            .replacen(
                "\"schema_version\":1",
                "\"schema_version\":1,\"schema_version\":1",
                1,
            );
    assert_eq!(
        fixture.server.dispatch(&duplicate).unwrap()["error"]["code"],
        -32700
    );
    for id in [
        json!(true),
        json!({}),
        json!([]),
        json!("x".repeat(3073)),
        json!("\n".repeat(1536)),
    ] {
        let raw =
            json!({"jsonrpc":"2.0","id":id,"method":"condition.authoring.apply","params":params});
        let failed = fixture.server.dispatch(&raw.to_string()).unwrap();
        assert_eq!(failed["id"], Value::Null, "{failed}");
        assert_eq!(failed["error"]["code"], -32600, "{failed}");
        assert_eq!(fixture.memory(), SOURCE);
    }
    let raw = json!({"jsonrpc":"1.0","id":1,"method":"condition.authoring.apply","params":params});
    assert_eq!(
        fixture.server.dispatch(&raw.to_string()).unwrap()["error"]["code"],
        -32600
    );
    assert_eq!(
        fs::read(fixture.root.join("world.wl")).unwrap(),
        SOURCE.as_bytes()
    );
}

#[test]
fn notification_applies_once_without_response_and_never_saves() {
    let mut fixture = Fixture::new();
    let request = fixture.request();
    let plan = fixture.preview(&request);
    let notification = json!({"jsonrpc":"2.0","method":"condition.authoring.apply","params":{
        "project_id":fixture.id,"request":request,"plan_digest":plan["result"]["plan"]["plan_digest"]}});
    assert!(fixture.server.dispatch(&notification.to_string()).is_none());
    let applied = fixture.memory().to_owned();
    assert_ne!(applied, SOURCE);
    assert!(fixture.server.dispatch(&notification.to_string()).is_none());
    assert_eq!(fixture.memory(), applied);
    assert_eq!(
        fixture.server.projects[&fixture.id]
            .scene_revision
            .content_generation,
        1
    );
    assert_eq!(
        fs::read(fixture.root.join("world.wl")).unwrap(),
        SOURCE.as_bytes()
    );
}

#[test]
fn external_changes_and_stale_input_generations_are_business_failures() {
    let mut fixture = Fixture::new();
    let request = fixture.request();
    let plan = fixture.preview(&request);
    let mut stale = request.clone();
    stale["expected_generation"] = json!(999999);
    let failed = fixture.preview(&stale);
    assert_eq!(failed["result"]["ok"], false, "{failed}");
    assert_eq!(
        failed["result"]["error"]["code"],
        "CONDITION_AUTHORING_REJECTED"
    );
    assert!(failed.get("error").is_none());
    let external = format!("{SOURCE}// 外部稿\r\n");
    fs::write(fixture.root.join("world.wl"), &external).unwrap();
    let result = fixture.apply(&request, &plan["result"]["plan"]["plan_digest"]);
    assert_eq!(result["result"]["ok"], false, "{result}");
    assert_eq!(result["result"]["applied"], false);
    assert_eq!(fixture.memory(), SOURCE);
    assert_eq!(
        fs::read_to_string(fixture.root.join("world.wl")).unwrap(),
        external
    );
}

#[test]
fn condition_capability_is_additive_and_response_budget_includes_newline() {
    let capabilities = rpc(&mut Server::default(), "initialize", json!({}));
    let list = capabilities["result"]["capabilities"].as_array().unwrap();
    assert!(list.contains(&json!(
        worldline_core::condition_authoring::CONDITION_AUTHORING_CAPABILITY
    )));
    assert!(list.contains(&json!(
        worldline_runtime::OBJECT_STATE_INSPECTION_CAPABILITY
    )));
    assert!(list.contains(&json!("authoring.schema_authoring.v1")));
    assert_eq!(capabilities["result"]["protocol"], 1);
    assert!(fits(&"x".repeat(MAX_RESPONSE - 3)));
    assert!(!fits(&"x".repeat(MAX_RESPONSE - 2)));
    let oversized = json!({"jsonrpc":"2.0","id":1,"result":{"applied":true,"saved":false,"detail":"x".repeat(MAX_RESPONSE)}});
    let bounded = bounded_response(json!(1), oversized);
    assert_eq!(bounded["result"]["error"]["code"], "OUTPUT_LIMIT");
    assert_eq!(bounded["result"]["applied"], true);
    assert_eq!(bounded["result"]["saved"], false);
    let bounded = bounded_response(
        json!(1),
        err(json!(1), -32602, &"x".repeat(MAX_RESPONSE), Value::Null),
    );
    assert_eq!(bounded["error"]["code"], -32602);
    assert!(bounded.get("result").is_none());
}

#[test]
fn missing_owner_and_tampered_plan_are_business_failures_without_mutation() {
    let mut fixture = Fixture::new();
    let request = fixture.request();
    let rejected = fixture.apply(&request, &json!("changed-digest"));
    assert_eq!(
        rejected["result"]["error"]["code"],
        "CONDITION_AUTHORING_REJECTED"
    );
    assert_eq!(rejected["result"]["applied"], false);
    let mut missing = selector();
    missing["owner"]["target"]["id"] = json!("missing");
    let rejected = rpc(
        &mut fixture.server,
        "condition.authoring.query",
        json!({"project_id":fixture.id,"selector":missing}),
    );
    assert_eq!(
        rejected["result"]["error"]["code"],
        "CONDITION_AUTHORING_REJECTED"
    );
    assert!(rejected.get("error").is_none());
    assert_eq!(fixture.memory(), SOURCE);
    assert_eq!(
        fs::read(fixture.root.join("world.wl")).unwrap(),
        SOURCE.as_bytes()
    );
}

#[path = "tests/parity.rs"]
mod parity;

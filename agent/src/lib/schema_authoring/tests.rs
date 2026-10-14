use super::*;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

const SOURCE: &str = "schema city for entity entity_type place\n  field people population number required\nentity harbor kind place\n  property unknown = \"原始值\"\nevent start\n  -> END\n";
struct Fixture {
    root: PathBuf,
    server: Server,
    id: String,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rpc-schema-instance-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join(".world")).unwrap();
        fs::write(root.join("world.wl"), SOURCE).unwrap();
        fs::write(
            root.join(".world/project.json"),
            r#"{"schema_version":1,"language_version":"1.13","required_features":[]}"#,
        )
        .unwrap();
        let mut server = Server::default();
        let opened = rpc(&mut server, "project.open", json!({"path":root}));
        assert_eq!(opened["result"]["ok"], true, "{opened}");
        let id = opened["result"]["project_id"].as_str().unwrap().to_owned();
        Self { root, server, id }
    }
    fn query(&mut self) -> Value {
        rpc(
            &mut self.server,
            "schema.authoring.query",
            json!({"project_id":self.id,"target":{"kind":"entity","id":"harbor"}}),
        )
    }
    fn request(&mut self) -> Value {
        let query = self.query();
        assert_eq!(query["result"]["ok"], true, "{query}");
        json!({"schema_version":1,"expected_baseline":query["result"]["baseline"],
            "target":{"kind":"entity","id":"harbor"},"binding":{"op":"bind","schema_id":"city"},
            "fields":[{"op":"set","field_id":"people","key":"population","value":0}]})
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn rpc(server: &mut Server, method: &str, params: Value) -> Value {
    server
        .dispatch(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string())
        .unwrap()
}

#[test]
fn core_parity_atomic_memory_apply_explicit_save_and_stale_plan() {
    let mut fixture = Fixture::new();
    let request = fixture.request();
    let core_request = parse_schema_authoring_request(&request.to_string()).unwrap();
    let project = &fixture.server.projects[&fixture.id].project;
    let core_plan = project.preview_schema_authoring(&core_request).unwrap();
    let original_fingerprint = project.clone().compile().analysis.fingerprint;
    let preview = rpc(
        &mut fixture.server,
        "schema.authoring.preview",
        json!({"project_id":fixture.id,"request":request}),
    );
    assert_eq!(preview["result"]["plan"], json!(core_plan));
    assert_eq!(
        fs::read_to_string(fixture.root.join("world.wl")).unwrap(),
        SOURCE
    );
    let apply =
        json!({"project_id":fixture.id,"request":request,"plan_digest":core_plan.plan_digest});
    let result = rpc(&mut fixture.server, "schema.authoring.apply", apply.clone());
    assert_eq!(result["result"]["applied"], true, "{result}");
    assert_eq!(result["result"]["saved"], false);
    assert_eq!(
        fs::read_to_string(fixture.root.join("world.wl")).unwrap(),
        SOURCE
    );
    assert_eq!(
        fixture.server.projects[&fixture.id]
            .project
            .clone()
            .compile()
            .analysis
            .fingerprint,
        original_fingerprint
    );
    assert_eq!(
        fixture.query()["result"]["projection"]["fields"][0]["value"],
        json!(0.0)
    );
    assert_eq!(
        rpc(&mut fixture.server, "schema.authoring.apply", apply)["result"]["ok"],
        false
    );
    let saved = rpc(
        &mut fixture.server,
        "project.save",
        json!({"project_id":fixture.id,"expected_baseline":result["result"]["baseline"]}),
    );
    assert_eq!(saved["result"]["ok"], true, "{saved}");
    let reopened = Project::open(&fixture.root).unwrap();
    assert!(
        reopened
            .schema_authoring(&TargetRef::new("entity", "harbor"))
            .unwrap()
            .fields[0]
            .present
    );
}

#[test]
fn unknown_nested_fields_duplicate_keys_and_oversized_ids_are_rejected_before_apply() {
    let mut fixture = Fixture::new();
    let request = fixture.request();
    let plan = rpc(
        &mut fixture.server,
        "schema.authoring.preview",
        json!({"project_id":fixture.id,"request":request}),
    );
    let mut nested = request.clone();
    nested["target"]["display"] = json!("不可猜测");
    assert_eq!(
        rpc(
            &mut fixture.server,
            "schema.authoring.preview",
            json!({"project_id":fixture.id,"request":nested})
        )["error"]["code"],
        -32602
    );
    assert_eq!(
        rpc(
            &mut fixture.server,
            "schema.authoring.query",
            json!({"project_id":fixture.id,"path":fixture.root,"target":{"kind":"entity","id":"harbor"}})
        )["error"]["code"],
        -32602
    );
    let duplicate = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"schema.authoring.query","params":{{"project_id":"{}","target":{{"kind":"entity","id":"harbor","id":"harbor"}}}}}}"#,
        fixture.id
    );
    assert_eq!(
        fixture.server.dispatch(&duplicate).unwrap()["error"]["code"],
        -32700
    );
    let oversize = json!({"jsonrpc":"2.0","id":"x".repeat(3073),"method":"schema.authoring.apply","params":{"project_id":fixture.id,"request":request,"plan_digest":plan["result"]["plan"]["plan_digest"]}});
    let failed = fixture.server.dispatch(&oversize.to_string()).unwrap();
    assert_eq!(failed["id"], Value::Null);
    assert_eq!(failed["error"]["code"], -32600);
    assert_eq!(
        fixture.server.projects[&fixture.id]
            .project
            .document(&fixture.root.join("world.wl"))
            .unwrap(),
        SOURCE
    );
}

#[test]
fn external_changes_cannot_authorize_structured_write() {
    let mut fixture = Fixture::new();
    let request = fixture.request();
    let plan = rpc(
        &mut fixture.server,
        "schema.authoring.preview",
        json!({"project_id":fixture.id,"request":request}),
    );
    fs::write(
        fixture.root.join("world.wl"),
        format!("{SOURCE}// 外部稿\n"),
    )
    .unwrap();
    let result = rpc(
        &mut fixture.server,
        "schema.authoring.apply",
        json!({"project_id":fixture.id,"request":request,"plan_digest":plan["result"]["plan"]["plan_digest"]}),
    );
    assert_eq!(result["result"]["ok"], false, "{result}");
    assert_eq!(result["result"]["applied"], false);
    assert_eq!(
        fixture.server.projects[&fixture.id]
            .project
            .document(&fixture.root.join("world.wl"))
            .unwrap(),
        SOURCE
    );
    assert!(fs::read_to_string(fixture.root.join("world.wl"))
        .unwrap()
        .ends_with("// 外部稿\n"));
}

#[test]
fn response_budget_counts_outer_json_and_newline() {
    assert!(fits(&"x".repeat(MAX_RESPONSE - 3)));
    assert!(!fits(&"x".repeat(MAX_RESPONSE - 2)));
}

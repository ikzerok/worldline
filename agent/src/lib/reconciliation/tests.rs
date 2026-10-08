use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture {
    root: PathBuf,
    server: Server,
    id: String,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "worldline-reconciliation-rpc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("world.wl"), "event start\n  基线\n  -> END\n").unwrap();
        let mut server = Server::default();
        let opened = server
            .project_open(&json!({"path":root}))
            .unwrap_or_else(|error| panic!("{}", error.message));
        let id = opened["project_id"].as_str().unwrap().to_owned();
        let project = &mut server.projects.get_mut(&id).unwrap().project;
        project
            .set_text(
                &project.entry.clone(),
                "event start\n  本地\n  -> END\n".into(),
            )
            .unwrap();
        fs::write(&project.entry, "event start\n  磁盘\n  -> END\n").unwrap();
        Self { root, server, id }
    }
    fn preview(&mut self) -> Value {
        assert_eq!(
            self.server
                .reconciliation(&json!({"project_id":self.id}), "capture")
                .unwrap_or_else(|error| panic!("{}", error.message))["ok"],
            true
        );
        self.server.reconciliation(&json!({"project_id":self.id,"request":{"choices":[{"path":"world.wl","choice":{"kind":"local"}}]}}), "preview").unwrap_or_else(|error| panic!("{}", error.message))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn rpc_requires_captured_preview_and_separate_project_save() {
    let mut fixture = Fixture::new();
    let unreviewed = fixture
        .server
        .reconciliation(
            &json!({"project_id":fixture.id,"plan_digest":"0000000000000000"}),
            "apply",
        )
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert_eq!(unreviewed["ok"], false);
    let preview = fixture.preview();
    assert_eq!(preview["plan"]["can_apply"], true, "{preview}");
    let applied = fixture
        .server
        .reconciliation(
            &json!({"project_id":fixture.id,"plan_digest":preview["plan"]["plan_digest"]}),
            "apply",
        )
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert_eq!(applied["applied"], true, "{applied}");
    assert_eq!(applied["saved"], false);
    assert!(fs::read_to_string(fixture.root.join("world.wl"))
        .unwrap()
        .contains("磁盘"));
    let saved = fixture
        .server
        .project_save(&json!({"project_id":fixture.id,"expected_baseline":applied["baseline"]}))
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert_eq!(saved["saved"], true);
    assert!(fs::read_to_string(fixture.root.join("world.wl"))
        .unwrap()
        .contains("本地"));
    assert_eq!(
        fixture
            .server
            .reconciliation(
                &json!({"project_id":fixture.id,"plan_digest":preview["plan"]["plan_digest"]}),
                "apply"
            )
            .unwrap_or_else(|error| panic!("{}", error.message))["ok"],
        false
    );
}

#[test]
fn rpc_stale_disk_is_domain_failure_and_unknown_parameter_is_protocol_failure() {
    let mut fixture = Fixture::new();
    let preview = fixture.preview();
    fs::write(
        fixture.root.join("world.wl"),
        "event changed_again\n  -> END\n",
    )
    .unwrap();
    let response = fixture
        .server
        .reconciliation(
            &json!({"project_id":fixture.id,"plan_digest":preview["plan"]["plan_digest"]}),
            "apply",
        )
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert_eq!(response["ok"], false);
    assert_eq!(response["applied"], false);
    assert!(fixture
        .server
        .reconciliation(&json!({"project_id":fixture.id,"save":true}), "capture")
        .is_err());
}

#[test]
fn reconciliation_rpc_counts_unicode_and_escaping_in_full_id_before_actions() {
    for id in [json!("中".repeat(700)), json!("\n\"".repeat(600))] {
        let mut server = Server::default();
        let response = budget::dispatch(
            &mut server,
            &json!({"jsonrpc":"2.0","id":id,
            "method":"reconciliation.capture","params":{"project_id":"p1"}}),
        )
        .unwrap();
        assert!(response["id"].is_null());
        assert_eq!(response["error"]["code"], -32600);
        // 严格小于完整行上限，为最后一个换行字节保留空间；MAX不变。
        assert!(serde_json::to_vec(&response).unwrap().len() < budget::MAX_RESPONSE);
        assert!(server.projects.is_empty());
    }
}

#[test]
fn reconciliation_rpc_rejects_highly_escaped_params_before_clone_or_capture() {
    let mut fixture = Fixture::new();
    let before = fixture.server.projects[&fixture.id]
        .project
        .content_baseline();
    let text = "\0".repeat(6 * 1024 * 1024);
    let message = json!({"jsonrpc":"2.0","id":"request","method":"reconciliation.preview",
        "params":{"project_id":fixture.id,"request":{"choices":[{"path":"world.wl",
        "choice":{"kind":"manual","text":text}}]}}});
    let response = budget::dispatch(&mut fixture.server, &message).unwrap();
    assert_eq!(response["error"]["code"], -32602);
    assert_eq!(
        fixture.server.projects[&fixture.id]
            .project
            .content_baseline(),
        before
    );
    assert!(fixture.server.projects[&fixture.id]
        .reconciliation
        .session
        .is_none());
}

#[test]
fn reconciliation_rpc_large_raw_sides_fail_before_json_value_or_cache() {
    let mut fixture = Fixture::new();
    let path = fixture.root.join("world.wl");
    let large = "x".repeat(3 * 1024 * 1024);
    let baseline = format!("event start\n  -> END\n//base {large}\n");
    let local = format!("event start\n  -> END\n//local {large}\n");
    let disk = format!("event start\n  -> END\n//disk {large}\n");
    fs::write(&path, &baseline).unwrap();
    let project = &mut fixture
        .server
        .projects
        .get_mut(&fixture.id)
        .unwrap()
        .project;
    *project = Project::open_read_only(&fixture.root).unwrap();
    project.set_text(&path, local.clone()).unwrap();
    fs::write(&path, &disk).unwrap();
    let response = budget::dispatch(
        &mut fixture.server,
        &json!({"jsonrpc":"2.0","id":"large",
        "method":"reconciliation.capture","params":{"project_id":fixture.id}}),
    )
    .unwrap();
    assert_eq!(
        response["result"]["error"]["code"], "OUTPUT_LIMIT",
        "{response}"
    );
    assert_eq!(response["result"]["applied"], false);
    let unit = &fixture.server.projects[&fixture.id];
    assert!(unit.reconciliation.session.is_none());
    assert_eq!(unit.project.document(&path).unwrap(), local);
    assert_eq!(fs::read_to_string(path).unwrap(), disk);
    // 严格小于完整行上限，为最后一个换行字节保留空间；MAX不变。
    assert!(serde_json::to_vec(&response).unwrap().len() < budget::MAX_RESPONSE);
}

#[test]
fn reconciliation_rpc_valid_maximal_id_stays_within_complete_line_budget() {
    let mut fixture = Fixture::new();
    let response = budget::dispatch(
        &mut fixture.server,
        &json!({"jsonrpc":"2.0","id":"a".repeat(2046),
        "method":"reconciliation.capture","params":{"project_id":fixture.id}}),
    )
    .unwrap();
    assert_eq!(response["result"]["ok"], true);
    // 严格小于完整行上限，为最后一个换行字节保留空间；MAX不变。
    assert!(serde_json::to_vec(&response).unwrap().len() < budget::MAX_RESPONSE);
}

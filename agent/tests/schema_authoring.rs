use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{project::Project, source_edit::SourceEditRequest};
const SOURCE: &str = "schema city for entity entity_type place closed\n  field people population number required\nentity harbor kind place\n  property population = 0\nbind entity harbor to city\nevent start\n  -> END\n";
fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn exchange(requests: Vec<Value>) -> Vec<Value> {
    let input = requests
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = Vec::new();
    worldline_agent::run(&mut std::io::Cursor::new(input), &mut output);
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
#[test]
fn schema_rpc_index_preview_and_apply_share_core_and_preserve_rejected_edits() {
    let root = std::env::temp_dir().join(format!("wl-schema-rpc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.12","required_features":[]}"#,
    )
    .unwrap();
    std::fs::write(root.join("world.wl"), SOURCE).unwrap();
    let project = worldline_core::project::Project::open(&root).unwrap();
    let changed = SOURCE.replace("population number required", "population text required");
    let dto = json!({"schema_version":1,"path":"world.wl","expected_baseline":project.content_baseline(),"source":changed});
    let core_request: worldline_core::source_edit::SourceEditRequest =
        serde_json::from_value(dto.clone()).unwrap();
    let core_preview = project.preview_schema_edit(&core_request).unwrap();
    let rows = exchange(vec![
        request(1, "schema.index", json!({"path":root})),
        request(2, "schema.edit.preview", json!({"path":root,"request":dto})),
        request(
            3,
            "schema.edit.apply",
            json!({"path":root,"request":dto,"plan_digest":"wrong"}),
        ),
        request(4, "schema.index", json!({"path":root})),
    ]);
    assert_eq!(rows[0]["result"]["index"], json!(project.schema_index()));
    assert_eq!(rows[1]["result"]["preview"], json!(core_preview));
    assert_eq!(rows[1]["result"]["preview"]["complete"], true);
    assert_eq!(
        rows[1]["result"]["preview"]["incomplete_reasons"],
        json!([])
    );
    assert_eq!(rows[2]["result"]["ok"], false);
    assert_eq!(rows[2]["result"]["error"]["code"], "SCHEMA_EDIT_REJECTED");
    assert_eq!(rows[0]["result"]["baseline"], rows[3]["result"]["baseline"]);
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        SOURCE
    );
    let rows = exchange(vec![
        request(
            1,
            "schema.edit.apply",
            json!({"path":root,"request":dto,"plan_digest":core_preview.plan_digest}),
        ),
        request(
            2,
            "schema.edit.apply",
            json!({"path":root,"request":dto,"plan_digest":core_preview.plan_digest}),
        ),
    ]);
    assert_eq!(rows[0]["result"]["ok"], true, "{}", rows[0]);
    assert_eq!(rows[0]["result"]["preview"], json!(core_preview));
    assert_eq!(rows[1]["result"]["ok"], false);
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        changed
    );
    std::fs::remove_dir_all(root).unwrap();
}

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, source: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-schema-rpc-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".world")).unwrap();
        fs::write(
            root.join(".world/project.json"),
            r#"{"schema_version":1,"language_version":"1.12","required_features":[]}"#,
        )
        .unwrap();
        fs::write(root.join("world.wl"), source).unwrap();
        Self(root)
    }

    fn project(&self) -> Project {
        Project::open(&self.0).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn edit_request(project: &Project, source: &str) -> SourceEditRequest {
    SourceEditRequest {
        schema_version: 1,
        path: "world.wl".into(),
        expected_baseline: project.content_baseline(),
        source: source.into(),
    }
}

#[test]
fn schema_rpc_repair_stays_incomplete_until_the_next_session_preview() {
    let source = format!("include \"missing.wl\"\n{SOURCE}");
    let fixture = Fixture::new("repair", &source);
    let mut project = fixture.project();
    let repair = edit_request(&project, SOURCE);
    let repairing = project.preview_schema_edit(&repair).unwrap();
    assert!(!repairing.complete);
    assert!(repairing
        .before_diagnostics
        .iter()
        .any(|d| d.code == "A105"));
    assert!(!repairing.after_diagnostics.iter().any(|d| d.code == "A105"));
    project
        .apply_schema_edit(&repair, &repairing.plan_digest)
        .unwrap();
    let changed = SOURCE.replace("population number", "population text");
    let next = edit_request(&project, &changed);
    let complete = project.preview_schema_edit(&next).unwrap();
    assert!(complete.complete);
    assert!(complete
        .after_diagnostics
        .iter()
        .any(|d| d.code == "SCH005"));
    let rows = exchange(vec![
        request(1, "project.open", json!({"path":fixture.0})),
        request(
            2,
            "schema.edit.preview",
            json!({"project_id":"p1","request":repair}),
        ),
        request(
            3,
            "schema.edit.apply",
            json!({"project_id":"p1","request":repair,"plan_digest":repairing.plan_digest}),
        ),
        request(
            4,
            "schema.edit.preview",
            json!({"project_id":"p1","request":next}),
        ),
        request(
            5,
            "schema.edit.apply",
            json!({"project_id":"p1","request":next,"plan_digest":complete.plan_digest}),
        ),
    ]);
    assert_eq!(rows[0]["result"]["ok"], false, "缺失源码仍可打开创作会话");
    assert_eq!(rows[0]["result"]["project_id"], "p1");
    for row in &rows[1..] {
        assert!(row.get("error").is_none(), "{row}");
        assert_eq!(row["result"]["ok"], true, "{row}");
    }
    for index in [1, 2] {
        assert_eq!(rows[index]["result"]["preview"], json!(repairing));
        assert_eq!(
            rows[index]["result"]["preview"]["incomplete_reasons"],
            json!(["source_loading"])
        );
    }
    for index in [3, 4] {
        assert_eq!(rows[index]["result"]["preview"], json!(complete));
        assert_eq!(
            rows[index]["result"]["preview"]["incomplete_reasons"],
            json!([])
        );
    }
    assert_eq!(
        fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
        changed
    );
    assert_eq!(
        rows[4]["result"]["baseline"],
        fixture.project().content_baseline()
    );
}

#[test]
fn schema_rpc_preserves_core_reason_categories_and_known_semantic_errors() {
    let cases = [
        (format!("{SOURCE}period\n"), Some("syntax"), "P"),
        (
            SOURCE.replace(
                "property population = 0",
                "property population = 0\n  property population = 1",
            ),
            Some("ambiguous_declaration"),
            "A212",
        ),
        (
            SOURCE.replace(
                "field people population number required",
                "field incomplete",
            ),
            Some("schema_definition"),
            "SCH001",
        ),
        (
            SOURCE
                .replace("population number", "population text")
                .replace("-> END", "-> missing"),
            None,
            "A101",
        ),
        (
            format!("{SOURCE}asset outside file \"../outside.wl\"\n"),
            None,
            "A109",
        ),
    ];
    for (candidate, reason, diagnostic) in cases {
        let fixture = Fixture::new("reasons", SOURCE);
        let project = fixture.project();
        let edit = edit_request(&project, &candidate);
        let core = project.preview_schema_edit(&edit).unwrap();
        assert!(
            core.after_diagnostics
                .iter()
                .any(|d| d.code.starts_with(diagnostic)),
            "{:?}",
            core.after_diagnostics
        );
        let rows = exchange(vec![request(
            1,
            "schema.edit.preview",
            json!({"path":fixture.0,"request":edit}),
        )]);
        let result = &rows[0]["result"];
        assert_eq!(result["ok"], true, "{}", rows[0]);
        assert_eq!(result["preview"], json!(core));
        assert_eq!(result["preview"]["complete"], reason.is_none());
        assert_eq!(
            result["preview"]["incomplete_reasons"],
            json!(reason.into_iter().collect::<Vec<_>>())
        );
        assert_eq!(result["baseline"], edit.expected_baseline);
        assert_eq!(
            fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
            SOURCE
        );
    }
}

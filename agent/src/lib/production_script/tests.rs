use super::*;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::localization::{
    LocalizationCatalogQuery, LocalizationEdit, LocalizationEditDraft, LocalizationPart,
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const SOURCE: &str = "character actor\ncharacter other\nfragment shared()\n  say other \"别人的片段\" direction \"SECRET_FRAGMENT\" #wl-localization:other\n  return\nevent start\n  say actor \"=1+2,中文🙂\" direction \"SECRET_DIRECTION\" #wl-localization:hello\n  call shared()\n  -> END\n";
fn fixture() -> (PathBuf, Server, String) {
    let root = std::env::temp_dir().join(format!(
        "v034-rpc-production-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), SOURCE).unwrap();
    fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.11","entry":"world.wl","required_features":["content.localization.v1"]}"#).unwrap();
    let mut server = Server::default();
    let opened = rpc(&mut server, "project.open", json!({"path":root}));
    assert_eq!(opened["result"]["ok"], true, "{opened}");
    let id = opened["result"]["project_id"].as_str().unwrap().to_owned();
    (root, server, id)
}
fn rpc(server: &mut Server, method: &str, params: Value) -> Value {
    server
        .dispatch(
            &json!({"jsonrpc":"2.0","id":"台本🙂","method":method,"params":params}).to_string(),
        )
        .unwrap()
}
fn request() -> Value {
    json!({"schema_version":1,"scope":{"kind":"current_target","target":{"kind":"event","id":"start"}},"speaker":{"kind":"character","id":"actor"}})
}
fn export(server: &mut Server, id: &str, request: &Value, format: &str, direction: bool) -> Value {
    rpc(
        server,
        "production.script.export",
        json!({"project_id":id,"request":request,
        "options":{"schema_version":1,"format":format,"include_direction":direction}}),
    )
}

#[test]
fn selected_role_and_all_formats_match_core_without_raw_private_side_channels() {
    let (root, mut server, id) = fixture();
    let queried = rpc(
        &mut server,
        "production.script.query",
        json!({"project_id":id,"request":request(),"limit":1}),
    );
    assert_eq!(queried["result"]["ok"], true, "{queried}");
    assert_eq!(queried["result"]["page"]["total"], 1);
    assert!(!queried.to_string().contains("SECRET"));
    assert!(!queried.to_string().contains("别人的片段"));
    let mut guarded = request();
    guarded["expected_snapshot_key"] = queried["result"]["page"]["snapshot_key"].clone();
    for format in ["json", "markdown", "csv"] {
        let exported = export(&mut server, &id, &guarded, format, false);
        assert_eq!(exported["result"]["ok"], true, "{exported}");
        let text = exported["result"]["artifact"]["text"].as_str().unwrap();
        assert!(!text.contains("SECRET"));
        assert!(!text.contains("别人的片段"));
        assert_eq!(exported["result"]["artifact"]["byte_count"], text.len());
        assert_eq!(exported["result"]["delivered"], false);
        assert_eq!(exported["result"]["saved"], false);
        let snapshot = server.projects[&id]
            .project
            .production_script_snapshot(&[], &[], &serde_json::from_value(guarded.clone()).unwrap())
            .unwrap();
        let artifact = snapshot
            .export(&serde_json::from_value(json!({"schema_version":1,"format":format})).unwrap())
            .unwrap();
        assert_eq!(text.as_bytes(), artifact.bytes());
        if format == "csv" {
            assert!(text.starts_with("\"'"));
            assert!(text.contains("\r\n"));
        }
        if format == "json" {
            let document: Value = serde_json::from_str(text).unwrap();
            assert!(document["rows"][0].get("direction").is_none());
        }
    }
    let included = export(&mut server, &id, &guarded, "json", true);
    assert!(included["result"]["artifact"]["text"]
        .as_str()
        .unwrap()
        .contains("SECRET_DIRECTION"));
    assert!(!included.to_string().contains("SECRET_FRAGMENT"));
    assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), SOURCE);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn locale_strict_is_selected_unit_scoped_and_fallback_is_explicit() {
    let (root, mut server, id) = fixture();
    let project = &mut server.projects.get_mut(&id).unwrap().project;
    let catalog = project
        .query_localization_catalog(&LocalizationCatalogQuery::default())
        .unwrap();
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.id.as_deref() == Some("hello"))
        .unwrap();
    let draft = LocalizationEditDraft {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh-Hans".into(),
        source_baseline: catalog.source_baseline,
        edits: vec![LocalizationEdit {
            id: "hello".into(),
            source_revision: entry.source_revision.clone().unwrap(),
            translation_parts: vec![LocalizationPart::Text {
                text: "译文🙂".into(),
            }],
        }],
    };
    let plan = project.preview_localization_edit(&draft).unwrap();
    project
        .apply_localization_edit(&draft, &plan.plan_digest)
        .unwrap();
    let mut selected = request();
    selected["target_locale"] = json!("zh-Hans");
    let translated = export(&mut server, &id, &selected, "json", false);
    assert_eq!(translated["result"]["ok"], true, "{translated}");
    assert!(translated.to_string().contains("译文🙂"));
    selected["speaker"] = Value::Null;
    let strict = export(&mut server, &id, &selected, "json", false);
    assert_eq!(strict["result"]["ok"], false, "{strict}");
    assert_eq!(strict["result"]["error"]["code"], "LOCALE_INCOMPLETE");
    selected["locale_policy"] = json!("source_fallback");
    let fallback = export(&mut server, &id, &selected, "json", false);
    assert_eq!(fallback["result"]["ok"], true, "{fallback}");
    let doc: Value =
        serde_json::from_str(fallback["result"]["artifact"]["text"].as_str().unwrap()).unwrap();
    assert!(doc["rows"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["used_source_fallback"] == true && row["status"] == "missing"));
    assert!(!root.join(".world/localization/zh-Hans.json").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn protocol_errors_and_business_budgets_have_distinct_domains() {
    let (root, mut server, id) = fixture();
    let mut bad = request();
    bad["speaker"]["extra"] = json!(true);
    assert_eq!(
        rpc(
            &mut server,
            "production.script.query",
            json!({"project_id":id,"request":bad})
        )["error"]["code"],
        -32602
    );
    assert_eq!(
        rpc(
            &mut server,
            "production.script.query",
            json!({"project_id":id,"request":request(),"offset":-1})
        )["error"]["code"],
        -32602
    );
    assert_eq!(
        rpc(
            &mut server,
            "production.script.export",
            json!({"project_id":id,"request":request(),"options":{"schema_version":1,"format":"json"},"limit":1})
        )["error"]["code"],
        -32602
    );
    let mut over = request();
    over["speaker"] = Value::Null;
    over["limits"] = json!({"rows":1});
    let rejected = rpc(
        &mut server,
        "production.script.query",
        json!({"project_id":id,"request":over}),
    );
    assert!(rejected.get("error").is_none());
    assert_eq!(rejected["result"]["error"]["code"], "BUDGET_EXCEEDED");
    let mut stale = request();
    stale["expected_snapshot_key"] = json!("old");
    assert_eq!(
        export(&mut server, &id, &stale, "json", false)["result"]["ok"],
        false
    );
    assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), SOURCE);
    fs::remove_dir_all(root).unwrap();
}

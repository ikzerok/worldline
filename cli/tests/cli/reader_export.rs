use super::common::*;
use serde_json::json;

#[test]
fn reader_export_cli_previews_applies_and_rejects_stale_plans() {
    let workspace = temp_workspace(
        "reader-export-cli",
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":[]}"#,
        "event intro as \"CLI Public Title\"\n  CLI public prose.\n  -> END\n",
    );
    let entry = workspace.join("world.wl");
    let selection = json!({
        "schema_version": 1,
        "site_title": "CLI Reader",
        "objects": [{"kind":"event", "id":"intro"}],
        "manuscripts": [],
        "attachments": []
    })
    .to_string();
    let mut out = Vec::new();
    let preview_code = wl::run(
        &[
            "reader-export".into(),
            "preview".into(),
            entry.to_string_lossy().into_owned(),
            "--selection-json".into(),
            selection.clone(),
            "--json".into(),
        ],
        &mut out,
        &mut std::io::Cursor::new(Vec::<u8>::new()),
    )
    .unwrap();
    assert_eq!(preview_code, 0);
    let preview = json_lines(&out).remove(0);
    assert_eq!(preview["ok"], true);
    let digest = preview["plan"]["plan_digest"].as_str().unwrap().to_string();

    let destination =
        std::env::temp_dir().join(format!("wl-reader-export-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&destination);
    out.clear();
    let apply_code = wl::run(
        &[
            "reader-export".into(),
            "apply".into(),
            entry.to_string_lossy().into_owned(),
            "--selection-json".into(),
            selection.clone(),
            "--plan-digest".into(),
            digest.clone(),
            "--out".into(),
            destination.to_string_lossy().into_owned(),
            "--json".into(),
        ],
        &mut out,
        &mut std::io::Cursor::new(Vec::<u8>::new()),
    )
    .unwrap();
    assert_eq!(apply_code, 0, "{}", String::from_utf8_lossy(&out));
    let applied = json_lines(&out).remove(0);
    assert_eq!(applied["ok"], true);
    assert!(destination.join("search.html").is_file());

    std::fs::write(
        &entry,
        "event intro as \"Changed Title\"\n  Changed prose.\n  -> END\n",
    )
    .unwrap();
    let stale_destination =
        std::env::temp_dir().join(format!("wl-reader-export-stale-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stale_destination);
    out.clear();
    let stale_code = wl::run(
        &[
            "reader-export".into(),
            "apply".into(),
            entry.to_string_lossy().into_owned(),
            "--selection-json".into(),
            selection,
            "--plan-digest".into(),
            digest,
            "--out".into(),
            stale_destination.to_string_lossy().into_owned(),
            "--json".into(),
        ],
        &mut out,
        &mut std::io::Cursor::new(Vec::<u8>::new()),
    )
    .unwrap();
    assert_eq!(stale_code, 1);
    assert_eq!(json_lines(&out)[0]["error"]["code"], "STALE_PLAN");
    assert!(!stale_destination.exists());
    let _ = std::fs::remove_dir_all(destination);
    let _ = std::fs::remove_dir_all(stale_destination);
    let _ = std::fs::remove_dir_all(workspace);
}

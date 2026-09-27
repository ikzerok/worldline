use super::common::*;

#[test]
fn workspace_check_json_reports_revision_and_diagnostic_domains() {
    let root = temp_presentation_project("workspace-check");
    let (code, out) = run_args(&[
        "workspace",
        "check",
        root.to_string_lossy().as_ref(),
        "--json",
    ]);
    assert_eq!(code.unwrap(), 0);
    let value = json_lines(&out).remove(0);
    assert_eq!(value["ok"], true);
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["language_version"], "1.10");
    assert!(value["workspace_revision"].as_str().is_some());
    assert!(value["diagnostics"].is_array());
    assert!(value["workspace_diagnostics"].is_array());
    assert_eq!(value["read_only"], false);
    assert_eq!(value["truncated"], false);
    assert!(value["continuation"].is_null());
    assert_eq!(value["stats"]["events"], 1);
}

#[test]
fn maps_list_json_exposes_maps_and_target_references() {
    let root = temp_presentation_project("maps-list");
    let (code, out) = run_args(&["maps", "list", root.to_string_lossy().as_ref(), "--json"]);
    assert_eq!(code.unwrap(), 0);
    let value = json_lines(&out).remove(0);
    assert_eq!(value["ok"], true);
    assert_eq!(value["maps"]["overview"]["title"], "总览");
    assert_eq!(
        value["references"][0]["target"],
        serde_json::json!({"kind":"entity","id":"lighthouse"})
    );
    assert_eq!(
        value["references"][0]["placements"][0],
        serde_json::json!({"map_id":"overview","placement_id":"lighthouse_marker"})
    );
    assert_eq!(value["truncated"], false);
    assert!(value["workspace_revision"].as_str().is_some());
}

#[test]
fn read_only_workspace_diagnostics_stay_separate_and_fail_check() {
    let cases = [
        (
            "unknown-language",
            r#"{"schema_version":1,"language_version":"2.0","required_features":[]}"#,
        ),
        (
            "unknown-feature",
            r#"{"schema_version":1,"language_version":"1.10","required_features":["future.entities.v2"]}"#,
        ),
    ];
    for (name, manifest) in cases {
        let root = temp_workspace(name, manifest, "event start\n  -> END\n");
        let path = root.to_string_lossy().to_string();
        let mut out = Vec::new();
        let code = wl::run(
            &["check".into(), path.clone(), "--json".into()],
            &mut out,
            &mut std::io::Cursor::new(Vec::new()),
        )
        .unwrap();
        let check: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(code, 1, "{name}: {check}");
        assert_eq!(check["ok"], false, "{name}: {check}");
        assert_eq!(check["read_only"], true, "{name}: {check}");
        assert!(check["diagnostics"].as_array().unwrap().is_empty());
        assert_eq!(check["workspace_diagnostics"][0]["code"], "WS003");

        let mut out = Vec::new();
        let human_code = wl::run(
            &["check".into(), path.clone()],
            &mut out,
            &mut std::io::Cursor::new(Vec::new()),
        )
        .unwrap();
        let human = String::from_utf8(out).unwrap();
        assert_eq!(human_code, 1, "{name}: {human}");
        assert!(human.contains("WS003"), "{name}: {human}");
        assert!(human.contains("工作区只读"), "{name}: {human}");

        let mut out = Vec::new();
        let catalog_code = wl::run(
            &["catalog".into(), path, "--json".into()],
            &mut out,
            &mut std::io::Cursor::new(Vec::new()),
        )
        .unwrap();
        let catalog: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(catalog_code, 0, "{name}: {catalog}");
        assert_eq!(catalog["read_only"], true, "{name}: {catalog}");
        assert!(catalog["diagnostics"].as_array().unwrap().is_empty());
        assert_eq!(catalog["workspace_diagnostics"][0]["code"], "WS003");
    }
}

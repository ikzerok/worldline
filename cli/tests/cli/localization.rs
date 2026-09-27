use super::common::*;
use serde_json::{json, Value};

#[test]
fn localization_cli_exports_and_imports_only_the_explicit_translation_selection() {
    let workspace = temp_workspace(
        "localization-roundtrip",
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.localization.v1"]}"#,
        "let traveler = \"Ari\"\nevent greeting\n  Hello {traveler} [[event:secret|Harbor]] #wl-localization:greeting\n  -> END\nevent secret\n  -> END\n",
    );
    let path = workspace.to_string_lossy().into_owned();
    let selection = json!({
        "schema_version": 1,
        "source_locale": "en",
        "target_locale": "zh-Hant",
        "string_ids": ["greeting"]
    })
    .to_string();
    let (preview_code, out) = run_args(&[
        "localization",
        "export",
        "preview",
        &path,
        "--selection-json",
        &selection,
        "--json",
    ]);
    assert_eq!(
        preview_code.unwrap(),
        0,
        "{}",
        String::from_utf8_lossy(&out)
    );
    let preview = json_lines(&out).remove(0);
    assert_eq!(preview["ok"], true);
    assert_eq!(preview["plan"]["can_export"], true);
    let mut exchange = preview["plan"]["exchange"].clone();
    let package_text = exchange.to_string();
    assert!(!package_text.contains("traveler"));
    assert!(!package_text.contains("secret"));
    let translation_parts = exchange["entries"][0]["source_parts"]
        .as_array()
        .unwrap()
        .iter()
        .cloned()
        .map(|mut part| {
            match part["type"].as_str().unwrap() {
                "text" => part["text"] = json!("歡迎，"),
                "link" => part["label"] = json!("港口譯名"),
                "placeholder" => {}
                other => panic!("unexpected source part {other}"),
            }
            part
        })
        .collect::<Vec<_>>();
    exchange["entries"][0]["translation_parts"] = json!(translation_parts);
    let package = workspace.parent().unwrap().join(format!(
        "{}-translation.json",
        workspace.file_name().unwrap().to_string_lossy()
    ));
    std::fs::write(&package, serde_json::to_vec(&exchange).unwrap()).unwrap();
    let package_path = package.to_string_lossy().into_owned();

    let (import_preview_code, out) = run_args(&[
        "localization",
        "import",
        "preview",
        &path,
        "--selection-json",
        &selection,
        "--package",
        &package_path,
        "--json",
    ]);
    assert_eq!(
        import_preview_code.unwrap(),
        0,
        "{}",
        String::from_utf8_lossy(&out)
    );
    let import_preview = json_lines(&out).remove(0);
    assert_eq!(import_preview["ok"], true);
    assert_eq!(import_preview["plan"]["can_apply"], true);
    let digest = import_preview["plan"]["plan_digest"]
        .as_str()
        .unwrap()
        .to_string();

    let (apply_code, out) = run_args(&[
        "localization",
        "import",
        "apply",
        &path,
        "--selection-json",
        &selection,
        "--package",
        &package_path,
        "--plan-digest",
        &digest,
        "--json",
    ]);
    assert_eq!(apply_code.unwrap(), 0, "{}", String::from_utf8_lossy(&out));
    let applied = json_lines(&out).remove(0);
    assert_eq!(applied["ok"], true);
    assert_eq!(applied["changed_files"].as_array().unwrap().len(), 2);
    let sidecar = workspace.join(".world/localization/zh-Hant.json");
    let sidecar: Value = serde_json::from_slice(&std::fs::read(sidecar).unwrap()).unwrap();
    assert_eq!(
        sidecar["entries"]["greeting"]["translation_parts"][0]["text"],
        "歡迎，"
    );
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(workspace.join(".world/project.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest["localizations"]["zh-Hant"],
        ".world/localization/zh-Hant.json"
    );
    assert!(std::fs::read_to_string(workspace.join("world.wl"))
        .unwrap()
        .contains("#wl-localization:greeting"));

    let _ = std::fs::remove_file(package);
    let _ = std::fs::remove_dir_all(workspace);
}

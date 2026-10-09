use super::*;

fn locale_document(root: &std::path::Path, duplicate_header: bool) {
    let manifest_path = root.join(".world/project.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["localizations"] = json!({"zh-Hans":".world/localization/zh-Hans.json"});
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::create_dir_all(root.join(".world/localization")).unwrap();
    let header = if duplicate_header {
        "\"schema_version\":1,"
    } else {
        ""
    };
    fs::write(root.join(".world/localization/zh-Hans.json"), format!("{{{header}\"schema_version\":1,\"required_features\":[\"content.localization.v1\"],\"source_locale\":\"en\",\"target_locale\":\"zh-Hans\",\"entries\":{{}}}}" )).unwrap();
}

#[test]
fn catalog_forwards_core_typed_read_only_without_changing_repairable_files() {
    let root = fixture("readonly", SOURCE);
    locale_document(&root, true);
    let path = root.join(".world/localization/zh-Hans.json");
    let before = fs::read(&path).unwrap();
    let query = LocalizationCatalogQuery {
        target_locale: Some("zh-Hans".into()),
        ..Default::default()
    };
    let (code, response) = author(&root, "catalog", None, json!(query), None, false);
    assert_eq!(code, 0, "{response}");
    assert_eq!(response["page"]["read_only"], true);
    assert_eq!(response["read_only"], response["page"]["read_only"]);
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn source_fallback_is_explicit_and_keeps_real_missing_status() {
    let root = fixture("fallback", SOURCE);
    locale_document(&root, false);
    let mut args = vec![
        "play".into(),
        root.to_string_lossy().into_owned(),
        "--json".into(),
        "--seed".into(),
        "71".into(),
        "--locale".into(),
        "zh-Hans".into(),
    ];
    let (code, strict) = run(&args, "0\n");
    assert_eq!(code, 1, "{strict:?}");
    assert_eq!(strict[0]["type"], "run_error");
    args.extend(["--locale-fallback".into(), "source".into()]);
    let (code, fallback) = run(&args, "0\n");
    assert_eq!(code, 0, "{fallback:?}");
    assert_eq!(fallback[0]["outputs"][0]["content"], "Hello Ari!");
    assert_eq!(
        fallback[0]["outputs"][0]["localization"]["status"],
        "missing_translation"
    );
    assert_eq!(fallback[0]["choices"][0]["label"], "Continue");
}

#[test]
fn candidate_import_is_memory_only_while_old_import_keeps_immediate_save() {
    let root = fixture("legacy", SOURCE);
    let project = Project::open(&root).unwrap();
    let selection = LocalizationSelection {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh-Hans".into(),
        string_ids: vec!["greeting".into()],
    };
    let export = project.preview_localization_export(&selection).unwrap();
    let mut exchange = export.exchange;
    exchange.entries[0].translation_parts = Some(
        exchange.entries[0]
            .source_parts
            .iter()
            .map(|part| match part {
                LocalizationPart::Text { text } => LocalizationPart::Text {
                    text: text.replace("Hello", "你好"),
                },
                other => other.clone(),
            })
            .collect(),
    );
    let request = json!({"selection":selection,"exchange":exchange});
    let (code, preview) = author(
        &root,
        "import-candidate",
        Some("preview"),
        request.clone(),
        None,
        false,
    );
    assert_eq!(code, 0, "{preview}");
    let (code, applied) = author(
        &root,
        "import-candidate",
        Some("apply"),
        request,
        preview["plan"]["plan_digest"].as_str(),
        false,
    );
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["applied"], true);
    assert_eq!(applied["saved"], false);
    assert!(!root.join(".world/localization/zh-Hans.json").exists());
    let package = root.parent().unwrap().join(format!(
        "v033-legacy-package-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&package, serde_json::to_vec(&exchange).unwrap()).unwrap();
    let args = vec![
        "localization".into(),
        "import".into(),
        "preview".into(),
        root.to_string_lossy().into_owned(),
        "--selection-json".into(),
        json!(selection).to_string(),
        "--package".into(),
        package.to_string_lossy().into_owned(),
        "--json".into(),
    ];
    let (code, old_preview) = run(&args, "");
    assert_eq!(code, 0, "{old_preview:?}");
    let mut apply_args = args;
    apply_args[2] = "apply".into();
    apply_args.extend([
        "--plan-digest".into(),
        old_preview[0]["plan"]["plan_digest"]
            .as_str()
            .unwrap()
            .into(),
    ]);
    let (code, old_applied) = run(&apply_args, "");
    assert_eq!(code, 0, "{old_applied:?}");
    assert!(
        fs::read_to_string(root.join(".world/localization/zh-Hans.json"))
            .unwrap()
            .contains("你好")
    );
    assert_eq!(fs::read(root.join("world.wl")).unwrap(), SOURCE.as_bytes());
}

use super::*;
#[test]
fn catalog_query_rpc_uses_core_cursor_and_returns_stale_cursor_errors() {
    let root = temp_entity_project(
        "catalog-query-rpc",
        "entity harbor kind place as \"港口\"\nentity lighthouse kind place as \"灯塔\"\nevent start\n  -> END\n",
    );
    let path = root.to_string_lossy().to_string();
    let query = json!({
        "schema_version": 1,
        "filters": [{"dimension": "kind", "values": ["entity"]}]
    });
    let (_, first_responses) = exchange(&[
        req(1, "project.open", json!({"path": path.clone()})),
        req(
            2,
            "catalog.query",
            json!({"project_id":"p1", "query":query.clone(), "page_size":1}),
        ),
        req(3, "shutdown", json!({})),
    ]);
    let first = &first_responses[1]["result"];
    assert_eq!(first["ok"], true, "{first:?}");
    assert_eq!(first["query"]["total"], 2);
    assert_eq!(first["query"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(first["read_only"], false);
    let cursor = first["query"]["next"].clone();
    assert!(cursor.is_object());

    let (_, second_responses) = exchange(&[
        req(
            1,
            "catalog.query",
            json!({"path":path.clone(), "query":query.clone(), "cursor":cursor.clone()}),
        ),
        req(2, "shutdown", json!({})),
    ]);
    let second = &second_responses[0]["result"];
    assert_eq!(second["ok"], true, "{second:?}");
    assert_eq!(second["query"]["offset"], 1);
    assert_eq!(second["query"]["items"].as_array().unwrap().len(), 1);

    std::fs::write(
        root.join("world.wl"),
        "entity harbor kind place as \"港口\"\nentity lighthouse kind place as \"灯塔\"\nentity island kind place as \"岛屿\"\nevent start\n  -> END\n",
    )
    .unwrap();
    let (_, stale_responses) = exchange(&[
        req(
            1,
            "catalog.query",
            json!({"path":path, "query":query, "cursor":cursor}),
        ),
        req(2, "shutdown", json!({})),
    ]);
    let stale = &stale_responses[0]["result"];
    assert_eq!(stale["ok"], false, "{stale:?}");
    assert_eq!(stale["error"]["code"], "STALE_CURSOR");
    assert!(stale["query"].is_null());
}

#[test]
fn reader_export_rpc_previews_exports_and_rejects_stale_plans() {
    let root = temp_workspace(
        "reader-export-rpc",
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":[]}"#,
        "event intro as \"RPC Public Title\"\n  RPC public prose.\n  -> END\n",
    );
    let path = root.to_string_lossy().to_string();
    let selection = json!({
        "schema_version": 1,
        "site_title": "RPC Reader",
        "objects": [{"kind":"event", "id":"intro"}],
        "manuscripts": [],
        "attachments": []
    });
    let (_, preview_responses) = exchange(&[
        req(1, "project.open", json!({"path":path.clone()})),
        req(
            2,
            "reader.export.preview",
            json!({"project_id":"p1", "selection":selection.clone()}),
        ),
        req(3, "shutdown", json!({})),
    ]);
    let preview = &preview_responses[1]["result"];
    assert_eq!(preview["ok"], true, "{preview:?}");
    assert_eq!(preview["operation"], "preview");
    let digest = preview["plan"]["plan_digest"].as_str().unwrap().to_string();

    let destination = std::env::temp_dir().join(format!(
        "worldline-reader-export-rpc-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&destination);
    let (_, apply_responses) = exchange(&[
        req(1, "project.open", json!({"path":path.clone()})),
        req(
            2,
            "reader.export.apply",
            json!({
                "project_id":"p1",
                "selection":selection.clone(),
                "plan_digest":digest.clone(),
                "output":destination.to_string_lossy().to_string(),
            }),
        ),
        req(3, "shutdown", json!({})),
    ]);
    let applied = &apply_responses[1]["result"];
    assert_eq!(applied["ok"], true, "{applied:?}");
    assert!(destination.join("index.html").is_file());

    std::fs::write(
        root.join("world.wl"),
        "event intro as \"Changed Title\"\n  Changed prose.\n  -> END\n",
    )
    .unwrap();
    let stale_destination = std::env::temp_dir().join(format!(
        "worldline-reader-export-stale-rpc-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&stale_destination);
    let (_, stale_responses) = exchange(&[
        req(
            1,
            "reader.export.apply",
            json!({
                "path":path,
                "selection":selection,
                "plan_digest":digest,
                "output":stale_destination.to_string_lossy().to_string(),
            }),
        ),
        req(2, "shutdown", json!({})),
    ]);
    let stale = &stale_responses[0]["result"];
    assert_eq!(stale["ok"], false, "{stale:?}");
    assert_eq!(stale["error"]["code"], "STALE_PLAN");
    assert!(!stale_destination.exists());
    let _ = std::fs::remove_dir_all(destination);
    let _ = std::fs::remove_dir_all(stale_destination);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn localization_rpc_exports_and_imports_typed_translation_data() {
    let root = temp_workspace(
        "localization-roundtrip",
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.localization.v1"]}"#,
        "let traveler = \"Ari\"\nevent greeting\n  Hello {traveler} [[event:secret|Harbor]] #wl-localization:greeting\n  -> END\nevent secret\n  -> END\n",
    );
    let path = root.to_string_lossy().into_owned();
    let selection = json!({
        "schema_version": 1,
        "source_locale": "en",
        "target_locale": "zh-Hant",
        "string_ids": ["greeting"]
    });
    let package = root.parent().unwrap().join(format!(
        "{}-translation.json",
        root.file_name().unwrap().to_string_lossy()
    ));
    let (_, export_responses) = exchange(&[
        req(1, "project.open", json!({"path":path.clone()})),
        req(
            2,
            "localization.export.preview",
            json!({"project_id":"p1", "selection":selection.clone()}),
        ),
        req(3, "shutdown", json!({})),
    ]);
    let preview = &export_responses[1]["result"];
    assert_eq!(preview["ok"], true, "{preview:?}");
    assert_eq!(preview["plan"]["can_export"], true);
    let export_digest = preview["plan"]["plan_digest"].as_str().unwrap().to_string();
    let source_exchange = preview["plan"]["exchange"].clone();

    let (_, export_apply_responses) = exchange(&[
        req(1, "project.open", json!({"path":path.clone()})),
        req(
            2,
            "localization.export.apply",
            json!({
                "project_id":"p1",
                "selection":selection.clone(),
                "plan_digest":export_digest,
                "output":package.to_string_lossy().to_string(),
            }),
        ),
        req(3, "shutdown", json!({})),
    ]);
    assert_eq!(export_apply_responses[1]["result"]["ok"], true);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&package).unwrap()).unwrap(),
        source_exchange
    );

    let mut exchange_value = source_exchange;
    let package_text = exchange_value.to_string();
    assert!(!package_text.contains("traveler"));
    assert!(!package_text.contains("secret"));
    let translation_parts = exchange_value["entries"][0]["source_parts"]
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
    exchange_value["entries"][0]["translation_parts"] = json!(translation_parts);
    std::fs::write(&package, serde_json::to_vec(&exchange_value).unwrap()).unwrap();

    let (_, import_preview_responses) = exchange(&[
        req(1, "project.open", json!({"path":path.clone()})),
        req(
            2,
            "localization.import.preview",
            json!({
                "project_id":"p1",
                "selection":selection.clone(),
                "exchange":exchange_value.clone(),
            }),
        ),
        req(3, "shutdown", json!({})),
    ]);
    let import_preview = &import_preview_responses[1]["result"];
    assert_eq!(import_preview["ok"], true, "{import_preview:?}");
    assert_eq!(import_preview["plan"]["can_apply"], true);
    let import_digest = import_preview["plan"]["plan_digest"].as_str().unwrap();
    let (_, import_responses) = exchange(&[
        req(1, "project.open", json!({"path":path})),
        req(
            2,
            "localization.import.apply",
            json!({
                "project_id":"p1",
                "selection":selection,
                "exchange":exchange_value,
                "plan_digest":import_digest,
            }),
        ),
        req(3, "shutdown", json!({})),
    ]);
    let applied = &import_responses[1]["result"];
    assert_eq!(applied["ok"], true, "{applied:?}");
    assert_eq!(applied["changed_files"].as_array().unwrap().len(), 2);
    let sidecar: Value = serde_json::from_slice(
        &std::fs::read(root.join(".world/localization/zh-Hant.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        sidecar["entries"]["greeting"]["translation_parts"][0]["text"],
        "歡迎，"
    );
    let _ = std::fs::remove_file(package);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn localization_rpc_rejects_duplicate_translation_parts_keys() {
    let root = temp_workspace(
        "localization-duplicate-json-key",
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.localization.v1"]}"#,
        "let traveler = \"Ari\"\nevent greeting\n  Hello {traveler} #wl-localization:greeting\n  -> END\n",
    );
    let path = root.to_string_lossy().to_string();
    let selection = json!({
        "schema_version": 1,
        "source_locale": "en",
        "target_locale": "zh-Hant",
        "string_ids": ["greeting"]
    });
    let (_, preview_responses) = exchange(&[
        req(1, "project.open", json!({"path":path.clone()})),
        req(
            2,
            "localization.export.preview",
            json!({"project_id":"p1", "selection":selection.clone()}),
        ),
        req(3, "shutdown", json!({})),
    ]);
    let exported = preview_responses[1]["result"]["plan"]["exchange"].to_string();
    let duplicate_exchange = exported.replace(
        "\"translation_parts\":null",
        "\"translation_parts\":[{\"type\":\"text\",\"text\":\"first\"},{\"type\":\"placeholder\",\"token\":\"p0\"}],\"translation_parts\":[{\"type\":\"text\",\"text\":\"second\"},{\"type\":\"placeholder\",\"token\":\"p0\"}]",
    );
    assert_ne!(duplicate_exchange, exported);
    let raw_request = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"localization.import.preview\",\"params\":{{\"path\":{},\"selection\":{},\"exchange\":{duplicate_exchange}}}}}",
        serde_json::to_string(&path).unwrap(),
        serde_json::to_string(&selection).unwrap(),
    );
    let (code, responses) = exchange_raw(&raw_request);
    assert_eq!(code, 0);
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["error"]["code"], -32700);
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn catalog_query_rpc_keeps_read_only_and_error_boundaries() {
    let root = temp_workspace(
        "catalog-query-read-only",
        r#"{"schema_version":1,"language_version":"1.10","required_features":["future.catalog.v2"]}"#,
        "entity harbor kind place as \"港口\"\nevent start\n  -> END\n",
    );
    let path = root.to_string_lossy().to_string();
    let (_, responses) = exchange(&[
        req(
            1,
            "catalog.query",
            json!({"path":path.clone(), "query":{"schema_version":1,"filters":[]}}),
        ),
        req(
            2,
            "catalog.query",
            json!({"path":path.clone(), "query":{"filters":[]}}),
        ),
        req(
            3,
            "catalog.query",
            json!({
                "path":path,
                "query":{"schema_version":1,"filters":[{"dimension":"kind","values":["future-kind"]}]}
            }),
        ),
        req(
            4,
            "catalog.query",
            json!({
                "path":path,
                "query":{"schema_version":1,"filters":[]},
                "max_candidates":1
            }),
        ),
        req(5, "shutdown", json!({})),
    ]);
    let readonly = &responses[0]["result"];
    assert_eq!(readonly["ok"], true, "{readonly:?}");
    assert_eq!(readonly["read_only"], true);
    assert!(!readonly["workspace_diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(responses[1]["error"]["code"], -32602);
    assert_eq!(responses[2]["result"]["ok"], false);
    assert_eq!(responses[2]["result"]["error"]["code"], "INVALID_QUERY");
    assert_eq!(responses[3]["result"]["ok"], false);
    assert_eq!(
        responses[3]["result"]["error"]["code"],
        "CANDIDATE_BUDGET_EXCEEDED"
    );
}

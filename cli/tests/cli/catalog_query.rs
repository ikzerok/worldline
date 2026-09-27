use super::common::*;
use serde_json::json;

#[test]
fn catalog_query_cli_uses_core_query_dto_and_snapshot_cursor() {
    let root = temp_entity_project(
        "catalog-query-pages",
        "entity harbor kind place as \"港口\"\nentity lighthouse kind place as \"灯塔\"\nevent start\n  -> END\n",
    );
    let path = root.to_string_lossy().into_owned();
    let query = json!({
        "schema_version": 1,
        "filters": [{"dimension": "kind", "values": ["entity"]}]
    })
    .to_string();
    let mut args = vec![
        "catalog-query".to_string(),
        path.clone(),
        "--query".into(),
        query.clone(),
        "--page-size=1".into(),
        "--json".into(),
    ];
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::<u8>::new())).unwrap();
    assert_eq!(code, 0);
    let first = json_lines(&out).remove(0);
    assert_eq!(first["ok"], true);
    assert_eq!(first["query"]["total"], 2);
    assert_eq!(first["query"]["items"].as_array().unwrap().len(), 1);
    let cursor = first["query"]["next"].clone();
    assert!(cursor.is_object());

    args = vec![
        "catalog-query".to_string(),
        path.clone(),
        "--query".into(),
        query.clone(),
        "--cursor".into(),
        cursor.to_string(),
        "--json".into(),
    ];
    out.clear();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::<u8>::new())).unwrap();
    assert_eq!(code, 0);
    let second = json_lines(&out).remove(0);
    assert_eq!(second["query"]["offset"], 1);
    assert_eq!(second["query"]["items"].as_array().unwrap().len(), 1);
    assert_ne!(
        first["query"]["items"][0]["target"]["id"],
        second["query"]["items"][0]["target"]["id"]
    );

    std::fs::write(
        root.join("world.wl"),
        "entity harbor kind place as \"港口\"\nentity lighthouse kind place as \"灯塔\"\nentity island kind place as \"岛屿\"\nevent start\n  -> END\n",
    )
    .unwrap();
    args = vec![
        "catalog-query".to_string(),
        path,
        "--query".into(),
        query,
        "--cursor".into(),
        cursor.to_string(),
        "--json".into(),
    ];
    out.clear();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::<u8>::new())).unwrap();
    assert_eq!(code, 2);
    let stale = json_lines(&out).remove(0);
    assert_eq!(stale["ok"], false);
    assert_eq!(stale["error"]["code"], "STALE_CURSOR");
    assert!(stale["query"].is_null());
}

#[test]
fn catalog_query_cli_reports_core_validation_and_candidate_budget_errors() {
    let root = temp_entity_project(
        "catalog-query-errors",
        "entity harbor kind place as \"港口\"\nevent start\n  -> END\n",
    );
    let path = root.to_string_lossy().into_owned();
    let invalid_query = json!({
        "schema_version": 1,
        "filters": [{"dimension": "kind", "values": ["unknown-kind"]}]
    })
    .to_string();
    let mut args = vec![
        "catalog-query".to_string(),
        path.clone(),
        "--query".into(),
        invalid_query,
        "--json".into(),
    ];
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::<u8>::new())).unwrap();
    assert_eq!(code, 2);
    let invalid = json_lines(&out).remove(0);
    assert_eq!(invalid["error"]["code"], "INVALID_QUERY");

    args = vec![
        "catalog-query".to_string(),
        path,
        "--query".into(),
        json!({"schema_version": 1, "filters": []}).to_string(),
        "--max-candidates=1".into(),
        "--json".into(),
    ];
    out.clear();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::<u8>::new())).unwrap();
    assert_eq!(code, 2);
    let budget = json_lines(&out).remove(0);
    assert_eq!(budget["error"]["code"], "CANDIDATE_BUDGET_EXCEEDED");
}

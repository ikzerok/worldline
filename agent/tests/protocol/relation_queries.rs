use super::*;
#[test]
fn relation_query_uses_catalog_index_and_returns_truncation_fields() {
    let (_, responses) = exchange(&[
        req(
            1,
            "compile",
            json!({ "source": RELATION_STORY, "language_version": "1.10" }),
        ),
        req(
            2,
            "relation.query",
            json!({
                "story_id": "s1",
                "target": {"kind":"entity","id":"keepers"},
                "depth": 1,
                "direction": "both"
            }),
        ),
        req(3, "shutdown", json!({})),
    ]);
    assert_eq!(responses[0]["result"]["ok"], true, "{responses:?}");
    let result = &responses[1]["result"];
    assert_eq!(result["ok"], true, "{responses:?}");
    assert_eq!(result["schema_version"], 1);
    assert_eq!(result["target"], json!({"kind":"entity","id":"keepers"}));
    assert_eq!(result["edges"][0]["id"], "rel_keepers_lighthouse");
    assert_eq!(result["truncated"], false);
    assert!(result["workspace_revision"].is_null());
}

#[test]
fn relation_query_filters_author_scopes_and_expands_period_children_explicitly() {
    let source = r#"
period old as "旧纪元"
period late as "旧纪元末" within old
entity version_a kind version as "版本A"
entity a kind place
entity b kind place
relation_type links as "连接"
relation_def old_a type links from entity a to entity b
  scope period old
  scope entity version_a
relation_def late_a type links from entity a to entity b
  scope period late
  scope entity version_a
relation_def global type links from entity a to entity b
event start
  -> END
"#;
    let (_, responses) = exchange(&[
        req(
            1,
            "compile",
            json!({ "source": source, "language_version": "1.10" }),
        ),
        req(
            2,
            "relation.query",
            json!({
                "story_id": "s1",
                "target": "entity:a",
                "scope_refs": ["period:old", "entity:version_a"]
            }),
        ),
        req(
            3,
            "relation.query",
            json!({
                "story_id": "s1",
                "target": "entity:a",
                "scope_refs": ["period:old", "entity:version_a"],
                "include_period_children": true
            }),
        ),
        req(4, "shutdown", json!({})),
    ]);
    assert_eq!(responses[0]["result"]["ok"], true, "{responses:?}");
    assert_eq!(
        responses[1]["result"]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| edge["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["old_a"]
    );
    assert_eq!(
        responses[2]["result"]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| edge["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["late_a", "old_a"]
    );
}

#[test]
fn relation_query_rejects_unknown_target_as_jsonrpc_parameter_error() {
    let (_, responses) = exchange(&[
        req(
            1,
            "compile",
            json!({ "source": RELATION_STORY, "language_version": "1.10" }),
        ),
        req(
            2,
            "relation.query",
            json!({
                "story_id": "s1",
                "target": "entity:missing"
            }),
        ),
        req(3, "shutdown", json!({})),
    ]);
    assert_eq!(responses[1]["error"]["code"], -32602, "{responses:?}");
}

#[test]
fn relation_query_accepts_nonnegative_offset_and_rejects_negative_offset() {
    let (_, responses) = exchange(&[
        req(
            1,
            "compile",
            json!({ "source": RELATION_STORY, "language_version": "1.10" }),
        ),
        req(
            2,
            "relation.query",
            json!({
                "story_id": "s1",
                "target": "entity:keepers",
                "offset": 1
            }),
        ),
        req(
            3,
            "relation.query",
            json!({
                "story_id": "s1",
                "target": "entity:keepers",
                "offset": -1
            }),
        ),
        req(4, "shutdown", json!({})),
    ]);
    assert_eq!(responses[1]["result"]["ok"], true, "{responses:?}");
    assert!(responses[1]["result"]["edges"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(responses[2]["error"]["code"], -32602, "{responses:?}");
}

#[test]
fn relation_rpc_accepts_windows_file_targets_for_query_and_edit() {
    let root = temp_relation_project(
        "file-target",
        "entity a kind place\nrelation_type records as \"记载\"\nrelation_def record type records from entity a to file \"chapters/record one.wl\"\n  source_note \"来源\"\n  scope file \"chapters/record one.wl\"\nevent start\n  -> END\n",
    );
    let target_file = root.join("chapters/record one.wl");
    std::fs::create_dir_all(target_file.parent().unwrap()).unwrap();
    std::fs::write(&target_file, "tag notes\n").unwrap();
    let file_id = std::fs::canonicalize(&target_file)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let file_id = file_id.strip_prefix(r"\\?\").unwrap_or(&file_id).to_owned();
    let target_text = format!("file:{file_id}");
    let target_object = json!({"kind": "file", "id": file_id});
    let path = root.to_string_lossy().to_string();
    let (_, responses) = exchange(&[
        req(1, "project.open", json!({"path": path})),
        req(
            2,
            "relation.query",
            json!({"project_id":"p1", "target":target_text}),
        ),
        req(
            3,
            "relation.update",
            json!({
                "project_id":"p1",
                "relation": {
                    "id":"record",
                    "to":target_object,
                    "source_note":null,
                    "scope_refs":[],
                    "properties":{}
                }
            }),
        ),
        req(
            4,
            "relation.query",
            json!({"project_id":"p1", "target":target_object}),
        ),
        req(5, "shutdown", json!({})),
    ]);
    assert_eq!(responses[1]["result"]["ok"], true, "{responses:?}");
    assert_eq!(responses[1]["result"]["target"]["id"], file_id);
    assert_eq!(responses[2]["result"]["ok"], true, "{responses:?}");
    assert_eq!(responses[2]["result"]["relation"]["to_ref"]["id"], file_id);
    assert_eq!(
        responses[2]["result"]["relation"]["source_note"],
        Value::Null
    );
    assert_eq!(responses[2]["result"]["relation"]["scope_refs"], json!([]));
    assert_eq!(responses[2]["result"]["relation"]["properties"], json!({}));
    assert_eq!(responses[3]["result"]["ok"], true, "{responses:?}");
}
#[test]
fn relation_project_rpc_returns_mapped_relations_and_explicit_history() {
    let source = r#"
character lin as "林舟"
character mei as "梅"
period era as "旧纪元"
event arrival with lin during era
  到达。
  -> END
event lost with lin
  无日期记录。
  -> END
relation_type family_link as "亲属"
relation_def lin_mei type family_link from character lin to character mei
"#;
    let (_, responses) = exchange(&[
        req(
            1,
            "compile",
            json!({ "source": source, "language_version": "1.10" }),
        ),
        req(
            2,
            "relation.project",
            json!({
                "story_id": "s1",
                "target": "character:lin",
                "role_mapping": {"family_link":"生亲"}
            }),
        ),
        req(
            3,
            "relation.project",
            json!({
                "story_id": "s1",
                "target": "character:lin",
                "role_mapping": {"unknown_type":"猜测"}
            }),
        ),
        req(4, "shutdown", json!({})),
    ]);
    assert_eq!(responses[0]["result"]["ok"], true, "{responses:?}");
    let result = &responses[1]["result"];
    assert_eq!(result["ok"], true, "{responses:?}");
    assert_eq!(result["relations"]["edges"][0]["id"], "lin_mei");
    assert_eq!(result["relations"]["edges"][0]["role"], "生亲");
    assert_eq!(result["relations"]["cycle_hint"], false);
    assert_eq!(
        result["history"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["event"]["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["arrival", "lost"]
    );
    assert_eq!(result["history"]["events"][1]["time_status"], "unknown");
    assert!(result["workspace_revision"].is_null());
    assert_eq!(responses[2]["error"]["code"], -32602);
}

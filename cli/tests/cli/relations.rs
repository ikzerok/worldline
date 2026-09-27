use super::common::*;
use serde_json::{json, Value};

#[test]
fn relations_json_uses_core_query_and_preserves_edge_identity() {
    let root = temp_presentation_project("relations-query");
    std::fs::write(
        root.join("world.wl"),
        "entity lighthouse kind place as \"灯塔\"\nentity keepers kind organization as \"守灯会\"\nrelation_type maintains as \"维护\"\n  inverse \"由其维护\"\n  direction directed\nrelation_def rel_keepers_lighthouse type maintains from entity keepers to entity lighthouse\n  description \"守灯会维护灯塔\"\nevent start\n  -> END\n",
    )
    .unwrap();
    let (code, out) = run_args(&[
        "relations",
        root.to_string_lossy().as_ref(),
        "--target",
        "entity:keepers",
        "--depth",
        "1",
        "--json",
    ]);
    assert_eq!(code.unwrap(), 0);
    let value = json_lines(&out).remove(0);
    assert_eq!(value["ok"], true);
    assert_eq!(value["schema_version"], 1);
    assert_eq!(
        value["target"],
        serde_json::json!({"kind":"entity","id":"keepers"})
    );
    assert_eq!(value["depth"], 1);
    assert_eq!(value["edges"][0]["id"], "rel_keepers_lighthouse");
    assert_eq!(
        value["edges"][0]["from_ref"],
        serde_json::json!({"kind":"entity","id":"keepers"})
    );
    assert_eq!(
        value["edges"][0]["to_ref"],
        serde_json::json!({"kind":"entity","id":"lighthouse"})
    );
    assert_eq!(value["truncated"], false);
    assert!(value["workspace_revision"].as_str().is_some());

    let (code, out) = run_args(&[
        "relations",
        root.to_string_lossy().as_ref(),
        "--target",
        "entity:keepers",
        "--offset",
        "1",
        "--depth",
        "1",
        "--json",
    ]);
    assert_eq!(code.unwrap(), 0);
    let continued = json_lines(&out).remove(0);
    assert_eq!(continued["ok"], true);
    assert!(continued["edges"].as_array().unwrap().is_empty());
}

#[test]
fn relations_project_json_preserves_explicit_roles_and_place_history() {
    let root = temp_presentation_project("relations-project");
    std::fs::write(
        root.join("world.wl"),
        "character lin as \"林舟\"\nentity harbor kind place as \"雾港\"\nperiod era as \"旧纪元\"\nevent arrival with lin during era\n  到达。\n  -> END\nevent lost with lin\n  无日期记录。\n  -> END\nevent mention\n  [[entity:harbor|港口提及]]\n  -> END\nrelation_type happens_at as \"发生地点\"\nrelation_def arrival_at type happens_at from event arrival to entity harbor\n  scope period era\nrelation_def lost_at type happens_at from event lost to entity harbor\n",
    )
    .unwrap();
    let original = std::fs::read(root.join("world.wl")).unwrap();
    let path = root.to_string_lossy().to_string();
    let (code, out) = run_args(&[
        "relations",
        "project",
        &path,
        "--target",
        "entity:harbor",
        "--role-mapping-json",
        r#"{"happens_at":"记录地点"}"#,
        "--depth",
        "2",
        "--scope",
        "period:era",
        "--include-unscoped",
        "--json",
    ]);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    let place = json_lines(&out).remove(0);
    assert_eq!(place["ok"], true);
    assert_eq!(place["relations"]["edges"][0]["role"], "记录地点");
    assert_eq!(place["relations"]["edges"][0]["id"], "arrival_at");
    assert_eq!(place["relations"]["cycle_hint"], false);
    assert_eq!(
        place["relations"]["edges"][0]["scope_refs"],
        serde_json::json!([{"kind":"period","id":"era"}])
    );
    assert_eq!(
        place["history"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["source"]["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["arrival_at", "lost_at"]
    );
    assert_eq!(place["history"]["events"][1]["time_status"], "unknown");
    assert_eq!(place["truncated"], false);
    assert!(place["workspace_revision"].as_str().is_some());

    let (code, out) = run_args(&[
        "relations",
        "project",
        &path,
        "--target",
        "character:lin",
        "--json",
    ]);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    let character = json_lines(&out).remove(0);
    assert!(character["relations"]["edges"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        character["history"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["event"]["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["arrival", "lost"]
    );
    let (code, out) = run_args(&[
        "relations",
        "project",
        &path,
        "--target",
        "entity:harbor",
        "--role-mapping-json",
        r#"{"missing":"关系"}"#,
        "--json",
    ]);
    assert_eq!(code.unwrap(), 2);
    assert_eq!(
        json_lines(&out).remove(0)["error"]["code"],
        "UNKNOWN_RELATION_TYPE"
    );
    assert_eq!(std::fs::read(root.join("world.wl")).unwrap(), original);
}

#[test]
fn relations_scope_flags_filter_author_scopes_without_inference() {
    let root = temp_presentation_project("relations-scope");
    std::fs::write(
        root.join("world.wl"),
        "period old as \"旧纪元\"\nperiod late as \"旧纪元末\" within old\nentity version_a kind version as \"版本A\"\nentity a kind place\nentity b kind place\nrelation_type links as \"连接\"\nrelation_def old_a type links from entity a to entity b\n  scope period old\n  scope entity version_a\nrelation_def late_a type links from entity a to entity b\n  scope period late\n  scope entity version_a\nrelation_def global type links from entity a to entity b\nevent start\n  -> END\n",
    )
    .unwrap();
    let path = root.to_string_lossy().to_string();
    let base = [
        "relations",
        path.as_str(),
        "--target",
        "entity:a",
        "--scope",
        "period:old",
        "--scope",
        "entity:version_a",
        "--json",
    ];
    let (code, out) = run_args(&base);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    let value = json_lines(&out).remove(0);
    assert_eq!(
        value["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| edge["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["old_a"]
    );

    let mut expanded = base.to_vec();
    expanded.insert(expanded.len() - 1, "--include-period-children");
    let (code, out) = run_args(&expanded);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    let value = json_lines(&out).remove(0);
    assert_eq!(
        value["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| edge["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["late_a", "old_a"]
    );
}

#[test]
fn relations_rejects_unknown_target_as_usage_failure() {
    let root = temp_presentation_project("relations-unknown");
    let (code, out) = run_args(&[
        "relations",
        root.to_string_lossy().as_ref(),
        "--target",
        "entity:missing",
        "--json",
    ]);
    assert_eq!(code.unwrap(), 2);
    let value = json_lines(&out).remove(0);
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "UNKNOWN_TARGET");
}

#[test]
fn relation_cli_crud_uses_core_edit_and_baseline_fields() {
    let root = temp_relation_project(
        "crud",
        "entity a kind place as \"甲\"\nentity b kind place as \"乙\"\nevent start\n  -> END\n",
    );
    let path = root.to_string_lossy().to_string();
    let (code, out) = run_args(&[
        "relation-type",
        "create",
        &path,
        "--id",
        "knows",
        "--display",
        "认识",
        "--inverse-display",
        "被认识",
        "--direction",
        "directed",
        "--from-kind",
        "entity",
        "--to-kind",
        "entity",
        "--json",
    ]);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    let type_result = json_lines(&out).remove(0);
    assert_eq!(type_result["ok"], true);
    assert!(type_result["baseline"].is_string());
    let (code, out) = run_args(&[
        "relation",
        "create",
        &path,
        "--id",
        "stale_relation",
        "--type",
        "knows",
        "--from",
        "entity:a",
        "--to",
        "entity:b",
        "--baseline",
        "stale",
        "--json",
    ]);
    assert_eq!(code.unwrap(), 1, "{out:?}");
    assert_eq!(json_lines(&out)[0]["error"]["code"], "STALE_BASELINE");
    let (code, out) = run_args(&[
        "relation",
        "create",
        &path,
        "--id",
        "a_knows_b",
        "--type",
        "knows",
        "--from",
        "entity:a",
        "--to",
        "entity:b",
        "--description",
        "甲认识乙",
        "--json",
    ]);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    let relation_result = json_lines(&out).remove(0);
    assert_eq!(relation_result["ok"], true);
    assert_eq!(relation_result["relation"]["id"], "a_knows_b");
    assert!(relation_result["catalog"]["relation_index"].is_array());
    let (code, out) = run_args(&[
        "relation",
        "update",
        &path,
        "--id",
        "a_knows_b",
        "--description",
        "甲已经认识乙",
        "--json",
    ]);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    let relation_result = json_lines(&out).remove(0);
    assert_eq!(relation_result["relation"]["description"], "甲已经认识乙");
    let (code, out) = run_args(&["relation", "delete", &path, "--id", "a_knows_b", "--json"]);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    assert_eq!(json_lines(&out)[0]["relation"], serde_json::Value::Null);
}

#[test]
fn relation_cli_accepts_windows_file_targets_and_explicitly_clears_optional_fields() {
    let root = temp_relation_project(
        "file-target-and-clear",
        "entity a kind place as \"甲\"\nentity b kind place as \"乙\"\nrelation_type records as \"记载\"\n  inverse \"被记载\"\n  from entity\n  to file\nrelation_def record type records from entity a to file \"chapters/record one.wl\"\n  source_note \"来源\"\n  scope file \"chapters/record one.wl\"\n  property active = true\nevent start\n  -> END\n",
    );
    let target_file = root.join("chapters/record one.wl");
    std::fs::create_dir_all(target_file.parent().unwrap()).unwrap();
    std::fs::write(&target_file, "tag notes\n").unwrap();
    let file_id = std::fs::canonicalize(&target_file)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let file_id = file_id.strip_prefix(r"\\?\").unwrap_or(&file_id).to_owned();
    let path = root.to_string_lossy().to_string();
    let target = format!("file:{file_id}");

    let (code, out) = run_args(&["relations", &path, "--target", &target, "--json"]);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    let query = json_lines(&out).remove(0);
    assert_eq!(query["ok"], true, "{query}");
    assert!(query["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edge| { edge["to_ref"]["id"] == file_id || edge["from_ref"]["id"] == file_id }));

    let (code, out) = run_args(&[
        "relation-type",
        "update",
        &path,
        "--id",
        "records",
        "--clear-inverse-display",
        "--clear-from-kind",
        "--clear-to-kind",
        "--json",
    ]);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    let cleared_type = json_lines(&out).remove(0);
    assert_eq!(
        cleared_type["relation_type"]["inverse_display"],
        Value::Null
    );
    assert_eq!(cleared_type["relation_type"]["from_kind"], Value::Null);
    assert_eq!(cleared_type["relation_type"]["to_kind"], Value::Null);

    let (code, out) = run_args(&[
        "relation",
        "update",
        &path,
        "--id",
        "record",
        "--to",
        &target,
        "--clear-source-note",
        "--clear-scope",
        "--clear-properties",
        "--json",
    ]);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    let cleared_relation = json_lines(&out).remove(0);
    assert_eq!(cleared_relation["relation"]["source_note"], Value::Null);
    assert_eq!(cleared_relation["relation"]["scope_refs"], json!([]));
    assert_eq!(cleared_relation["relation"]["properties"], json!({}));

    let (code, out) = run_args(&[
        "relation",
        "create",
        &path,
        "--id",
        "bad_clear",
        "--type",
        "records",
        "--from",
        "entity:a",
        "--to",
        "entity:b",
        "--clear-source-note",
    ]);
    assert!(
        code.is_err(),
        "create must reject --clear-source-note: {out:?}"
    );
}

#[test]
fn relation_cli_promotion_preview_then_commit_removes_legacy_line() {
    let root = temp_relation_project(
        "promotion",
        "character a\n  relation b as \"旧关系\"\ncharacter b\nrelation_type knows as \"认识\"\nevent start\n  -> END\n",
    );
    let path = root.to_string_lossy().to_string();
    let common = [
        "--source",
        "character:a",
        "--target",
        "character:b",
        "--label",
        "旧关系",
        "--id",
        "promoted",
        "--type",
        "knows",
        "--source-note",
        "由旧人物关系提升",
        "--scope",
        "character:b",
        "--property",
        "active=true",
        "--property",
        "weight=3",
        "--json",
    ];
    let mut preview_args = vec!["relations", "promote", "preview", &path];
    preview_args.extend(common);
    let (code, out) = run_args(&preview_args);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    let preview = json_lines(&out).remove(0);
    assert_eq!(preview["operation"], "preview");
    assert_eq!(preview["preview"]["fingerprint_changed"], true);
    assert_eq!(
        preview["preview"]["draft"]["scope_refs"],
        json!([{"kind":"character","id":"b"}])
    );
    assert_eq!(
        preview["preview"]["draft"]["properties"],
        json!([["active", true], ["weight", 3.0]])
    );
    assert!(std::fs::read_to_string(root.join("world.wl"))
        .unwrap()
        .contains("relation b as"));
    let mut commit_args = vec!["relations", "promote", "commit", &path];
    commit_args.extend(common);
    let (code, out) = run_args(&commit_args);
    assert_eq!(code.unwrap(), 0, "{out:?}");
    assert_eq!(json_lines(&out)[0]["operation"], "commit");
    let source = std::fs::read_to_string(root.join("world.wl")).unwrap();
    assert!(source.contains("relation_def promoted"), "{source}");
    assert!(!source.contains("relation b as"), "{source}");
    assert!(source.contains("scope character b"), "{source}");
    assert!(source.contains("property active = true"), "{source}");
    assert!(source.contains("property weight = 3"), "{source}");
}

//! 指纹安全边界、无效引用零修改及既有计划计数兼容。
use worldline_core::catalog::TargetRef;
use worldline_core::project::Project;

#[test]
fn unsupported_relation_state_owner_is_rejected_without_language_expansion() {
    let root = std::env::temp_dir().join(format!("wl-relation-state-{}", std::process::id()));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.documents.retain(|path, _| path == &entry);
    let source = r#"entity a kind item
entity b kind item
relation_type related as "关联"
relation_def edge type related from entity a to entity b
alias relation edge as "旧别名"
state relation_status on relation "edge" with [] as "关联状态"
event start
  -> END
"#;
    project.set_text(&entry, source.into()).unwrap();
    project
        .create_authoring_document(
            &root.join(".world/project.json"),
            br#"{
        "schema_version":1,"language_version":"1.10",
        "required_features":["content.entities.v1","content.relations.v1"]
    }"#
            .to_vec(),
        )
        .unwrap();
    let compiled = project.compile();
    assert!(compiled.has_errors());
    assert!(compiled
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "A216"));
    let baseline = project.content_baseline();
    assert!(project
        .plan_rename_target(&TargetRef::new("relation", "edge"), "renamed")
        .is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.document(&entry).unwrap(), source);
}

#[test]
fn legacy_syntax_reencoding_keeps_identity_reference_counts() {
    let root = std::env::temp_dir().join(format!("wl-legacy-count-{}", std::process::id()));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.documents.retain(|path, _| path == &entry);
    project
        .set_text(
            &entry,
            r#"character a as "甲"
event start with a
  choice "前缀\# [[character:a|甲]] [[character:a|乙]]"
    -> END
"#
            .into(),
        )
        .unwrap();
    project
        .create_authoring_document(
            &root.join(".world/project.json"),
            br#"{
        "schema_version":1,"language_version":"1.11","required_features":[]
    }"#
            .to_vec(),
        )
        .unwrap();
    let plan = project
        .plan_rename_target(&TargetRef::new("character", "a"), "alpha")
        .unwrap();
    assert_eq!(plan.explicit_references, 4);
    assert_eq!(plan.changes[0].reference_count, 4);
    assert_eq!(plan.changes[0].occurrences.len(), 3);
    let syntax = plan.changes[0]
        .occurrences
        .iter()
        .find(|item| item.field.as_deref() == Some("source.syntax"))
        .unwrap();
    assert_eq!(syntax.line, 3);
    assert!(syntax.before_token.contains(r#"前缀\#"#));
    assert_eq!(syntax.after_token.matches("[[character:alpha|").count(), 2);
    assert!(!syntax.before_token.ends_with('\n'));
    project.apply_rename_plan(&plan).unwrap();
    assert!(project
        .document(&entry)
        .unwrap()
        .contains("前缀# [[character:alpha|甲]] [[character:alpha|乙]]"));
}

#[test]
fn scope_only_reference_uses_actual_token_line_not_catalog_owner_line() {
    let root = std::env::temp_dir().join(format!("wl-scope-span-{}", std::process::id()));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.documents.retain(|path, _| path == &entry);
    project.set_text(&entry, "entity ledger kind item\nentity other kind item\nrelation_type related\nevent start\n  -> END\n".into()).unwrap();
    project
        .create_authoring_document(
            &root.join(".world/project.json"),
            br#"{
        "schema_version":1,"language_version":"1.10",
        "required_features":["content.entities.v1","content.relations.v1"]
    }"#
            .to_vec(),
        )
        .unwrap();
    let path = project
        .add_file(std::path::Path::new("relations.wl"))
        .unwrap();
    project.set_text(&path, "relation_def edge type related from entity other to entity other\n  scope_ref entity ledger // entity ledger\n".into()).unwrap();
    let plan = project
        .plan_rename_target(&TargetRef::new("entity", "ledger"), "renamed")
        .unwrap();
    let change = plan
        .changes
        .iter()
        .find(|change| change.path == path)
        .unwrap();
    assert_eq!(change.reference_count, 1);
    assert_eq!(change.occurrences[0].line, 2);
    assert_eq!(change.occurrences[0].field.as_deref(), Some("scope_ref.id"));
    project.apply_rename_plan(&plan).unwrap();
    assert!(project
        .document(&path)
        .unwrap()
        .contains("scope_ref entity renamed // entity ledger"));
}

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{ast::PropertyValue, project::Project, schemas::*, TargetRef};

#[path = "schema_authoring/guards.rs"]
mod guards;
#[path = "schema_authoring/protocol.rs"]
mod protocol;

const SCHEMAS: &str = r#"schema city for entity entity_type place closed
  field people_id population number required
  field flag_id public boolean required
  field note_id note text required
  field category_id category enum "city" "town"
  field owner_id owner ref entity entity_type organization
  field person_id person ref character
  field link_id link ref relation
schema free_city for entity entity_type place
  field people_id population number
schema world_data for world
  field note_id note text required
schema character_data for character
  field note_id note text required
schema relation_data for relation
  field note_id note text required
"#;
const FACTS: &str = r#"world globe as "世界"
character lead as "领袖"
entity harbor kind place as "港城"
  property population = 0 // 数值原注释
  property public = false
  property note = ""
  property category = "city"
entity council kind organization as "议会"
entity remote kind place as "远方"
relation_type knows as "知道"
  from entity
  to entity
relation_def bond type knows from entity harbor to entity council
event start
  你好。
  -> END
"#;
struct Fixture {
    root: PathBuf,
    project: Project,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn fixture(name: &str, bound: bool) -> Fixture {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "schema-authoring-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join(".world/project.json"), br#"{"schema_version":1,"language_version":"1.13","required_features":["content.object_refs.v1","content.character_refs.v1","content.relations.v1"]}"#).unwrap();
    fs::write(root.join("world.wl"), FACTS).unwrap();
    fs::write(root.join("schema.wl"), SCHEMAS).unwrap();
    fs::write(
        root.join("binding.wl"),
        if bound {
            "bind entity harbor to city // 独立绑定\n"
        } else {
            "// 保留的独立源码\n"
        },
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    let compiled = project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    Fixture { root, project }
}
fn request(project: &Project) -> SchemaAuthoringRequest {
    SchemaAuthoringRequest {
        schema_version: 1,
        expected_baseline: project.content_baseline(),
        target: TargetRef::new("entity", "harbor"),
        binding: SchemaBindingMutation::Keep,
        fields: Vec::new(),
    }
}
fn set(field: &str, key: &str, value: PropertyValue) -> SchemaFieldMutation {
    SchemaFieldMutation::Set {
        field_id: field.into(),
        key: key.into(),
        value,
    }
}
fn apply(project: &mut Project, request: &SchemaAuthoringRequest) -> SchemaAuthoringPreview {
    let preview = project.preview_schema_authoring(request).unwrap();
    project
        .apply_schema_authoring(request, &preview.plan_digest)
        .unwrap()
}

#[test]
fn projection_has_real_presence_all_types_candidates_and_same_compile_diagnostics() {
    let mut f = fixture("presence", true);
    let compiled = f.project.compile();
    let view = f
        .project
        .schema_authoring_from_compile(&compiled, &TargetRef::new("entity", "harbor"))
        .unwrap();
    assert!(view.complete && view.writable);
    assert_eq!(view.fields.len(), 7);
    assert_eq!(view.fields[0].value, Some(PropertyValue::Num(0.0)));
    assert_eq!(view.fields[1].value, Some(PropertyValue::Bool(false)));
    assert_eq!(
        view.fields[2].value,
        Some(PropertyValue::Str(String::new()))
    );
    assert!(view.fields[..4]
        .iter()
        .all(|f| f.present && f.property_source.is_some()));
    assert!(view.fields[4..]
        .iter()
        .all(|f| !f.present && f.value.is_none()));
    assert_eq!(
        view.fields[4]
            .reference_candidates
            .iter()
            .map(|c| c.target.clone())
            .collect::<Vec<_>>(),
        [TargetRef::new("entity", "council")]
    );
    assert_eq!(
        view.fields[5].reference_candidates[0].target,
        TargetRef::new("character", "lead")
    );
    assert_eq!(
        view.fields[6].reference_candidates[0].target,
        TargetRef::new("relation", "bond")
    );
    assert!(view
        .fields
        .iter()
        .all(|f| f.constraint_source.file.ends_with("schema.wl")));
    let index = SchemaIndex::from_compile(&compiled);
    assert_eq!(
        serde_json::to_value(index).unwrap(),
        serde_json::to_value(f.project.schema_index()).unwrap()
    );
}

#[test]
fn bind_set_preview_apply_undo_redo_save_and_reopen_are_one_transaction() {
    let mut f = fixture("bind", false);
    let before = f.project.clone();
    let fingerprint = f.project.compile().analysis.fingerprint;
    let mut req = request(&f.project);
    req.binding = SchemaBindingMutation::Bind {
        schema_id: "city".into(),
    };
    req.fields = vec![
        set("people_id", "population", PropertyValue::Num(12.0)),
        set(
            "owner_id",
            "owner",
            PropertyValue::Ref(TargetRef::new("entity", "council")),
        ),
    ];
    let preview = f.project.preview_schema_authoring(&req).unwrap();
    assert_eq!(f.project.content_baseline(), req.expected_baseline);
    assert_eq!(fs::read_to_string(f.root.join("world.wl")).unwrap(), FACTS);
    assert!(!preview.has_errors);
    f.project
        .apply_schema_authoring(&req, &preview.plan_digest)
        .unwrap();
    assert_eq!(f.project.compile().analysis.fingerprint, fingerprint);
    let after = f.project.clone();
    assert!(f.project.restore(before));
    assert_eq!(f.project.content_baseline(), req.expected_baseline);
    assert!(f.project.restore(after));
    f.project.save().unwrap();
    let reopened = Project::open(&f.root).unwrap();
    let view = reopened.schema_authoring(&req.target).unwrap();
    assert_eq!(view.binding.unwrap().schema_id, "city");
    assert_eq!(view.fields[0].value, Some(PropertyValue::Num(12.0)));
    assert_eq!(
        view.fields[4].value,
        Some(PropertyValue::Ref(TargetRef::new("entity", "council")))
    );
}

#[test]
fn cross_file_change_preserves_unknown_and_unselected_exact_bytes() {
    let mut f = fixture("bytes", true);
    let path = f.root.join("world.wl");
    let source = FACTS.replace(
        "  property category = \"city\"",
        "  property category = \"city\"\n  property custom = \"未知\\n原文\" // 不能重排",
    );
    f.project.set_text(&path, source.clone()).unwrap();
    let mut req = request(&f.project);
    req.binding = SchemaBindingMutation::Change {
        schema_id: "free_city".into(),
    };
    req.fields = vec![set("people_id", "population", PropertyValue::Num(20.0))];
    let preview = apply(&mut f.project, &req);
    assert_eq!(
        preview.changed_files,
        [PathBuf::from("binding.wl"), PathBuf::from("world.wl")]
    );
    assert_eq!(
        f.project.document(&path).unwrap(),
        source.replace("population = 0", "population = 20")
    );
    assert_eq!(
        f.project.document(&f.root.join("binding.wl")).unwrap(),
        "bind entity harbor to free_city // 独立绑定\n"
    );
    assert_eq!(fs::read_to_string(path).unwrap(), FACTS);
}

#[test]
fn invalid_scalar_enum_subtype_and_missing_required_drafts_are_explicitly_applicable() {
    let mut f = fixture("invalid", true);
    let mut req = request(&f.project);
    req.fields = vec![
        set("people_id", "population", PropertyValue::Str("很多".into())),
        set(
            "category_id",
            "category",
            PropertyValue::Str("village".into()),
        ),
        set(
            "owner_id",
            "owner",
            PropertyValue::Ref(TargetRef::new("entity", "remote")),
        ),
        SchemaFieldMutation::Remove {
            field_id: "note_id".into(),
            key: "note".into(),
        },
    ];
    let result = apply(&mut f.project, &req);
    assert!(result.has_errors && result.after.complete && result.after.writable);
    for code in ["SCH004", "SCH005", "SCH006", "SCH007"] {
        assert!(result.diagnostics.iter().any(|d| d.code == code));
    }
    assert!(result.after.fields[0]
        .diagnostics
        .iter()
        .any(|d| d.code == "SCH005"));
    assert_eq!(
        result.after.fields[0].value,
        Some(PropertyValue::Str("很多".into()))
    );
    assert!(!result.after.fields[2].present);
    assert!(result.after.fields[2]
        .diagnostics
        .iter()
        .any(|d| d.code == "SCH004"));
}

#[test]
fn unbind_leaves_all_properties_and_all_four_kinds_can_bind() {
    let mut f = fixture("kinds", true);
    let mut req = request(&f.project);
    req.binding = SchemaBindingMutation::Unbind;
    apply(&mut f.project, &req);
    assert_eq!(f.project.document(&f.root.join("world.wl")).unwrap(), FACTS);
    assert!(f
        .project
        .schema_authoring(&req.target)
        .unwrap()
        .binding
        .is_none());
    for (kind, id, schema) in [
        ("world", "globe", "world_data"),
        ("character", "lead", "character_data"),
        ("relation", "bond", "relation_data"),
    ] {
        let mut req = request(&f.project);
        req.target = TargetRef::new(kind, id);
        req.binding = SchemaBindingMutation::Bind {
            schema_id: schema.into(),
        };
        req.fields = vec![set("note_id", "note", PropertyValue::Str("资料".into()))];
        let result = apply(&mut f.project, &req);
        assert!(!result.has_errors, "{:?}", result.diagnostics);
        assert_eq!(result.runtime_changed, kind != "relation");
        assert_eq!(
            result.before_fingerprint == result.after_fingerprint,
            !result.runtime_changed
        );
        assert_eq!(
            result.after.fields[0].value,
            Some(PropertyValue::Str("资料".into()))
        );
    }
}

#[test]
fn existing_four_space_indentation_crlf_and_inner_comments_survive() {
    let mut f = fixture("layout", true);
    let path = f.root.join("world.wl");
    let source = FACTS
        .replace("  property", "    property")
        .replace("population = 0", "population /* 内注释 */ = 0")
        .replace('\n', "\r\n");
    f.project.set_text(&path, source.clone()).unwrap();
    let mut req = request(&f.project);
    req.fields = vec![
        set("people_id", "population", PropertyValue::Num(1.0)),
        set(
            "owner_id",
            "owner",
            PropertyValue::Ref(TargetRef::new("entity", "council")),
        ),
    ];
    let result = apply(&mut f.project, &req);
    assert!(!result.has_errors);
    let updated = f.project.document(&path).unwrap();
    assert!(updated.contains("    property owner = ref(\"entity\", \"council\")\r\n"));
    assert!(updated.contains("/* 内注释 */"));
    assert!(updated.contains("    property public = false\r\n    property note = \"\"\r\n"));
}

#[test]
fn binding_without_scalar_changes_preserves_fingerprint_for_all_four_kinds() {
    let mut f = fixture("bind-fingerprint", false);
    for (kind, id, schema) in [
        ("entity", "harbor", "city"),
        ("world", "globe", "world_data"),
        ("character", "lead", "character_data"),
        ("relation", "bond", "relation_data"),
    ] {
        let mut req = request(&f.project);
        req.target = TargetRef::new(kind, id);
        req.binding = SchemaBindingMutation::Bind {
            schema_id: schema.into(),
        };
        let result = apply(&mut f.project, &req);
        assert!(!result.runtime_changed, "{kind}");
        assert_eq!(result.before_fingerprint, result.after_fingerprint);
    }
}

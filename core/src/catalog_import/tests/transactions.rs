use super::*;
#[test]
fn catalog_import_as_identity_and_type_are_not_display_keyword() {
    let source = "character as as \"旧\"\nentity e kind as as \"旧\"\nevent start\n  -> END\n";
    let mut f = fixture(source, "1.13");
    let request = req(
        &f.project,
        "kind,id,name\ncharacter,as,新\nentity,e,新\n",
        vec![mapping(2, CatalogImportField::Display)],
    );
    let plan = f.project.preview_catalog_import(&request).unwrap();
    assert!(plan.can_apply, "{:?}", plan.diagnostics);
    f.project
        .apply_catalog_import(&request, &plan.plan_digest)
        .unwrap();
    assert_eq!(
        f.project.document(&f.root.join("world.wl")).unwrap(),
        source.replace("旧", "新")
    );
}
#[test]
fn catalog_import_forward_cycles_and_character_ref_only_preserve_fingerprint() {
    let source="character lin as \"林\"\n  property z=4\n  property friend=ref(\"character\", \"lin\") // 保留\n  property a=false\nevent start\n  -> END\n";
    let mut f = fixture(source, "1.13");
    let request = req(
        &f.project,
        "kind,id,friend\ncharacter,lin,lin\n",
        vec![property(
            2,
            "friend",
            CatalogImportType::Ref {
                target_kind: "character".into(),
            },
        )],
    );
    let plan = f.project.preview_catalog_import(&request).unwrap();
    assert!(plan.can_apply, "{:?}", plan.diagnostics);
    assert_eq!(
        plan.runtime_fingerprint_after,
        Some(plan.runtime_fingerprint_before)
    );
    assert!(plan.changed_files.is_empty());
    let csv = "kind,id,name,type,friend\nentity,a,甲,place,b\nentity,b,乙,place,a\n";
    let request = req(
        &f.project,
        csv,
        vec![
            mapping(2, CatalogImportField::Display),
            mapping(3, CatalogImportField::EntityType),
            property(
                4,
                "friend",
                CatalogImportType::Ref {
                    target_kind: "entity".into(),
                },
            ),
        ],
    );
    let plan = f.project.preview_catalog_import(&request).unwrap();
    assert!(plan.can_apply, "{:?}", plan.diagnostics);
    f.project
        .apply_catalog_import(&request, &plan.plan_digest)
        .unwrap();
    let request = req(
        &f.project,
        "kind,id,friend\ncharacter,lin,a\n",
        vec![property(
            2,
            "friend",
            CatalogImportType::Ref {
                target_kind: "entity".into(),
            },
        )],
    );
    let plan = f.project.preview_catalog_import(&request).unwrap();
    assert!(plan.can_apply);
    assert_eq!(
        plan.runtime_fingerprint_after,
        Some(plan.runtime_fingerprint_before)
    );
    f.project
        .apply_catalog_import(&request, &plan.plan_digest)
        .unwrap();
    let text = f.project.document(&f.root.join("world.wl")).unwrap();
    assert!(
        text.starts_with(&source.replace("ref(\"character\", \"lin\")", "ref(\"entity\", \"a\")"))
    );
}
#[test]
fn catalog_import_blank_zero_false_text_and_same_identity_different_kind() {
    let mut f=fixture("character same as \"人\"\n  property keep=42\nentity same kind place as \"地\"\nevent start\n  -> END\n","1.13");
    let mut blank = property(2, "keep", CatalogImportType::Number);
    blank.blank = CatalogBlankPolicy::Keep;
    let mut empty = property(5, "empty", CatalogImportType::Text);
    empty.blank = CatalogBlankPolicy::EmptyText;
    let csv="kind,id,keep,n,b,empty,raw\ncharacter,same,,0,false,,=HYPERLINK(中文)\nentity,same,,0,false,,<script>alert(1)</script>\n";
    let request = req(
        &f.project,
        csv,
        vec![
            blank,
            property(3, "n", CatalogImportType::Number),
            property(4, "b", CatalogImportType::Bool),
            empty,
            property(6, "raw", CatalogImportType::Text),
        ],
    );
    let plan = f.project.preview_catalog_import(&request).unwrap();
    assert!(plan.can_apply, "{:?}", plan.diagnostics);
    assert_eq!(plan.rows.len(), 2);
    f.project
        .apply_catalog_import(&request, &plan.plan_digest)
        .unwrap();
    let text = f.project.document(&f.root.join("world.wl")).unwrap();
    assert!(text.contains("property keep=42"));
    assert_eq!(text.matches("property b = false").count(), 2);
    assert_eq!(text.matches("property empty = \"\"").count(), 2);
    assert!(text.contains("property raw = \"<script>alert(1)</script>\""));
}
#[test]
fn catalog_import_stale_tampered_disk_added_source_and_undo() {
    let mut f = fixture("character lin as \"林\"\nevent start\n  -> END\n", "1.9");
    let request = req(
        &f.project,
        "kind,id,name\ncharacter,lin,新\n",
        vec![mapping(2, CatalogImportField::Display)],
    );
    let plan = f.project.preview_catalog_import(&request).unwrap();
    let original = f.project.content_baseline();
    assert!(f
        .project
        .apply_catalog_import(&request, "tampered")
        .is_err());
    let mut changed = request.clone();
    changed.csv = changed.csv.replace("新", "其他");
    assert!(f
        .project
        .apply_catalog_import(&changed, &plan.plan_digest)
        .is_err());
    assert_eq!(original, f.project.content_baseline());
    std::fs::write(f.root.join("added.wl"), "character other\n").unwrap();
    assert!(f
        .project
        .apply_catalog_import(&request, &plan.plan_digest)
        .is_err());
    std::fs::remove_file(f.root.join("added.wl")).unwrap();
    std::fs::write(f.root.join("world.wl"), "// outside\n").unwrap();
    assert!(f
        .project
        .apply_catalog_import(&request, &plan.plan_digest)
        .is_err());
    assert_eq!(original, f.project.content_baseline());
}
#[test]
fn catalog_import_schema_ref_missing_and_language_capabilities_block() {
    let f=fixture("schema place for entity entity_type place\n  field age_field age number required\nentity a kind place\n  property age=1\nbind entity a to place\nevent start\n  -> END\n","1.13");
    let request = req(
        &f.project,
        "kind,id,age\nentity,a,text\n",
        vec![property(2, "age", CatalogImportType::Text)],
    );
    let plan = f.project.preview_catalog_import(&request).unwrap();
    assert!(!plan.can_apply);
    assert!(plan.runtime_fingerprint_after.is_none());
    let f = fixture(STORY, "1.9");
    let request = req(
        &f.project,
        "kind,id,name,type\nentity,a,甲,place\n",
        vec![
            mapping(2, CatalogImportField::Display),
            mapping(3, CatalogImportField::EntityType),
        ],
    );
    assert!(
        !f.project
            .preview_catalog_import(&request)
            .unwrap()
            .can_apply
    );
    let f = fixture("character lin\nevent start\n  -> END\n", "1.13");
    let request = req(
        &f.project,
        "kind,id,target\ncharacter,lin,missing\n",
        vec![property(
            2,
            "target",
            CatalogImportType::Ref {
                target_kind: "entity".into(),
            },
        )],
    );
    assert!(
        !f.project
            .preview_catalog_import(&request)
            .unwrap()
            .can_apply
    );
}
#[test]
fn catalog_import_strict_json_duplicate_keys_and_unknown_fields() {
    let f = fixture(STORY, "1.9");
    let request = req(&f.project, "kind,id\n", vec![]);
    let json = serde_json::to_string(&request).unwrap();
    assert!(parse_catalog_import_request(&json).is_ok());
    let duplicate = json.replacen("{", "{\"schema_version\":1,", 1);
    assert!(parse_catalog_import_request(&duplicate).is_err());
    let unknown = json.replacen("{", "{\"unknown\":1,", 1);
    assert!(parse_catalog_import_request(&unknown).is_err());
}
#[test]
fn catalog_import_unicode_comment_prefix_exact_bytes_and_embedded_ref_comment_refusal() {
    let source="character /*中文😀*/ lin as \"林\"\n  property /*中文*/ age = /*起点*/28 //末尾\r\nentity port kind place\nentity next kind place\n  property home=ref(\"entity\", /*保留*/ \"port\")\nevent start\n  -> END\n";
    let mut f = fixture(source, "1.13");
    let request = req(
        &f.project,
        "kind,id,age\ncharacter,lin,29\n",
        vec![property(2, "age", CatalogImportType::Number)],
    );
    let plan = f.project.preview_catalog_import(&request).unwrap();
    assert!(plan.can_apply, "{:?}", plan.diagnostics);
    f.project
        .apply_catalog_import(&request, &plan.plan_digest)
        .unwrap();
    assert_eq!(
        f.project.document(&f.root.join("world.wl")).unwrap(),
        source.replace("*/28", "*/29")
    );
    let before = f.project.content_baseline();
    let request = req(
        &f.project,
        "kind,id,home\nentity,next,next\n",
        vec![property(
            2,
            "home",
            CatalogImportType::Ref {
                target_kind: "entity".into(),
            },
        )],
    );
    let plan = f.project.preview_catalog_import(&request).unwrap();
    assert!(!plan.can_apply);
    assert!(f
        .project
        .apply_catalog_import(&request, &plan.plan_digest)
        .is_err());
    assert_eq!(before, f.project.content_baseline());
}
#[test]
fn catalog_import_updates_stay_in_source_new_objects_use_destination_whole_undo() {
    let mut f = fixture(STORY, "1.9");
    std::fs::write(
        f.root.join("people.wl"),
        "character lin as \"林\"\n  property age=28\n",
    )
    .unwrap();
    f.project.refresh().unwrap();
    let before = f.project.clone();
    let request = req(
        &f.project,
        "kind,id,name\ncharacter,lin,新林\ncharacter,mei,梅\n",
        vec![mapping(2, CatalogImportField::Display)],
    );
    let plan = f.project.preview_catalog_import(&request).unwrap();
    assert!(plan.can_apply);
    assert_eq!(plan.changed_files.len(), 2);
    assert_eq!(plan.rows[0].source, Some(PathBuf::from("people.wl")));
    assert_eq!(plan.rows[1].source, Some(PathBuf::from("world.wl")));
    f.project
        .apply_catalog_import(&request, &plan.plan_digest)
        .unwrap();
    f.project.save().unwrap();
    assert!(f.project.restore(before.clone()));
    assert_eq!(f.project.content_baseline(), before.content_baseline());
    assert!(f.project.is_dirty());
}
#[test]
fn catalog_import_blocked_values_keep_identity_source_and_duplicate_detection() {
    let f = fixture("character lin\nevent start\n  -> END\n", "1.9");
    let request = req(
        &f.project,
        "kind,id,age\ncharacter,lin,bad\ncharacter,lin,bad\n",
        vec![property(2, "age", CatalogImportType::Number)],
    );
    let plan = f.project.preview_catalog_import(&request).unwrap();
    assert_eq!(plan.error_count, 3);
    assert_eq!(
        plan.rows[0].target,
        Some(TargetRef::new("character", "lin"))
    );
    assert_eq!(plan.rows[0].source, Some(PathBuf::from("world.wl")));
    assert_eq!(plan.rows[1].operation, "blocked");
}

mod transactions;
use super::*;
use crate::project::Project;
use std::sync::atomic::{AtomicUsize, Ordering};
struct Fixture { root: PathBuf, project: Project }
impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.root); } }
fn fixture(source: &str, version: &str) -> Fixture {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!("catalog-import-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(root.join("world.wl"), source).unwrap();
    if version != "1.9" {
        let manifest = serde_json::json!({"schema_version":1,"language_version":version,"required_features":["content.entities.v1","content.object_refs.v1","content.character_refs.v1"]});
        std::fs::write(root.join(".world/project.json"), manifest.to_string()).unwrap();
    }
    let project = Project::open(&root).unwrap();
    assert!(!project.compile_current().has_errors(), "{:?}", project.compile_current().diagnostics);
    Fixture { root, project }
}
fn mapping(column: usize, field: CatalogImportField) -> CatalogColumnMapping {
    CatalogColumnMapping { column, field, blank: CatalogBlankPolicy::Error }
}
fn req(project: &Project, csv: &str, extra: Vec<CatalogColumnMapping>) -> CatalogImportRequest {
    let mut columns = vec![mapping(0, CatalogImportField::Kind), mapping(1, CatalogImportField::Id)];
    columns.extend(extra);
    CatalogImportRequest { schema_version:1, expected_baseline:project.content_baseline(), csv:csv.into(), destination:"world.wl".into(), columns }
}
fn property(column: usize, key: &str, value_type: CatalogImportType) -> CatalogColumnMapping {
    mapping(column, CatalogImportField::Property { key:key.into(), value_type })
}
const STORY: &str = "event start\n  -> END\n";
#[test]
fn catalog_import_create_entity_and_reimport_idempotent() {
    let mut f = fixture(STORY, "1.13");
    let mut request = req(&f.project, "kind,id,name,type\nentity,harbor,海港,place\n", vec![mapping(2,CatalogImportField::Display),mapping(3,CatalogImportField::EntityType)]);
    let original = f.project.content_baseline();
    let plan = f.project.preview_catalog_import(&request).unwrap();
    assert!(plan.can_apply, "{:?}", plan.diagnostics);
    assert_eq!(original, f.project.content_baseline());
    assert_eq!(plan.rows[0].operation,"create");
    let before = f.project.clone();
    f.project.apply_catalog_import(&request,&plan.plan_digest).unwrap();
    assert!(f.project.document(&f.root.join("world.wl")).unwrap().contains("entity harbor kind place as \"海港\""));
    assert_eq!(plan.runtime_fingerprint_after,Some(plan.runtime_fingerprint_before));
    f.project.save().unwrap();
    let saved = std::fs::read(f.root.join("world.wl")).unwrap();
    request.expected_baseline=f.project.content_baseline();
    let second = f.project.preview_catalog_import(&request).unwrap();
    assert!(second.can_apply);
    assert!(second.changed_files.is_empty());
    assert_eq!(second.rows[0].operation,"unchanged");
    f.project.apply_catalog_import(&request,&second.plan_digest).unwrap();
    assert!(!f.project.is_dirty());
    assert_eq!(saved,std::fs::read(f.root.join("world.wl")).unwrap());
    assert!(f.project.restore(before));
    assert!(f.project.is_dirty());
}
#[test]
fn catalog_import_exact_property_patches_keep_comments_and_mixed_newlines() {
    let source = "character lin as \"林舟\" // 表头\r\n  property z = \"未动\"\n  property age=28 // 注释\r\n  property a = false\n\nevent start\r\n  -> END\n";
    let mut f=fixture(source,"1.9");
    let request=req(&f.project,"kind,id,age\ncharacter,lin,29\n",vec![property(2,"age",CatalogImportType::Number)]);
    let plan=f.project.preview_catalog_import(&request).unwrap();
    assert!(plan.can_apply,"{:?}",plan.diagnostics);
    f.project.apply_catalog_import(&request,&plan.plan_digest).unwrap();
    assert_eq!(f.project.document(&f.root.join("world.wl")).unwrap(),source.replace("age=28","age=29"));
    assert_ne!(plan.runtime_fingerprint_after,Some(plan.runtime_fingerprint_before));
}
#[test]
fn catalog_import_csv_quotes_bom_newline_limits() {
    let table=parse_catalog_csv("\u{feff}kind,id,name\r\ncharacter,a,\"甲,\"\"乙\"\"\r\n丙\"\r\n").unwrap();
    assert_eq!(table.rows[0].cells[2],"甲,\"乙\"\n丙");
    assert_eq!(table.normalization_count,1);
    for csv in ["a,a\n1,2", "a,\n1,2", "a,b\n1", "a\n\"x", "a\nx\"y", "a\n\"x\"x", "a\nx\ry"] {
        assert!(parse_catalog_csv(csv).is_err(),"{csv}");
    }
    assert!(parse_catalog_csv(&format!("a\n{}","x".repeat(MAX_CELL_BYTES+1))).is_err());
}
#[test]
fn catalog_import_collects_all_independent_errors_and_never_truncates_validity() {
    let f=fixture(STORY,"1.9");
    let mut csv="kind,id,age,alive\n".to_owned();
    for index in 0..101 { csv.push_str(&format!("character,c{index},nope,TRUE\n")); }
    let request=req(&f.project,&csv,vec![property(2,"age",CatalogImportType::Number),property(3,"alive",CatalogImportType::Bool)]);
    let plan=f.project.preview_catalog_import(&request).unwrap();
    assert_eq!(plan.error_count,202);
    assert_eq!(plan.diagnostics.len(),100);
    assert!(!plan.can_apply);
}
#[test]
fn catalog_import_rejects_omission_duplicates_and_nonfinite() {
    let mut f=fixture("character lin\nevent start\n  -> END\n","1.9");
    let original=f.project.content_baseline();
    for (csv,extra) in [
        ("kind,id,n\ncharacter,lin,NaN\n", vec![property(2,"n",CatalogImportType::Number)]),
        ("kind,id,n\ncharacter,lin,9007199254740993\n", vec![property(2,"n",CatalogImportType::Number)]),
        ("kind,id,n\ncharacter,lin,0\n", vec![]),
        ("kind,id,n\ncharacter,lin,0\ncharacter,lin,0\n", vec![property(2,"n",CatalogImportType::Number)]),
    ] {
        let request=req(&f.project,csv,extra);
        let plan=f.project.preview_catalog_import(&request).unwrap();
        assert!(!plan.can_apply,"{csv}");
        assert!(f.project.apply_catalog_import(&request,&plan.plan_digest).is_err());
        assert_eq!(original,f.project.content_baseline());
    }
}

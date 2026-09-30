use std::path::PathBuf;
use worldline_core::{project::Project, source_edit::SourceEditRequest};
fn fixture() -> Project {
    let root = std::env::temp_dir().join(format!("source-edit-{}", std::process::id()));
    let mut p = Project::new(&root);
    let entry = p.entry.clone();
    p.set_text(&entry, "event start\n  原稿。\n  -> END\n".into())
        .unwrap();
    p
}
#[test]
fn full_source_preview_is_readonly_and_apply_requires_exact_digest_and_baseline() {
    let mut p = fixture();
    let original = p.content_baseline();
    let request = SourceEditRequest {
        schema_version: 1,
        path: PathBuf::from("world.wl"),
        expected_baseline: original.clone(),
        source: "event start\n  if (\n    未完稿\n".into(),
    };
    let plan = p.preview_source_edit(&request).unwrap();
    assert!(!plan.diagnostics.is_empty());
    assert_eq!(p.content_baseline(), original);
    assert!(p.apply_source_edit(&request, "wrong").is_err());
    assert_eq!(p.content_baseline(), original);
    let before = p.clone();
    p.apply_source_edit(&request, &plan.plan_digest).unwrap();
    assert_eq!(p.document(&p.entry).unwrap(), request.source);
    assert!(p.apply_source_edit(&request, &plan.plan_digest).is_err());
    assert!(p.restore(before));
    assert_eq!(p.content_baseline(), original);
}
#[test]
fn unsupported_dto_and_outside_or_unknown_paths_never_write() {
    let p = fixture();
    let baseline = p.content_baseline();
    for path in [
        "../outside.wl",
        "/tmp/outside.wl",
        "missing.wl",
        ".world/project.json",
    ] {
        let r = SourceEditRequest {
            schema_version: 1,
            path: path.into(),
            expected_baseline: baseline.clone(),
            source: String::new(),
        };
        assert!(p.preview_source_edit(&r).is_err());
        assert_eq!(p.content_baseline(), baseline);
    }
    let r = SourceEditRequest {
        schema_version: 999,
        path: "world.wl".into(),
        expected_baseline: baseline,
        source: String::new(),
    };
    assert!(p.preview_source_edit(&r).is_err());
}

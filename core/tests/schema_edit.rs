use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{project::Project, source_edit::SourceEditRequest, TargetRef};

const SCHEMA: &str = "schema city for entity entity_type place closed\n  field people_id population number required\n";
const FACTS: &str = "entity harbor kind place as \"港城\"\n  property population = 0\nbind entity harbor to city\nevent start\n  你好。\n  -> END\n";
struct Fixture {
    root: PathBuf,
    project: Project,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn fixture(name: &str) -> Fixture {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "schema-edit-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"language_version":"1.12","required_features":[]}"#,
    )
    .unwrap();
    fs::write(root.join("world.wl"), FACTS).unwrap();
    fs::write(root.join("schema.wl"), SCHEMA).unwrap();
    let mut project = Project::open(&root).unwrap();
    assert!(
        !project.compile().has_errors(),
        "{:?}",
        project.compile().diagnostics
    );
    Fixture { root, project }
}
fn request(project: &Project, source: &str) -> SourceEditRequest {
    SourceEditRequest {
        schema_version: 1,
        path: PathBuf::from("schema.wl"),
        expected_baseline: project.content_baseline(),
        source: source.into(),
    }
}

#[test]
fn schema_change_preview_cancel_apply_and_undo_redo_are_atomic() {
    let mut f = fixture("undo");
    let before = f.project.clone();
    let baseline = f.project.content_baseline();
    let fingerprint = f.project.compile().analysis.fingerprint;
    let source = SCHEMA.replace("population number", "inhabitants number");
    let request = request(&f.project, &source);
    let preview = f.project.preview_schema_edit(&request).unwrap();
    assert_eq!(
        f.project.content_baseline(),
        baseline,
        "预览/取消不能改原稿"
    );
    assert_eq!(preview.expected_baseline, baseline);
    assert!(preview
        .field_changes
        .iter()
        .any(
            |change| change.field_id.as_deref() == Some("people_id") && change.change == "renamed"
        ));
    assert_eq!(preview.instance_impacts.len(), 1);
    assert_eq!(
        preview.instance_impacts[0].target,
        TargetRef::new("entity", "harbor")
    );
    assert!(preview.instance_impacts[0].before_diagnostics.is_empty());
    assert!(preview.instance_impacts[0]
        .after_diagnostics
        .iter()
        .any(|d| d.code == "SCH004"));
    let applied = f
        .project
        .apply_schema_edit(&request, &preview.plan_digest)
        .unwrap();
    assert!(applied.after_diagnostics.iter().any(|d| d.code == "SCH008"));
    assert!(f
        .project
        .document(&f.root.join("world.wl"))
        .unwrap()
        .contains("population = 0"));
    assert!(!f
        .project
        .document(&f.root.join("world.wl"))
        .unwrap()
        .contains("inhabitants ="));
    assert_eq!(f.project.compile().analysis.fingerprint, fingerprint);
    let after = f.project.clone();
    assert!(f.project.restore(before));
    assert_eq!(f.project.content_baseline(), baseline);
    assert!(f.project.restore(after));
    assert_eq!(f.project.content_baseline(), preview.plan_digest);
    assert!(f
        .project
        .apply_schema_edit(&request, &preview.plan_digest)
        .is_err());
}

#[test]
fn type_required_and_schema_removal_impacts_never_migrate_values() {
    let mut f = fixture("types");
    let source = SCHEMA.replace("number required", "text required")
        + "  field new_id founded number required\n";
    let request = request(&f.project, &source);
    let preview = f.project.preview_schema_edit(&request).unwrap();
    assert!(preview
        .field_changes
        .iter()
        .any(|c| c.change == "type_changed"));
    assert!(preview.field_changes.iter().any(|c| c.change == "added"));
    assert!(preview.after_diagnostics.iter().any(|d| d.code == "SCH005"));
    assert!(preview.after_diagnostics.iter().any(|d| d.code == "SCH004"));
    f.project
        .apply_schema_edit(&request, &preview.plan_digest)
        .unwrap();
    assert_eq!(f.project.document(&f.root.join("world.wl")).unwrap(), FACTS);
    let remove = request_for(&f.project, "");
    let preview = f.project.preview_schema_edit(&remove).unwrap();
    assert!(preview
        .field_changes
        .iter()
        .any(|c| c.change == "schema_removed"));
    assert!(!preview.complete);
    assert_eq!(preview.instance_impacts.len(), 1);
    assert!(preview.after_diagnostics.iter().any(|d| d.code == "SCH003"));
}
fn request_for(project: &Project, source: &str) -> SourceEditRequest {
    request(project, source)
}

#[test]
fn stale_baseline_forged_digest_and_external_file_change_reject_without_mutation() {
    let mut f = fixture("conflicts");
    let request = request(&f.project, &SCHEMA.replace("number", "text"));
    let preview = f.project.preview_schema_edit(&request).unwrap();
    let baseline = f.project.content_baseline();
    assert!(f
        .project
        .apply_schema_edit(&request, "wrong-digest")
        .is_err());
    assert_eq!(f.project.content_baseline(), baseline);
    fs::write(f.root.join("world.wl"), format!("{FACTS}// 外部修改\n")).unwrap();
    assert!(f
        .project
        .apply_schema_edit(&request, &preview.plan_digest)
        .is_err());
    assert_eq!(f.project.content_baseline(), baseline);
    fs::write(f.root.join("world.wl"), FACTS).unwrap();
    f.project
        .set_text(&f.root.join("world.wl"), format!("{FACTS}// 本地修改\n"))
        .unwrap();
    let current = f.project.content_baseline();
    assert!(f
        .project
        .apply_schema_edit(&request, &preview.plan_digest)
        .is_err());
    assert_eq!(f.project.content_baseline(), current);
}

#[test]
fn unknown_capability_and_old_version_remain_protected() {
    for manifest in [
        br#"{"schema_version":1,"language_version":"1.12","required_features":["future.unknown.v1"]}"#.as_slice(),
        br#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#.as_slice(),
        br#"{"schema_version":1,"language_version":"1.99","required_features":[]}"#.as_slice(),
    ] {
        let mut f = fixture("readonly");
        fs::write(f.root.join(".world/project.json"), manifest).unwrap();
        f.project = Project::open(&f.root).unwrap();
        let baseline = f.project.content_baseline();
        let request = request(&f.project, SCHEMA);
        assert!(f.project.preview_schema_edit(&request).is_err());
        assert!(f.project.apply_schema_edit(&request, "anything").is_err());
        assert_eq!(baseline, f.project.content_baseline());
    }
}

#[test]
fn invalid_draft_can_save_and_reopen_but_cannot_publish() {
    let mut f = fixture("draft");
    let request = request(&f.project, &SCHEMA.replace("number", "text"));
    let preview = f.project.preview_schema_edit(&request).unwrap();
    f.project
        .apply_schema_edit(&request, &preview.plan_digest)
        .unwrap();
    f.project.save().unwrap();
    let mut reopened = Project::open(&f.root).unwrap();
    assert!(reopened.compile().has_errors());
    assert_eq!(
        reopened.document(&f.root.join("schema.wl")).unwrap(),
        request.source
    );
    let selection = serde_json::from_value(serde_json::json!({
        "schema_version":2,"required_features":["reader.fields.v1"],"site_title":"公开",
        "objects":[{"kind":"event","id":"start"}],"manuscripts":[],"attachments":[]
    }))
    .unwrap();
    let error = reopened.preview_reader_export(&selection).unwrap_err();
    assert!(error.contains("SCH005"));
}

#[test]
fn malformed_draft_is_retained_and_reports_incomplete_impact() {
    let mut f = fixture("malformed");
    let source = "schema city for entity\n  field incomplete\n";
    let request = request(&f.project, source);
    let preview = f.project.preview_schema_edit(&request).unwrap();
    assert!(!preview.complete);
    assert!(preview.after_diagnostics.iter().any(|d| d.code == "SCH001"));
    f.project
        .apply_schema_edit(&request, &preview.plan_digest)
        .unwrap();
    assert_eq!(
        f.project.document(&f.root.join("schema.wl")).unwrap(),
        source
    );
}

#[test]
fn bindings_are_static_references_for_safe_rename_and_delete() {
    let mut f = fixture("rename");
    let target = TargetRef::new("entity", "harbor");
    let impact = f.project.deletion_impact(&target);
    assert!(impact.complete, "{:?}", impact.diagnostics);
    assert!(!impact.can_delete());
    assert!(impact
        .content_references
        .iter()
        .any(|r| r.kind == "schema 对象绑定"));
    let fingerprint = f.project.compile().analysis.fingerprint;
    let plan = f.project.plan_rename_target(&target, "port").unwrap();
    f.project.apply_rename_plan(&plan).unwrap();
    let compiled = f.project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert_eq!(compiled.program.schema_bindings[0].target.id, "port");
    assert_eq!(compiled.analysis.fingerprint, fingerprint);
    assert!(f
        .project
        .document(&f.root.join("world.wl"))
        .unwrap()
        .contains("bind entity port to city"));
}

#[test]
fn character_binding_rename_keeps_comment_and_schema_identity() {
    let mut f = fixture("character");
    f.project
        .set_text(
            &f.root.join("schema.wl"),
            "schema person for character\n".into(),
        )
        .unwrap();
    f.project
        .set_text(
            &f.root.join("world.wl"),
            "character mira\nbind character mira to person // mira 不应改\nevent start\n  -> END\n"
                .into(),
        )
        .unwrap();
    let plan = f
        .project
        .plan_rename_target(&TargetRef::new("character", "mira"), "nora")
        .unwrap();
    f.project.apply_rename_plan(&plan).unwrap();
    assert!(f
        .project
        .document(&f.root.join("world.wl"))
        .unwrap()
        .contains("bind character nora to person // mira 不应改"));
    assert_eq!(f.project.schema_index().schemas[0].id, "person");
}

#[test]
fn schemas_are_not_implicitly_published_or_ref_kinds_expanded() {
    let f = fixture("privacy");
    let selection = serde_json::from_value(serde_json::json!({
        "schema_version":2,"required_features":["reader.fields.v1"],"site_title":"公开",
        "objects":[{"kind":"event","id":"start"}],"manuscripts":[],"attachments":[]
    }))
    .unwrap();
    let preview = f.project.preview_reader_export(&selection).unwrap();
    let files = f
        .project
        .build_reader_export(&selection, &preview.plan_digest)
        .unwrap();
    let text = files
        .values()
        .map(|b| String::from_utf8_lossy(b))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!text.contains("people_id"));
    assert!(!text.contains("population"));
    assert_eq!(
        worldline_core::catalog::OBJECT_REFERENCE_TARGET_KINDS,
        &["entity", "relation"]
    );
    assert!(f
        .project
        .schema_index()
        .schemas
        .iter()
        .all(|s| Path::new(&s.file).is_absolute()));
}

#[test]
fn duplicate_properties_keep_existing_diagnostics_and_mark_impact_incomplete() {
    let mut f = fixture("duplicate-property");
    let facts = FACTS.replace(
        "  property population = 0",
        "  property population = 0\n  property population = \"重复键\"",
    );
    f.project.set_text(&f.root.join("world.wl"), facts).unwrap();
    let request = request(&f.project, &SCHEMA.replace("number", "text"));
    let preview = f.project.preview_schema_edit(&request).unwrap();
    assert!(!preview.complete);
    assert!(preview.before_diagnostics.iter().any(|d| d.code == "A212"));
    assert!(preview.after_diagnostics.iter().any(|d| d.code == "A212"));
    f.project
        .apply_schema_edit(&request, &preview.plan_digest)
        .unwrap();
    assert_eq!(
        f.project
            .document(&f.root.join("world.wl"))
            .unwrap()
            .matches("property population")
            .count(),
        2
    );
}

#[test]
fn duplicate_bindings_link_the_original_file_and_line() {
    let mut f = fixture("binding-location");
    let path = f.project.add_file(Path::new("z_binding.wl")).unwrap();
    f.project
        .set_text(&path, "bind entity harbor to city\n".into())
        .unwrap();
    let result = f.project.compile();
    let duplicate = result
        .diagnostics
        .iter()
        .find(|d| d.code == "SCH003" && d.message.contains("重复绑定"))
        .unwrap();
    assert_eq!(duplicate.file, path.to_string_lossy());
    assert_eq!(duplicate.span.line, 1);
    assert_eq!(duplicate.related.len(), 1);
    assert_eq!(
        duplicate.related[0].0,
        f.root.join("world.wl").to_string_lossy()
    );
    assert_eq!(duplicate.related[0].1.line, 3);
}

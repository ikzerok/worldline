use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);
fn project() -> Project {
    let root = std::env::temp_dir().join(format!(
        "worldline-template-protocol-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("world.wl"),
        "character author as \"作者\"\nevent start\n  -> END\n",
    )
    .unwrap();
    Project::open(&root.join("world.wl")).unwrap()
}
fn request(project: &Project) -> TemplateMutationRequest {
    TemplateMutationRequest {
        schema_version: 1,
        expected_revision: Revision::default(),
        expected_baseline: project.content_baseline(),
        intent: TemplateMutationIntent::Upsert {
            draft: ProjectTemplateDraft {
                existing_id: None,
                source_bytes: serde_json::to_vec_pretty(&json!({
                    "schema_version":1,"id":"project:profile","title":"人物字段",
                    "applies_to":{"kind":"character"},
                    "fields":[{"id":"note","key":"note","label":"备注","type":"text","required":false,"default":"提示"}],
                    "extension":{"keep":[0,false,""]}
                })).unwrap(),
                ..Default::default()
            },
        },
    }
}

#[test]
fn template_protocol_preview_is_read_only_and_apply_requires_exact_rebuilt_digest() {
    let mut project = project();
    let before = project.clone();
    let original = project.document(&project.entry).unwrap().to_owned();
    let fingerprint = project
        .compile_object_search_snapshot()
        .analysis
        .fingerprint;
    let request = request(&project);
    let plan = project
        .preview_template_request(Revision::default(), &request)
        .unwrap();
    assert!(plan.can_apply);
    assert_eq!(plan.operation, "import");
    assert_eq!(project.content_baseline(), request.expected_baseline);
    assert!(!project.root.join(".world/project.json").exists());
    let mut revision = Revision::default();
    assert_eq!(
        project
            .apply_template_request(&mut revision, &request, "forged")
            .unwrap_err()
            .code,
        "STALE_PLAN"
    );
    assert_eq!(project.content_baseline(), request.expected_baseline);
    let mut changed = request.clone();
    if let TemplateMutationIntent::Upsert { draft } = &mut changed.intent {
        let mut value: serde_json::Value = serde_json::from_slice(&draft.source_bytes).unwrap();
        value["title"] = json!("另一份标题");
        draft.source_bytes = serde_json::to_vec(&value).unwrap();
    }
    assert_eq!(
        project
            .apply_template_request(&mut revision, &changed, &plan.plan_digest)
            .unwrap_err()
            .code,
        "STALE_PLAN"
    );
    let result = project
        .apply_template_request(&mut revision, &request, &plan.plan_digest)
        .unwrap();
    assert!(result.applied && !result.saved);
    assert_eq!(project.document(&project.entry).unwrap(), original);
    assert_eq!(
        project
            .compile_object_search_snapshot()
            .analysis
            .fingerprint,
        fingerprint
    );
    assert!(project
        .template_index()
        .projects
        .contains_key("project:profile"));
    assert!(project.restore(before));
    // restore保留新文档的删除墓碑，保证应用后已保存再撤销也能安全删盘；
    // 内容恢复不承诺旧计划baseline复活。
    assert_eq!(project.document(&project.entry).unwrap(), original);
    assert!(!project
        .template_index()
        .projects
        .contains_key("project:profile"));
    assert_eq!(
        project
            .apply_template_request(&mut revision, &request, &plan.plan_digest)
            .unwrap_err()
            .code,
        "STALE_BASELINE"
    );
    let mut retry = request.clone();
    retry.expected_baseline = project.content_baseline();
    retry.expected_revision = revision;
    assert!(
        project
            .preview_template_request(revision, &retry)
            .unwrap()
            .can_apply
    );
    assert_eq!(
        project
            .compile_object_search_snapshot()
            .analysis
            .fingerprint,
        fingerprint
    );
}

#[test]
fn template_protocol_apply_save_reopen_preserves_extensions_and_avoids_instance_defaults() {
    let mut project = project();
    let request = request(&project);
    let mut revision = Revision::default();
    let plan = project
        .preview_template_request(revision, &request)
        .unwrap();
    project
        .apply_template_request(&mut revision, &request, &plan.plan_digest)
        .unwrap();
    project.save().unwrap();
    let reopened = Project::open(&project.entry).unwrap();
    let index = reopened.template_index();
    let source = index.projects["project:profile"]
        .source_document
        .as_ref()
        .unwrap();
    assert_eq!(source["extension"]["keep"], json!([0, false, ""]));
    assert!(!reopened
        .document(&reopened.entry)
        .unwrap()
        .contains("property note"));
    assert_eq!(reopened.language_version(), "1.9");
}

#[test]
fn template_protocol_rejects_stale_buffer_or_external_disk_before_partial_commit() {
    let mut project = project();
    let request = request(&project);
    let plan = project
        .preview_template_request(Revision::default(), &request)
        .unwrap();
    let baseline = project.content_baseline();
    std::fs::write(&project.entry, "event outside\n  -> END\n").unwrap();
    let error = project
        .apply_template_request(&mut Revision::default(), &request, &plan.plan_digest)
        .unwrap_err();
    assert_eq!(error.code, "PREVIEW_REJECTED");
    assert_eq!(project.content_baseline(), baseline);
    assert!(project.template_index().projects.is_empty());
    let mut stale = request;
    stale.expected_baseline = "old".into();
    assert_eq!(
        project
            .preview_template_request(Revision::default(), &stale)
            .unwrap_err()
            .code,
        "STALE_BASELINE"
    );
}

#[test]
fn template_protocol_strict_wire_and_schema_budget_are_independent_of_raw_extensions() {
    let project = project();
    let request = request(&project);
    let text = serde_json::to_string(&request).unwrap();
    assert_eq!(parse_template_mutation_request(&text).unwrap(), request);
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["expected_revision"]["extra"] = json!(true);
    assert!(parse_template_mutation_request(&value.to_string()).is_err());
    assert!(parse_template_mutation_request(&" ".repeat(MAX_TEMPLATE_REQUEST_BYTES + 1)).is_err());
    let mut unsupported = request;
    unsupported.schema_version = 99;
    assert_eq!(
        project
            .preview_template_request(Revision::default(), &unsupported)
            .unwrap_err()
            .code,
        "UNSUPPORTED_SCHEMA"
    );
}

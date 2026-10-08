use super::*;
use worldline_core::project_templates::{ProjectTemplatePreview, TemplateImpactLimits};

fn limits(instances: usize, fields: usize, bytes: usize) -> TemplateImpactLimits {
    TemplateImpactLimits {
        max_instances: instances,
        max_field_values: fields,
        max_output_bytes: bytes,
    }
}

fn document(kind: &str, title: &str, fields: Vec<Value>) -> Vec<u8> {
    serde_json::to_vec(
        &json!({"schema_version":1,"id":"project:budget","title":title,
        "applies_to":{"kind":kind},"fields":fields}),
    )
    .unwrap()
}

fn fields(count: usize) -> Vec<Value> {
    (0..count)
        .map(|index| {
            json!({"id":format!("f_{index}"),"key":format!("key_{index}"),
        "label":"预算字段","type":"text","required":false})
        })
        .collect()
}

fn report_bytes(preview: &ProjectTemplatePreview) -> usize {
    serde_json::to_vec(&json!({
        "expected_revision":preview.expected_revision,"expected_baseline":preview.expected_baseline,
        "complete":preview.complete,"incomplete_reason":preview.incomplete_reason,
        "current_template":preview.current_template,"proposed_template":preview.proposed_template,
        "field_changes":preview.field_changes,"instances":preview.instances,
        "diagnostics":preview.diagnostics,"changed_files":preview.changed_files
    }))
    .unwrap()
    .len()
}

#[test]
fn impact_limits_preflight_actual_object_field_counts_before_building_a_report() {
    let mut project = project("draft-budget-counts");
    let mut source = String::new();
    for index in 0..120 {
        source.push_str(&format!("entity item_{index} kind place\n"));
    }
    source.push_str("event start\n  -> END\n");
    project.set_text(&project.entry.clone(), source).unwrap();
    let revision = Revision::default();
    let mutation = project
        .template_mutation_from_bytes(&document("entity", "多字段", fields(80)))
        .unwrap();
    let request = command(&project, revision, mutation);
    let baseline = project.content_baseline();
    let error = project
        .preview_template_mutation_with_limits(revision, &request, &limits(120, 200, 0))
        .unwrap_err();
    assert!(error.starts_with("TemplateImpactLimit："));
    assert!(error.contains("字段值"), "{error}");
    let error = project
        .preview_template_mutation_with_limits(
            revision,
            &request,
            &limits(0, usize::MAX, usize::MAX),
        )
        .unwrap_err();
    assert!(error.contains("实例数量"));
    let full = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    assert!(full.complete);
    assert_eq!(full.instances.len(), 120);
    assert_eq!(
        full.instances
            .iter()
            .map(|row| row.fields.len())
            .sum::<usize>(),
        9600
    );
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn impact_byte_limit_matches_exact_json_including_escaped_values_summaries_and_warnings() {
    let project = project("draft-budget-exact");
    let bytes = document(
        "character",
        "标题\n\"转义\"",
        vec![
            json!({"id":"home","key":"home","label":"家","type":"number","required":false}),
            json!({"id":"missing","key":"missing","label":"缺值","type":"text","required":false}),
        ],
    );
    let revision = Revision::default();
    let request = command(
        &project,
        revision,
        project.template_mutation_from_bytes(&bytes).unwrap(),
    );
    let full = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    assert!(full
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "TPL006"));
    let exact = report_bytes(&full);
    let bounded = project
        .preview_template_mutation_with_limits(revision, &request, &limits(1, 2, exact))
        .unwrap();
    assert_eq!(report_bytes(&bounded), exact);
    assert!(bounded.complete);
    let error = project
        .preview_template_mutation_with_limits(revision, &request, &limits(1, 2, exact - 1))
        .unwrap_err();
    assert!(error.starts_with("TemplateImpactLimit："));
    let error = project
        .preview_template_mutation_with_limits(revision, &request, &limits(1, 0, usize::MAX))
        .unwrap_err();
    assert!(error.contains("字段值"));
}

#[test]
fn one_long_property_value_is_budgeted_without_mutation_and_unlimited_preview_remains_available() {
    let mut project = project("draft-budget-long-value");
    let long = "正文".repeat(65_536);
    let source = format!("character author\n  property blob = \"{long}\"\nevent start\n  -> END\n");
    project.set_text(&project.entry.clone(), source).unwrap();
    let bytes = document(
        "character",
        "长值",
        vec![json!({"id":"blob","key":"blob","label":"正文","type":"text","required":false})],
    );
    let revision = Revision::default();
    let request = command(
        &project,
        revision,
        project.template_mutation_from_bytes(&bytes).unwrap(),
    );
    let baseline = project.content_baseline();
    let error = project
        .preview_template_mutation_with_limits(revision, &request, &limits(1, 1, 4096))
        .unwrap_err();
    assert!(error.contains("字节"), "{error}");
    assert_eq!(project.content_baseline(), baseline);
    let full = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    assert_eq!(full.instances.len(), 1);
    assert!(report_bytes(&full) > long.len());
}

#[test]
fn impact_limits_cover_long_metadata_field_changes_and_true_zero_boundaries() {
    let project = project("draft-budget-metadata");
    let revision = Revision::default();
    for bytes in [
        document("world", &"长标题".repeat(16_384), vec![]),
        document(
            "world",
            "长字段",
            vec![
                json!({"id":"f","key":"long_key".repeat(8192),"label":"字段","type":"text","required":false}),
            ],
        ),
    ] {
        let request = command(
            &project,
            revision,
            project.template_mutation_from_bytes(&bytes).unwrap(),
        );
        let error = project
            .preview_template_mutation_with_limits(revision, &request, &limits(1, 1, 1024))
            .unwrap_err();
        assert!(error.contains("字节"));
    }
    let request = command(
        &project,
        revision,
        project
            .template_mutation_from_bytes(&document("entity", "空定义", vec![]))
            .unwrap(),
    );
    let empty_fields = project
        .preview_template_mutation_with_limits(revision, &request, &limits(1, 0, 8192))
        .unwrap();
    assert_eq!(empty_fields.instances.len(), 1);
    assert!(empty_fields.instances[0].fields.is_empty());
    let error = project
        .preview_template_mutation_with_limits(revision, &request, &limits(1, 0, 0))
        .unwrap_err();
    assert!(error.contains("字节"));
    assert!(serde_json::from_value::<TemplateImpactLimits>(
        json!({"max_instances":1,"max_field_values":1,"max_output_bytes":1,"unknown":true})
    )
    .is_err());
}

#[test]
fn bounded_preview_preserves_source_incomplete_reason_and_counts_only_applicable_sides() {
    let mut project = project("draft-budget-incomplete");
    project
        .set_text(
            &project.entry.clone(),
            "include \"missing.wl\"\nevent start\n  -> END\n".into(),
        )
        .unwrap();
    let revision = Revision::default();
    let request = command(
        &project,
        revision,
        project
            .template_mutation_from_bytes(&document("entity", "不完整", vec![]))
            .unwrap(),
    );
    let incomplete = project
        .preview_template_mutation_with_limits(revision, &request, &limits(0, 0, 32 * 1024))
        .unwrap();
    assert!(!incomplete.complete);
    assert!(incomplete
        .incomplete_reason
        .as_ref()
        .unwrap()
        .contains("编译错误"));
    let mut next = revision;
    assert!(project
        .apply_template_mutation(&mut next, incomplete)
        .is_err());
    let mut project = super::project("draft-budget-scope");
    let old = document("character", "人物", fields(1));
    register(&mut project, old);
    let new = document("entity", "实体", fields(1));
    let request = command(
        &project,
        revision,
        project.template_mutation_from_bytes(&new).unwrap(),
    );
    // 默认fixture分别有1个人物、1个entity：每行仅对应一侧，合计2个字段值。
    let preview = project
        .preview_template_mutation_with_limits(revision, &request, &limits(2, 2, 32 * 1024))
        .unwrap();
    assert_eq!(preview.instances.len(), 2);
    assert_eq!(
        preview
            .instances
            .iter()
            .map(|row| row.fields.len())
            .sum::<usize>(),
        2
    );
}

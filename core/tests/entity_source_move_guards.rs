//! 源码、库存、能力、资源与完整计划守卫必须零修改失败。
#[path = "support/entity_source_move_fixture.rs"]
mod fixture;
use fixture::*;
use worldline_core::source_lifecycle::{
    SourceLifecycleFailureKind as Kind, SourceLifecycleRequest,
};

#[test]
fn inactive_missing_deleted_illegal_and_unsupported_targets_reject_without_changes() {
    let fixture = Fixture::simple();
    let mut project = fixture.project();
    let disk = fixture.bytes();
    let baseline = project.content_baseline();
    for target in [
        "archive.wl",
        "inactive.wl",
        "missing.wl",
        "../outside.wl",
        "assets/picture.txt",
        ".world/.transactions/hidden.wl",
    ] {
        assert!(
            project
                .preview_source_lifecycle(&fixture.request(target))
                .is_err(),
            "不应接受目标 {target}"
        );
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(fixture.bytes(), disk);
    }
    for id in ["", "missing", "start", "secret", "inactive"] {
        let request = SourceLifecycleRequest::MoveEntity {
            id: id.into(),
            to: TARGET.into(),
        };
        assert!(
            project.preview_source_lifecycle(&request).is_err(),
            "不应接受实体 {id}"
        );
        assert_eq!(project.content_baseline(), baseline);
    }
    project.delete_document(&fixture.root.join(TARGET)).unwrap();
    let deleted_baseline = project.content_baseline();
    assert!(project
        .preview_source_lifecycle(&fixture.request(TARGET))
        .is_err());
    assert_eq!(project.content_baseline(), deleted_baseline);
    assert_eq!(fixture.bytes(), disk);
}

#[test]
fn duplicate_entity_invalid_draft_and_unknown_capabilities_also_reject_same_file() {
    for source in [
        "entity north_lighthouse kind place\nentity north_lighthouse kind item\n",
        "entity north_lighthouse kind place\n  property broken = (\n",
    ] {
        let fixture = Fixture::new(source, "// 目标\n");
        let project = fixture.project();
        let baseline = project.content_baseline();
        for target in [SOURCE, TARGET] {
            assert!(project
                .preview_source_lifecycle(&fixture.request(target))
                .is_err());
            assert_eq!(project.content_baseline(), baseline);
        }
    }
    let fixture = Fixture::simple();
    let manifest_path = fixture.root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["required_features"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("future.entity_move.v99"));
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();
    let project = fixture.project();
    let disk = fixture.bytes();
    let baseline = project.content_baseline();
    for target in [SOURCE, TARGET] {
        assert!(project
            .preview_source_lifecycle(&fixture.request(target))
            .is_err());
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(fixture.bytes(), disk);
    }
}

#[test]
fn source_and_target_as_raw_attachments_reject_the_byte_changing_move() {
    for relative in [SOURCE, TARGET] {
        let fixture = Fixture::simple();
        let world = std::fs::read_to_string(fixture.root.join("world.wl")).unwrap();
        fixture.write(
            "world.wl",
            &format!("asset original file \"{relative}\" as \"原始源码附件\"\n{world}"),
        );
        let mut project = fixture.project();
        assert!(!project.compile().has_errors());
        let baseline = project.content_baseline();
        let disk = fixture.bytes();
        let failure = project
            .preview_source_lifecycle_classified(&fixture.request(TARGET))
            .unwrap_err();
        assert_eq!(failure.kind, Kind::SemanticChange);
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(fixture.bytes(), disk);
    }
}

#[test]
fn changing_any_source_buffer_after_preview_rejects_the_whole_old_plan() {
    for relative in [SOURCE, TARGET, "world.wl", "inactive.wl"] {
        let fixture = Fixture::simple();
        let mut project = fixture.project();
        let plan = fixture.plan();
        let path = fixture.root.join(relative);
        let original = project.document(&path).unwrap().to_owned();
        project
            .set_text(&path, format!("{original}// 作者后续编辑🙂\n"))
            .unwrap();
        let baseline = project.content_baseline();
        let buffers = project.sources();
        let disk = fixture.bytes();
        assert_eq!(
            project
                .apply_source_lifecycle_plan_classified(&plan)
                .unwrap_err()
                .kind,
            Kind::SourceChanged
        );
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(project.sources(), buffers);
        assert_eq!(fixture.bytes(), disk);
    }
}

#[test]
fn external_source_inventory_resource_and_manifest_changes_never_overwrite_buffers() {
    for relative in [
        SOURCE,
        TARGET,
        "world.wl",
        "assets/picture.txt",
        "extra.wl",
        ".world/project.json",
    ] {
        let fixture = Fixture::full();
        let mut project = fixture.project();
        let plan = fixture.plan();
        let buffers = project.sources();
        let baseline = project.content_baseline();
        let old = std::fs::read_to_string(fixture.root.join(relative)).unwrap_or_default();
        let suffix = if relative.ends_with(".json") {
            " "
        } else {
            "\n// 外部修改\n"
        };
        fixture.write(relative, &format!("{old}{suffix}"));
        let disk = fixture.bytes();
        assert!(
            project
                .apply_source_lifecycle_plan_classified(&plan)
                .is_err(),
            "外部修改 {relative} 必须拒绝"
        );
        assert_eq!(project.sources(), buffers);
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(fixture.bytes(), disk);
    }
}

#[test]
fn authoring_capability_edit_after_preview_preserves_the_new_manifest_and_old_sources() {
    let fixture = Fixture::simple();
    let mut project = fixture.project();
    let plan = fixture.plan();
    let path = fixture.root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    manifest["source_config"]["active"] = serde_json::json!(["world.wl", SOURCE]);
    manifest["source_config"]["archived"] = serde_json::json!(["archive.wl", TARGET]);
    project
        .set_authoring_document(&path, manifest.to_string().into_bytes())
        .unwrap();
    let baseline = project.content_baseline();
    let buffers = project.sources();
    assert!(project.apply_source_lifecycle_plan(&plan).is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.sources(), buffers);
    assert_eq!(
        project.authoring_document(&path).unwrap().bytes(),
        manifest.to_string().as_bytes()
    );
}

#[test]
fn cancellation_during_preparation_and_immediately_before_commit_is_zero_write() {
    let fixture = Fixture::simple();
    let mut project = fixture.project();
    let request = fixture.request(TARGET);
    let baseline = project.content_baseline();
    let disk = fixture.bytes();
    assert!(project
        .preview_source_lifecycle_cancellable(&request, || true)
        .is_err());
    let mut preparation_checks = 0;
    let plan = project
        .preview_source_lifecycle_cancellable(&request, || {
            preparation_checks += 1;
            false
        })
        .unwrap();
    assert!(preparation_checks >= 2);
    for stop_at in [1, preparation_checks, preparation_checks + 1] {
        let mut checks = 0;
        assert!(
            project
                .apply_source_lifecycle_cancellable(&request, &plan.plan_digest, || {
                    checks += 1;
                    checks >= stop_at
                })
                .is_err(),
            "取消检查 {stop_at} 必须中止提交"
        );
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(fixture.bytes(), disk);
    }
}

#[test]
fn every_public_plan_mutation_and_digest_forgery_is_rejected_before_writes() {
    let fixture = Fixture::full();
    let mut project = fixture.project();
    let plan = fixture.plan();
    let baseline = project.content_baseline();
    let disk = fixture.bytes();
    let mut variants = Vec::new();
    macro_rules! tamper {
        ($candidate:ident, $change:expr) => {{
            let mut $candidate = plan.clone();
            $change;
            variants.push($candidate);
        }};
    }
    tamper!(p, p.plan_digest.push('x'));
    tamper!(p, p.content_baseline.push('x'));
    tamper!(p, p.request = fixture.request("world.wl"));
    tamper!(p, p.source_path = Some(fixture.root.join(TARGET)));
    tamper!(p, p.destination_path = Some(fixture.root.join(SOURCE)));
    tamper!(p, p.membership = "archived".into());
    tamper!(p, p.runtime_fingerprint_before ^= 1);
    tamper!(p, p.runtime_fingerprint_after ^= 1);
    tamper!(p, p.entry_after = "forged".into());
    tamper!(p, p.load_order_after.reverse());
    tamper!(p, p.resources.clear());
    tamper!(p, p.changes.pop());
    tamper!(p, p.changes[0].path = fixture.root.join("world.wl"));
    tamper!(p, p.changes[0].occurrences[0].before_range.end += 1);
    tamper!(p, p.changes[0].occurrences[0].after_range.end += 1);
    tamper!(p, p.changes[0].occurrences[0].before_token.push('🙂'));
    tamper!(p, p.changes[0].occurrences[0].after_token.push('🙂'));
    tamper!(p, p.changes[0].occurrences[0].before_context.push('🙂'));
    tamper!(p, p.changes[0].occurrences[0].after_context.push('🙂'));
    for changed in variants {
        assert!(project.apply_source_lifecycle_plan(&changed).is_err());
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(fixture.bytes(), disk);
    }
    assert_eq!(
        project
            .apply_source_lifecycle_classified(&fixture.request(TARGET), "forged")
            .unwrap_err()
            .kind,
        Kind::SourceChanged
    );
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn refresh_invalidates_the_prior_plan_even_when_a_changed_file_is_restored() {
    let fixture = Fixture::simple();
    let mut project = fixture.project();
    let plan = fixture.plan();
    fixture.write(TARGET, "// 外部版本\n");
    project.refresh().unwrap();
    fixture.write(TARGET, "// 活动目标\n");
    project.refresh().unwrap();
    let baseline = project.content_baseline();
    let disk = fixture.bytes();
    assert!(
        project.apply_source_lifecycle_plan(&plan).is_err(),
        "刷新代次变化必须使旧计划过期"
    );
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(fixture.bytes(), disk);
}

#[test]
fn a_block_comment_crossing_the_entity_tail_is_rejected_as_ambiguous() {
    let fixture = Fixture::new(
        concat!(
            "entity north_lighthouse kind place\n  property height = 38\n",
            "  /* 注释从实体缩进内开始\n顶层结束，无法证明归属 */\n",
            "entity neighbor kind place\n",
        ),
        "// 目标\n",
    );
    let mut project = fixture.project();
    assert!(!project.compile().has_errors());
    let baseline = project.content_baseline();
    let disk = fixture.bytes();
    let failure = project
        .preview_source_lifecycle_classified(&fixture.request(TARGET))
        .unwrap_err();
    assert_eq!(failure.kind, Kind::UnableToProve);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(fixture.bytes(), disk);
}

#[test]
fn registered_unknown_authoring_format_blocks_a_move_without_touching_raw_bytes() {
    let fixture = Fixture::simple();
    let path = fixture.root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["maps"] = serde_json::json!({"overview":".world/maps/overview.json"});
    std::fs::write(path, manifest.to_string()).unwrap();
    fixture.write(
        ".world/maps/overview.json",
        "{\"schema_version\":999,\"extension\":\"必须按原字节保留🙂\"}\n",
    );
    let project = fixture.project();
    let baseline = project.content_baseline();
    let disk = fixture.bytes();
    for target in [SOURCE, TARGET] {
        assert!(project
            .preview_source_lifecycle(&fixture.request(target))
            .is_err());
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(fixture.bytes(), disk);
    }
}

#[test]
fn modifying_an_existing_unregistered_file_invalidates_the_preview_inventory() {
    let fixture = Fixture::simple();
    fixture.write("notes.md", "版本 A\n");
    let mut project = fixture.project();
    let plan = fixture.plan();
    let baseline = project.content_baseline();
    let buffers = project.sources();
    fixture.write("notes.md", "版本 B\n");
    let disk = fixture.bytes();
    assert!(project.apply_source_lifecycle_plan(&plan).is_err());
    assert!(project
        .apply_source_lifecycle(&fixture.request(TARGET), &plan.plan_digest)
        .is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.sources(), buffers);
    assert_eq!(fixture.bytes(), disk);
}

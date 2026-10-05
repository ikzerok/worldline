//! 单实体移源的原字节、完整静态身份、运行指纹和快照回归。
#[path = "support/entity_source_move_fixture.rs"]
mod fixture;
use fixture::*;
use serde_json::Value;
use std::collections::BTreeMap;
use worldline_core::catalog::Catalog;
use worldline_core::project::Project;
use worldline_core::source_lifecycle::SourceLifecyclePlan;

fn semantic_entities(catalog: &Catalog) -> Value {
    let mut entities = serde_json::to_value(&catalog.entities).unwrap();
    for value in entities.as_object_mut().unwrap().values_mut() {
        let object = value.as_object_mut().unwrap();
        object.remove("file");
        object.remove("line");
    }
    entities
}

fn reference_multiset(catalog: &Catalog) -> BTreeMap<String, usize> {
    let mut result = BTreeMap::new();
    for reference in &catalog.references {
        let key = serde_json::to_string(&(&reference.source, &reference.target, &reference.kind))
            .unwrap();
        *result.entry(key).or_default() += 1;
    }
    result
}

fn verify_projection(before: &Project, after: &Project, plan: &SourceLifecyclePlan) {
    assert_eq!(plan.changes.len(), 2, "仅源和目标两个缓冲改变");
    for change in &plan.changes {
        assert_eq!(change.path, change.after_path);
        assert_eq!(change.kind, "source");
        let original = before.document(&change.path).unwrap();
        let candidate = after.document(&change.after_path).unwrap();
        let mut projected = original.as_bytes().to_vec();
        assert!(!change.occurrences.is_empty());
        for occurrence in change.occurrences.iter().rev() {
            let old = &occurrence.before_range;
            let new = &occurrence.after_range;
            assert_eq!(&original[old.start..old.end], occurrence.before_token);
            assert_eq!(&candidate[new.start..new.end], occurrence.after_token);
            assert!(occurrence.before_context.contains(&occurrence.before_token));
            assert!(occurrence.after_context.contains(&occurrence.after_token));
            assert!(original.contains(&occurrence.before_context));
            assert!(candidate.contains(&occurrence.after_context));
            assert_eq!(
                occurrence.line,
                original[..old.start]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count() as u32
                    + 1
            );
            projected.splice(old.start..old.end, occurrence.after_token.bytes());
        }
        assert_eq!(
            projected,
            candidate.as_bytes(),
            "逐处投影应重建准确候选字节"
        );
    }
}

#[test]
fn complete_entity_moves_exact_utf8_crlf_without_neighbor_comments_or_blank_lines() {
    let fixture = Fixture::full();
    let disk = fixture.bytes();
    let mut project = fixture.project();
    let before = project.compile();
    assert!(!before.has_errors(), "{:?}", before.diagnostics);
    let original = project.clone();
    let plan = project
        .preview_source_lifecycle(&fixture.request(TARGET))
        .unwrap();
    assert_eq!(fixture.bytes(), disk);
    assert_eq!(project.content_baseline(), original.content_baseline());
    assert_eq!(plan.source_path, Some(fixture.root.join(SOURCE)));
    assert_eq!(plan.destination_path, Some(fixture.root.join(TARGET)));
    assert_eq!(plan.membership, "active");
    assert_eq!(
        plan.runtime_fingerprint_before,
        plan.runtime_fingerprint_after
    );
    assert_eq!(plan.load_order_before, plan.load_order_after);
    assert_eq!(plan.entry_before, plan.entry_after);
    project.apply_source_lifecycle_plan(&plan).unwrap();
    assert_eq!(
        project.document(&fixture.root.join(SOURCE)).unwrap(),
        format!("{PREFIX}{SUFFIX}")
    );
    let target = project.document(&fixture.root.join(TARGET)).unwrap();
    assert!(target.starts_with(TARGET_TEXT));
    assert!(target.ends_with(DECLARATION));
    let separator = &target[TARGET_TEXT.len()..target.len() - DECLARATION.len()];
    assert!(!separator.is_empty() && separator.contains('\n'));
    assert!(separator.bytes().all(|byte| matches!(byte, b'\r' | b'\n')));
    let removed = plan
        .changes
        .iter()
        .find(|change| change.path == fixture.root.join(SOURCE))
        .unwrap();
    assert_eq!(removed.occurrences.len(), 1);
    assert_eq!(removed.occurrences[0].before_token, DECLARATION);
    assert_eq!(removed.occurrences[0].before_range.start, PREFIX.len());
    assert_eq!(
        removed.occurrences[0].before_range.end,
        PREFIX.len() + DECLARATION.len()
    );
    assert!(removed.occurrences[0].after_token.is_empty());
    verify_projection(&original, &project, &plan);
    assert_eq!(project.document(&project.entry).unwrap(), WORLD);
    assert_eq!(fixture.bytes(), disk, "应用不得隐式保存");
    assert!(project.is_dirty());
    let after = project.compile();
    assert!(!after.has_errors(), "{:?}", after.diagnostics);
    assert_eq!(
        semantic_entities(&before.analysis.catalog),
        semantic_entities(&after.analysis.catalog)
    );
    assert_eq!(
        reference_multiset(&before.analysis.catalog),
        reference_multiset(&after.analysis.catalog)
    );
    assert!(
        reference_multiset(&before.analysis.catalog)
            .values()
            .any(|count| *count >= 2),
        "重复正文链接不得去重"
    );
    let before_catalog = serde_json::to_value(&before.analysis.catalog).unwrap();
    let after_catalog = serde_json::to_value(&after.analysis.catalog).unwrap();
    for field in [
        "aliases",
        "text_links",
        "anchors",
        "states",
        "tags",
        "assets",
        "marks",
        "attachments",
        "relation_types",
        "relations",
        "relation_index",
    ] {
        assert_eq!(
            before_catalog[field], after_catalog[field],
            "正式资料字段 {field} 必须保持"
        );
    }
    assert_eq!(before.program.schemas, after.program.schemas);
    assert_eq!(
        before.program.schema_bindings,
        after.program.schema_bindings
    );
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    assert_eq!(after.analysis.catalog.states["lamp"].target.id, ID);
    assert_eq!(
        std::path::PathBuf::from(&after.analysis.catalog.entities[ID].file),
        fixture.root.join(TARGET)
    );
    assert_eq!(
        std::path::PathBuf::from(&after.analysis.catalog.entities["neighboring"].file),
        fixture.root.join(SOURCE)
    );
    assert_eq!(project.language_version(), "1.13");
}

#[test]
fn one_snapshot_undo_redo_save_reopen_and_export_keep_unreferenced_files() {
    let fixture = Fixture::full();
    let disk = fixture.bytes();
    let mut project = fixture.project();
    let original = project.clone();
    let before_fingerprint = project.compile().analysis.fingerprint;
    let plan = project
        .preview_source_lifecycle(&fixture.request(TARGET))
        .unwrap();
    project.apply_source_lifecycle_plan(&plan).unwrap();
    let changed = project.clone();
    assert!(project.restore(original.clone()));
    assert_eq!(project.content_baseline(), original.content_baseline());
    assert!(!project.is_dirty());
    assert!(project.restore(changed.clone()));
    assert_eq!(project.content_baseline(), changed.content_baseline());
    project.save().unwrap();
    assert!(!project.is_dirty());
    assert!(fixture.root.join(SOURCE).is_file());
    let mut reopened = fixture.project();
    assert_eq!(reopened.content_baseline(), project.content_baseline());
    assert_eq!(reopened.compile().analysis.fingerprint, before_fingerprint);
    assert_eq!(
        std::path::PathBuf::from(&reopened.compile().analysis.catalog.entities[ID].file),
        fixture.root.join(TARGET)
    );
    let exported = reopened.export_files().unwrap();
    for path in [
        ".hidden/unused.bin",
        "inactive.wl",
        "archive.wl",
        "assets/picture.txt",
        ".world/project.json",
    ] {
        let bytes = exported.get(std::path::Path::new(path)).unwrap_or_else(|| {
            panic!("完整导出缺少原有文件：{path}；已导出={:?}", exported.keys())
        });
        assert_eq!(bytes, &disk[std::path::Path::new(path)]);
    }
    assert!(project.restore(original));
    assert!(project.is_dirty(), "已保存后撤销应相对新磁盘基线变脏");
    project.save().unwrap();
    assert_eq!(fixture.bytes(), disk);
}

#[test]
fn no_terminal_newline_long_unicode_line_and_blank_target_keep_original_bytes() {
    let declaration = format!(
        "entity {ID} kind place as \"长🙂实体\"\n  property note = \"{}\"",
        "长中文🙂".repeat(2000)
    );
    for target in ["", "// 无换行目标🙂", "// CRLF 目标\r\n\r\n"] {
        let fixture = Fixture::new(&declaration, target);
        let mut project = fixture.project();
        let original = project.clone();
        let plan = fixture.plan();
        project.apply_source_lifecycle_plan(&plan).unwrap();
        assert_eq!(project.document(&fixture.root.join(SOURCE)).unwrap(), "");
        let changed = project.document(&fixture.root.join(TARGET)).unwrap();
        assert!(changed.starts_with(target));
        assert!(changed.ends_with(&declaration));
        verify_projection(&original, &project, &plan);
        assert!(!project.compile().has_errors());
    }
}

#[test]
fn same_source_is_validated_no_change_and_multiple_active_targets_are_legal() {
    let fixture = Fixture::simple();
    let mut project = fixture.project();
    let baseline = project.content_baseline();
    let disk = fixture.bytes();
    let plan = project
        .preview_source_lifecycle(&fixture.request(SOURCE))
        .unwrap();
    assert!(plan.changes.is_empty());
    assert_eq!(plan.source_path, plan.destination_path);
    project.apply_source_lifecycle_plan(&plan).unwrap();
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.is_dirty());
    assert_eq!(fixture.bytes(), disk);
    for target in [TARGET, "world.wl"] {
        let mut candidate = project.clone();
        let plan = candidate
            .preview_source_lifecycle(&fixture.request(target))
            .unwrap();
        candidate.apply_source_lifecycle_plan(&plan).unwrap();
        assert_eq!(
            std::path::PathBuf::from(&candidate.compile().analysis.catalog.entities[ID].file),
            fixture.root.join(target)
        );
    }
}

#[test]
fn preview_is_identical_across_one_hundred_recompilations_and_reopens() {
    let fixture = Fixture::full();
    let mut project = fixture.project();
    let plan = fixture.plan();
    for _ in 0..100 {
        assert!(!project.compile().has_errors());
        assert_eq!(
            project
                .preview_source_lifecycle(&fixture.request(TARGET))
                .unwrap(),
            plan
        );
        let reopened = fixture.project();
        assert_eq!(
            reopened
                .preview_source_lifecycle(&fixture.request(TARGET))
                .unwrap(),
            plan
        );
    }
}

#[test]
fn entity_declared_in_the_entry_can_move_without_relocating_the_entry_file() {
    let fixture = Fixture::simple();
    fixture.write(SOURCE, "// 原有活动源码\n");
    fixture.write("world.wl", &format!(
        "include \"{SOURCE}\"\ninclude \"{TARGET}\"\nentity {ID} kind place\nevent start\n  -> END\n"
    ));
    let mut project = fixture.project();
    let entry = project.entry.clone();
    let before = project.compile();
    assert!(!before.has_errors());
    let plan = fixture.plan();
    assert_eq!(plan.source_path.as_ref(), Some(&entry));
    project.apply_source_lifecycle_plan(&plan).unwrap();
    let after = project.compile();
    assert_eq!(project.entry, entry);
    assert_eq!(before.program.files, after.program.files);
    assert_eq!(before.program.entry, after.program.entry);
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    assert_eq!(
        std::path::PathBuf::from(&after.analysis.catalog.entities[ID].file),
        fixture.root.join(TARGET)
    );
}

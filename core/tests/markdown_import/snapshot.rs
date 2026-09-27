use super::fixture::Fixture;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use worldline_core::markdown_import::{MarkdownImportOptions, MarkdownImportSourceSnapshot};
use worldline_core::workspace_snapshot::Files;

#[test]
fn files_snapshot_preview_matches_native_plan_without_mutating_project_or_inputs() {
    let fixture = Fixture::new();
    let project = fixture.project();
    let request = fixture.request(&project);
    let options = MarkdownImportOptions::from(&request);
    let files = fixture.source_files();
    let source = MarkdownImportSourceSnapshot {
        label: "browser-selected-folder",
        files: &files,
    };
    let target_before = worldline_core::workspace_snapshot::snapshot_files(&project).unwrap();

    let native_plan = project.preview_markdown_import(&request).unwrap();
    let snapshot_plan = project
        .preview_markdown_import_snapshot(&source, &options)
        .unwrap();

    assert_eq!(snapshot_plan.plan_digest, native_plan.plan_digest);
    assert_eq!(
        snapshot_plan.source_fingerprint,
        native_plan.source_fingerprint
    );
    assert_eq!(snapshot_plan.pages, native_plan.pages);
    assert_eq!(snapshot_plan.links, native_plan.links);
    assert_eq!(snapshot_plan.attachments, native_plan.attachments);
    assert_eq!(snapshot_plan.losses, native_plan.losses);
    assert_eq!(snapshot_plan.conflicts, native_plan.conflicts);
    assert_eq!(project.content_baseline(), request.expected_baseline);
    assert_eq!(
        worldline_core::workspace_snapshot::snapshot_files(&project).unwrap(),
        target_before
    );
    assert_eq!(files, fixture.source_files());
    assert!(!project.is_dirty());
    assert!(!fixture
        .root
        .join("project/.world/markdown-imports")
        .exists());
}

#[test]
fn files_snapshot_apply_returns_complete_candidate_and_preserves_confirmation_boundaries() {
    let fixture = Fixture::new();
    let project = fixture.project();
    let request = fixture.request(&project);
    let mut options = MarkdownImportOptions::from(&request);
    let files = fixture.source_files();
    let source = MarkdownImportSourceSnapshot {
        label: "browser-selected-folder",
        files: &files,
    };
    let target_before = worldline_core::workspace_snapshot::snapshot_files(&project).unwrap();
    let plan = project
        .preview_markdown_import_snapshot(&source, &options)
        .unwrap();

    let unconfirmed = project.apply_markdown_import_snapshot(&source, &options, &plan.plan_digest);
    assert!(unconfirmed.unwrap_err().contains("缺少损失确认"));
    assert_eq!(project.content_baseline(), request.expected_baseline);
    assert_eq!(
        worldline_core::workspace_snapshot::snapshot_files(&project).unwrap(),
        target_before
    );

    options.accept_losses = true;
    options.allow_language_upgrade = true;
    let applied = project
        .apply_markdown_import_snapshot(&source, &options, &plan.plan_digest)
        .unwrap();
    assert_eq!(applied.result.plan.plan_digest, plan.plan_digest);
    assert_eq!(applied.result.new_baseline, plan.new_baseline);
    assert_eq!(applied.result.baseline, request.expected_baseline);
    assert_eq!(project.content_baseline(), request.expected_baseline);
    let source_copy = PathBuf::from(format!(
        ".world/markdown-imports/{}/sources/harbor.md",
        plan.namespace
    ));
    assert_eq!(
        applied.workspace_files[&source_copy],
        files[&PathBuf::from("harbor.md")]
    );
    let generated = PathBuf::from(format!(
        ".world/markdown-imports/{}/import.wl",
        plan.namespace
    ));
    assert!(
        String::from_utf8_lossy(&applied.workspace_files[&generated])
            .contains("entity harbor kind place")
    );
    let attachment = PathBuf::from(&plan.attachments[0].output_path);
    assert_eq!(applied.workspace_files[&attachment], [0, 1, 255]);
    assert!(!fixture
        .root
        .join("project/.world/markdown-imports")
        .exists());
}

#[test]
fn files_snapshot_rejects_unsafe_colliding_and_over_budget_paths() {
    let fixture = Fixture::new();
    let project = fixture.project();
    let options = MarkdownImportOptions {
        expected_baseline: project.content_baseline(),
        id_overrides: BTreeMap::new(),
        namespace: None,
        accept_losses: true,
        allow_language_upgrade: true,
    };

    let unsafe_files = Files::from([(PathBuf::from("../escape.md"), b"# Escape".to_vec())]);
    let unsafe_source = MarkdownImportSourceSnapshot {
        label: "unsafe",
        files: &unsafe_files,
    };
    assert!(project
        .preview_markdown_import_snapshot(&unsafe_source, &options)
        .unwrap_err()
        .contains("路径"));

    let case_collision = Files::from([
        (PathBuf::from("Page.md"), b"# One".to_vec()),
        (PathBuf::from("page.md"), b"# Two".to_vec()),
    ]);
    let collision_source = MarkdownImportSourceSnapshot {
        label: "collision",
        files: &case_collision,
    };
    assert!(project
        .preview_markdown_import_snapshot(&collision_source, &options)
        .unwrap_err()
        .contains("大小写"));

    let too_many = (0..=worldline_core::markdown_import::MAX_IMPORT_FILES)
        .map(|index| (PathBuf::from(format!("file-{index}.bin")), Vec::new()))
        .collect::<Files>();
    let over_budget_source = MarkdownImportSourceSnapshot {
        label: "over-budget",
        files: &too_many,
    };
    assert!(project
        .preview_markdown_import_snapshot(&over_budget_source, &options)
        .unwrap_err()
        .contains("文件数超过"));

    let too_long = Files::from([(
        PathBuf::from(format!(
            "{}/{}/{}/page.md",
            "a".repeat(180),
            "b".repeat(180),
            "c".repeat(180)
        )),
        b"# Page".to_vec(),
    )]);
    let too_long_source = MarkdownImportSourceSnapshot {
        label: "too-long",
        files: &too_long,
    };
    assert!(project
        .preview_markdown_import_snapshot(&too_long_source, &options)
        .unwrap_err()
        .contains("过长"));

    let too_many_entries = (0..worldline_core::markdown_import::MAX_IMPORT_FILES)
        .map(|index| {
            (
                PathBuf::from(format!("d{index}/a{index}/b{index}/c{index}/page.md")),
                Vec::new(),
            )
        })
        .collect::<Files>();
    let too_many_entries_source = MarkdownImportSourceSnapshot {
        label: "too-many-entries",
        files: &too_many_entries,
    };
    assert!(project
        .preview_markdown_import_snapshot(&too_many_entries_source, &options)
        .unwrap_err()
        .contains("目录项超过"));
}

#[test]
fn files_snapshot_apply_rechecks_source_and_keeps_language_upgrade_confirmation_separate() {
    let fixture = Fixture::new();
    let manifest_path = fixture.root.join("project/.world/project.json");
    fs::write(
        &manifest_path,
        r#"{"schema_version":1,"language_version":"1.9","required_features":[],"extension":{"keep":true}}"#,
    )
    .unwrap();
    let project = fixture.project();
    let baseline = project.content_baseline();
    let request = fixture.request(&project);
    let mut options = MarkdownImportOptions::from(&request);
    options.accept_losses = true;
    let mut files = fixture.source_files();
    let source = MarkdownImportSourceSnapshot {
        label: "browser-selected-folder",
        files: &files,
    };
    let plan = project
        .preview_markdown_import_snapshot(&source, &options)
        .unwrap();
    assert!(plan.requires_language_upgrade);

    files.insert(
        PathBuf::from("coast.md"),
        "# 海岸\n\n## Shore\n来源在预览后变化。\n"
            .as_bytes()
            .to_vec(),
    );
    let changed_source = MarkdownImportSourceSnapshot {
        label: "browser-selected-folder",
        files: &files,
    };
    assert!(project
        .apply_markdown_import_snapshot(&changed_source, &options, &plan.plan_digest)
        .unwrap_err()
        .contains("预览已过期"));

    files.insert(
        PathBuf::from("coast.md"),
        fs::read(fixture.source.join("coast.md")).unwrap(),
    );
    let source = MarkdownImportSourceSnapshot {
        label: "browser-selected-folder",
        files: &files,
    };
    assert!(project
        .apply_markdown_import_snapshot(&source, &options, &plan.plan_digest)
        .unwrap_err()
        .contains("缺少语言升级确认"));
    assert_eq!(project.content_baseline(), baseline);

    options.allow_language_upgrade = true;
    let applied = project
        .apply_markdown_import_snapshot(&source, &options, &plan.plan_digest)
        .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&applied.workspace_files[&PathBuf::from(".world/project.json")])
            .unwrap();
    assert_eq!(manifest["language_version"], "1.10");
    assert_eq!(manifest["extension"]["keep"], true);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(manifest_path).unwrap()).unwrap()
            ["language_version"],
        "1.9"
    );
}

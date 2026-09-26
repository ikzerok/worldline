use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::markdown_import::{
    MarkdownImportOptions, MarkdownImportRequest, MarkdownImportSourceSnapshot,
};
use worldline_core::project::Project;
use worldline_core::workspace_snapshot::Files;

struct Fixture {
    root: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "worldline-markdown-import-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let source = root.join("external");
        fs::create_dir_all(root.join("project/.world")).unwrap();
        fs::create_dir_all(source.join("media")).unwrap();
        fs::write(root.join("project/world.wl"), "event start\n  -> END\n").unwrap();
        fs::write(
            root.join("project/.world/project.json"),
            r#"{"schema_version":1,"language_version":"1.10","required_features":[],"extension":{"keep":true}}"#,
        )
        .unwrap();
        fs::write(
            source.join("harbor.md"),
            "---\nid: harbor\ntitle: 雾港\nkind: place\n---\n# 雾港\n灯塔与[[原样保留]]海面。\n\n[岸线](coast.md#shore)\n\n![地图](media/map.bin)\n\n```js\nrun()\n```\n",
        )
        .unwrap();
        fs::write(source.join("coast.md"), "# 海岸\n\n## Shore\n海浪拍岸。\n").unwrap();
        fs::write(source.join("media/map.bin"), [0, 1, 255]).unwrap();
        Self { root, source }
    }

    fn project(&self) -> Project {
        Project::open(&self.root.join("project")).unwrap()
    }

    fn request(&self, project: &Project) -> MarkdownImportRequest {
        MarkdownImportRequest {
            source_root: self.source.clone(),
            expected_baseline: project.content_baseline(),
            id_overrides: BTreeMap::new(),
            namespace: None,
            accept_losses: false,
            allow_language_upgrade: false,
        }
    }

    fn source_files(&self) -> Files {
        Files::from([
            (
                PathBuf::from("harbor.md"),
                fs::read(self.source.join("harbor.md")).unwrap(),
            ),
            (
                PathBuf::from("coast.md"),
                fs::read(self.source.join("coast.md")).unwrap(),
            ),
            (
                PathBuf::from("media/map.bin"),
                fs::read(self.source.join("media/map.bin")).unwrap(),
            ),
        ])
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn markdown_preview_maps_pages_links_and_attachments_without_writing() {
    let fixture = Fixture::new();
    let project = fixture.project();
    let baseline = project.content_baseline();
    let original = fs::read(fixture.source.join("harbor.md")).unwrap();
    let request = fixture.request(&project);

    let plan = project.preview_markdown_import(&request).unwrap();

    assert_eq!(plan.baseline, baseline);
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.is_dirty());
    assert!(!fixture
        .root
        .join("project/.world/markdown-imports")
        .exists());
    assert_eq!(
        fs::read(fixture.source.join("harbor.md")).unwrap(),
        original
    );
    assert_eq!(plan.pages.len(), 2);
    assert!(plan
        .pages
        .iter()
        .find(|page| page.source == "coast.md")
        .unwrap()
        .id
        .starts_with("md_"));
    assert_eq!(plan.links.len(), 1);
    assert_eq!(plan.links[0].target.kind, "anchor");
    assert_eq!(plan.attachments.len(), 1);
    assert!(plan
        .losses
        .iter()
        .any(|loss| loss.code == "UNSUPPORTED_CODE_BLOCK"));
    assert!(!plan.can_apply);
}

#[test]
fn markdown_ids_are_stable_and_conflicts_require_an_explicit_mapping() {
    let fixture = Fixture::new();
    let mut project = fixture.project();
    let coast = |plan: &worldline_core::markdown_import::MarkdownImportPlan| {
        plan.pages
            .iter()
            .find(|page| page.source == "coast.md")
            .unwrap()
            .id
            .clone()
    };
    let request = fixture.request(&project);
    let first = project.preview_markdown_import(&request).unwrap();
    let first_id = coast(&first);
    fs::write(
        fixture.source.join("coast.md"),
        "# 海岸\n\n## Shore\n新正文。\n",
    )
    .unwrap();
    let changed_source = project.preview_markdown_import(&request).unwrap();
    assert_eq!(coast(&changed_source), first_id);

    project
        .set_text(
            &project.entry.clone(),
            "entity harbor kind place as \"旧港\"\nevent start\n  -> END\n".into(),
        )
        .unwrap();
    let request = fixture.request(&project);
    let conflicted = project.preview_markdown_import(&request).unwrap();
    let id_conflict = conflicted
        .conflicts
        .iter()
        .find(|conflict| conflict.code == "ENTITY_ID_CONFLICT")
        .unwrap();
    assert_eq!(id_conflict.source.as_deref(), Some("harbor.md"));
    assert!(id_conflict
        .candidates
        .contains(&"harbor_import_2".to_string()));
    assert!(!conflicted.can_apply);

    let mut mapped_request = request;
    mapped_request
        .id_overrides
        .insert("harbor.md".into(), "harbor_notes".into());
    mapped_request.accept_losses = true;
    let mapped = project.preview_markdown_import(&mapped_request).unwrap();
    assert!(!mapped
        .conflicts
        .iter()
        .any(|conflict| conflict.source.as_deref() == Some("harbor.md")
            && conflict.code == "ENTITY_ID_CONFLICT"));
    assert!(mapped
        .pages
        .iter()
        .any(|page| page.source == "harbor.md" && page.id == "harbor_notes"));
    assert!(mapped.can_apply);
}

#[test]
fn markdown_apply_compiles_and_saves_the_whole_candidate_with_raw_sources() {
    let fixture = Fixture::new();
    let mut project = fixture.project();
    let original = fs::read(fixture.source.join("harbor.md")).unwrap();
    let mut request = fixture.request(&project);
    request.accept_losses = true;

    let plan = project.preview_markdown_import(&request).unwrap();
    assert!(plan.can_apply);
    assert!(plan.files.iter().any(|file| file.path == "world.wl"));
    assert!(plan
        .files
        .iter()
        .any(|file| file.kind == "generated_source"));
    assert!(plan.files.iter().any(|file| file.kind == "source_copy"));
    assert!(plan
        .files
        .iter()
        .any(|file| file.kind == "manifest_preserving_update"));

    let result = project
        .apply_markdown_import(&request, &plan.plan_digest)
        .unwrap();

    assert_eq!(result.new_baseline, result.plan.new_baseline);
    assert_eq!(project.content_baseline(), result.new_baseline);
    assert!(!project.is_dirty());
    assert_eq!(
        fs::read(fixture.source.join("harbor.md")).unwrap(),
        original
    );
    let imported = fixture.root.join("project").join(format!(
        ".world/markdown-imports/{}/sources/harbor.md",
        plan.namespace
    ));
    assert_eq!(fs::read(imported).unwrap(), original);
    let copied_asset = fixture
        .root
        .join("project")
        .join(&plan.attachments[0].output_path);
    assert_eq!(fs::read(copied_asset).unwrap(), [0, 1, 255]);
    assert!(result.changed_files.contains(&project.entry));

    let compiled = project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert!(compiled.analysis.catalog.entities.contains_key("harbor"));
    assert!(compiled
        .analysis
        .catalog
        .relations
        .contains_key(&plan.links[0].relation_id));
    assert!(compiled.analysis.catalog.assets[&plan.attachments[0].id].available);
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.root.join("project/.world/project.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["language_version"], "1.10");
    assert_eq!(manifest["extension"]["keep"], true);
}

#[test]
fn markdown_preview_tracks_crlf_lines_and_percent_encoded_relative_paths() {
    let fixture = Fixture::new();
    let project = fixture.project();
    let harbor = b"---\r\nid: harbor\r\ntitle: \xe9\x9b\xbe\xe6\xb8\xaf\r\nkind: place\r\n---\r\n# \xe9\x9b\xbe\xe6\xb8\xaf\r\n[\xe5\xb2\xb8\xe7\xba\xbf](coast%20line.md#wind%20tower)\r\n";
    fs::write(fixture.source.join("harbor.md"), harbor).unwrap();
    fs::remove_file(fixture.source.join("coast.md")).unwrap();
    fs::write(
        fixture.source.join("coast line.md"),
        "# 海岸\r\n\r\n## Wind Tower\r\n潮汐\r\n",
    )
    .unwrap();
    let request = fixture.request(&project);

    let plan = project.preview_markdown_import(&request).unwrap();

    assert_eq!(plan.links.len(), 1);
    assert_eq!(plan.links[0].line, 7);
    assert_eq!(plan.links[0].target.kind, "anchor");
    assert_eq!(plan.links[0].label, "岸线");
    let raw_copy = fixture.root.join("project").join(format!(
        ".world/markdown-imports/{}/sources/harbor.md",
        plan.namespace
    ));
    assert!(
        !raw_copy.exists(),
        "preview must not create the source copy"
    );
    assert_eq!(fs::read(fixture.source.join("harbor.md")).unwrap(), harbor);
}

#[test]
fn markdown_apply_rejects_a_changed_source_without_writing_any_candidate_files() {
    let fixture = Fixture::new();
    let mut project = fixture.project();
    let mut request = fixture.request(&project);
    request.accept_losses = true;
    let plan = project.preview_markdown_import(&request).unwrap();
    let project_baseline = project.content_baseline();
    fs::write(
        fixture.source.join("coast.md"),
        "# 海岸\n\n## Shore\n潮汐拍岸。\n",
    )
    .unwrap();

    let error = project
        .apply_markdown_import(&request, &plan.plan_digest)
        .unwrap_err();

    assert!(error.contains("过期"));
    assert_eq!(project.content_baseline(), project_baseline);
    assert!(!fixture
        .root
        .join(format!(
            "project/.world/markdown-imports/{}",
            plan.namespace
        ))
        .exists());
}

#[test]
fn markdown_reference_budget_stops_before_candidate_compilation_or_writes() {
    let fixture = Fixture::new();
    let project = fixture.project();
    fs::remove_file(fixture.source.join("coast.md")).unwrap();
    let mut markdown = String::from("# H\n");
    for _ in 0..=worldline_core::markdown_import::MAX_IMPORT_REFERENCES {
        markdown.push_str("[x](#h)\n");
    }
    fs::write(fixture.source.join("harbor.md"), markdown).unwrap();
    let request = fixture.request(&project);

    let error = project.preview_markdown_import(&request).unwrap_err();

    assert!(error.contains("引用超过"));
    assert!(!fixture
        .root
        .join("project/.world/markdown-imports")
        .exists());
}

#[test]
fn markdown_links_in_lossy_lists_quotes_and_tables_still_appear_in_the_impact_plan() {
    let fixture = Fixture::new();
    let project = fixture.project();
    fs::write(
        fixture.source.join("harbor.md"),
        "# Harbor\n- [List link](coast.md)\n> [Quote link](coast.md)\n| [Table link](coast.md) |\n`[Not a link](coast.md)`\n",
    )
    .unwrap();
    let request = fixture.request(&project);

    let plan = project.preview_markdown_import(&request).unwrap();

    assert_eq!(plan.links.len(), 3);
    assert_eq!(
        plan.links
            .iter()
            .map(|link| link.label.as_str())
            .collect::<Vec<_>>(),
        ["List link", "Quote link", "Table link"]
    );
    assert!(plan
        .losses
        .iter()
        .any(|loss| loss.code == "UNSUPPORTED_LIST"));
    assert!(plan
        .losses
        .iter()
        .any(|loss| loss.code == "UNSUPPORTED_BLOCK_QUOTE"));
    assert!(plan
        .losses
        .iter()
        .any(|loss| loss.code == "UNSUPPORTED_TABLE"));
    assert!(plan
        .losses
        .iter()
        .any(|loss| loss.code == "UNSUPPORTED_INLINE_MARKUP"));
    assert!(plan
        .losses
        .iter()
        .filter(|loss| loss.code == "UNSUPPORTED_TABLE" || loss.code == "UNSUPPORTED_LIST")
        .all(|loss| loss.preserved_at.is_some()));
}

#[test]
fn markdown_apply_lists_and_saves_preexisting_dirty_project_documents() {
    let fixture = Fixture::new();
    let mut project = fixture.project();
    let entry = project.entry.clone();
    let current = project.document(&entry).unwrap().to_string();
    project
        .set_text(&entry, format!("{current}\n// keep this buffered edit\n"))
        .unwrap();
    let mut request = fixture.request(&project);
    request.accept_losses = true;
    let plan = project.preview_markdown_import(&request).unwrap();
    assert!(plan.files.iter().any(|file| file.path == "world.wl"));

    let result = project
        .apply_markdown_import(&request, &plan.plan_digest)
        .unwrap();

    assert!(result.changed_files.contains(&entry));
    let saved = fs::read_to_string(entry).unwrap();
    assert!(saved.contains("// keep this buffered edit"));
    assert!(saved.contains("markdown-imports"));
    assert!(!project.is_dirty());
}

#[test]
fn markdown_apply_rejects_a_stale_project_baseline_before_writing() {
    let fixture = Fixture::new();
    let mut project = fixture.project();
    let request = fixture.request(&project);
    let plan = project.preview_markdown_import(&request).unwrap();
    let entry = project.entry.clone();
    project
        .set_text(
            &entry,
            "event start\n  -> END\n\n// changed after preview\n".into(),
        )
        .unwrap();
    let baseline = project.content_baseline();

    let error = project
        .apply_markdown_import(&request, &plan.plan_digest)
        .unwrap_err();

    assert!(error.contains("基线已过期"));
    assert_eq!(project.content_baseline(), baseline);
    assert!(!fixture
        .root
        .join(format!(
            "project/.world/markdown-imports/{}",
            plan.namespace
        ))
        .exists());
}

#[test]
fn markdown_unreferenced_files_are_reported_deterministically_and_not_copied() {
    let fixture = Fixture::new();
    let project = fixture.project();
    fs::write(
        fixture.source.join("harbor.md"),
        "---\nid: harbor\ntitle: 雾港\nkind: place\n---\n# 雾港\n正文。\n",
    )
    .unwrap();
    let request = fixture.request(&project);

    let first = project.preview_markdown_import(&request).unwrap();
    let second = project.preview_markdown_import(&request).unwrap();

    assert_eq!(first.plan_digest, second.plan_digest);
    assert!(first
        .losses
        .iter()
        .any(|loss| { loss.code == "UNREFERENCED_SOURCE_FILE" && loss.source == "media/map.bin" }));
    assert!(!first
        .files
        .iter()
        .any(|file| file.source.as_deref() == Some("media/map.bin")));
}

#[test]
fn markdown_apply_acceptance_is_separate_from_the_reviewed_candidate_digest() {
    let fixture = Fixture::new();
    let mut project = fixture.project();
    let mut request = fixture.request(&project);
    let plan = project.preview_markdown_import(&request).unwrap();
    assert!(!plan.can_apply);

    request.accept_losses = true;
    request.allow_language_upgrade = true;
    let result = project
        .apply_markdown_import(&request, &plan.plan_digest)
        .unwrap();

    assert!(result.plan.can_apply);
    assert_eq!(result.plan.plan_digest, plan.plan_digest);
    assert_eq!(result.new_baseline, project.content_baseline());
}

#[test]
fn markdown_import_requires_explicit_language_upgrade_confirmation() {
    let fixture = Fixture::new();
    let manifest = fixture.root.join("project/.world/project.json");
    fs::write(
        &manifest,
        r#"{"schema_version":1,"language_version":"1.9","required_features":[]}"#,
    )
    .unwrap();
    let mut project = fixture.project();
    let mut request = fixture.request(&project);
    request.accept_losses = true;
    let plan = project.preview_markdown_import(&request).unwrap();
    assert!(plan.requires_language_upgrade);
    assert!(!plan.can_apply);

    let error = project
        .apply_markdown_import(&request, &plan.plan_digest)
        .unwrap_err();
    assert!(error.contains("缺少语言升级确认"));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&manifest).unwrap()).unwrap()
            ["language_version"],
        "1.9"
    );
    assert!(!fixture
        .root
        .join(format!(
            "project/.world/markdown-imports/{}",
            plan.namespace
        ))
        .exists());

    request.allow_language_upgrade = true;
    let result = project
        .apply_markdown_import(&request, &plan.plan_digest)
        .unwrap();
    assert!(result.plan.requires_language_upgrade);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&manifest).unwrap()).unwrap()
            ["language_version"],
        "1.10"
    );
}

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

#[test]
fn markdown_front_matter_closing_separator_is_not_a_horizontal_rule_loss() {
    let fixture = Fixture::new();
    fs::write(
        fixture.source.join("harbor.md"),
        "---\nid: harbor\ntitle: 雾港\nkind: place\n---\n# 雾港\n正文。\n",
    )
    .unwrap();
    fs::remove_file(fixture.source.join("media/map.bin")).unwrap();
    let project = fixture.project();
    let request = fixture.request(&project);

    let plan = project.preview_markdown_import(&request).unwrap();

    assert!(plan.losses.is_empty(), "{:?}", plan.losses);
    assert!(plan.can_apply);
}

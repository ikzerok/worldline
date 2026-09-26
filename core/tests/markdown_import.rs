use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::markdown_import::MarkdownImportRequest;
use worldline_core::project::Project;

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
    assert!(result
        .changed_files
        .contains(&fixture.root.join("project/world.wl")));

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

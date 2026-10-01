//! 语言能力启用的最小正式回归：显式选择、全文诊断与零损失事务边界。
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::authoring::EntityDraft;
use worldline_core::capabilities::{
    feature_capabilities, language_capabilities, CapabilityEnableRequest,
};
use worldline_core::project::Project;
use worldline_core::{LanguageVersion, Severity};

const SOURCE: &str = "event start\n  正文。\n  -> END\n";

struct Fixture {
    root: PathBuf,
    project: Project,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture(source: &str, manifest: Option<&str>) -> Fixture {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "wl-capabilities-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    if let Some(manifest) = manifest {
        fs::write(root.join(".world/project.json"), manifest).unwrap();
    }
    let project = Project::open(&root).unwrap();
    Fixture { root, project }
}

fn request(
    project: &Project,
    version: LanguageVersion,
    features: &[&str],
) -> CapabilityEnableRequest {
    CapabilityEnableRequest {
        target_language: version,
        enable_features: features.iter().map(|feature| feature.to_string()).collect(),
        expected_baseline: project.content_baseline(),
    }
}

#[test]
fn catalog_covers_supported_versions_and_only_existing_manifest_features() {
    assert_eq!(
        language_capabilities()
            .iter()
            .map(|item| item.version)
            .collect::<Vec<_>>(),
        LanguageVersion::SUPPORTED
    );
    assert!(language_capabilities()[0].required_features.is_empty());
    for capability in language_capabilities() {
        assert!(!capability.description.is_empty());
        for feature in capability.required_features {
            assert!(feature_capabilities()
                .iter()
                .any(|item| item.id == *feature));
            assert!(!feature.starts_with("runtime."));
        }
    }
    let character = feature_capabilities()
        .iter()
        .find(|item| item.id == "content.character_refs.v1")
        .unwrap();
    assert_eq!(character.minimum_language, LanguageVersion::V1_13);
    assert_eq!(character.dependencies, &["content.object_refs.v1"]);
}

#[test]
fn fresh_default_plan_is_read_only_and_apply_enables_entities_without_saving() {
    let fixture = fixture(SOURCE, None);
    let mut project = Project::new(&fixture.root.join("unsaved"));
    let entry = project.entry.clone();
    project.documents.retain(|path, _| path == &entry);
    project.set_text(&entry, SOURCE.into()).unwrap();
    assert_eq!(project.language_version(), "1.9");
    let baseline = project.content_baseline();
    let fingerprint = project.compile().analysis.fingerprint;
    let previous = project.clone();
    let unchanged = project
        .plan_capability_enable(&request(&project, LanguageVersion::V1_9, &[]))
        .unwrap();
    assert!(!unchanged.manifest_changed);
    assert!(!unchanged.can_apply);
    assert!(unchanged.manifest_bytes_after().is_empty());
    let plan = project
        .plan_capability_enable(&request(&project, LanguageVersion::V1_10, &[]))
        .unwrap();
    assert!(plan.can_apply);
    assert!(plan.manifest_bytes_before().is_none());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.language_version(), "1.9");
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    assert!(!project.root.exists());
    project.apply_capability_enable(&plan).unwrap();
    assert_eq!(project.language_version(), "1.10");
    assert!(project.is_dirty());
    assert!(!project.root.exists());
    let upgraded = project.clone();
    assert!(project.restore(previous));
    assert_eq!(project.language_version(), "1.9");
    assert!(
        project
            .plan_capability_enable(&request(&project, LanguageVersion::V1_10, &[]))
            .unwrap()
            .can_apply
    );
    assert!(project.restore(upgraded));
    assert_eq!(project.language_version(), "1.10");
    for kind in ["place", "organization", "item"] {
        project
            .write_entity(
                &entry,
                None,
                &EntityDraft {
                    id: kind.into(),
                    entity_type: kind.into(),
                    display: kind.into(),
                    description: "资料".into(),
                    properties: Vec::new(),
                },
            )
            .unwrap();
    }
    let compiled = project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert_eq!(compiled.analysis.catalog.entities.len(), 3);
    assert!(!project.root.exists());
}

#[test]
fn known_manifest_extensions_and_existing_feature_tokens_are_preserved_byte_for_byte() {
    let manifest = r#"{
  "schema_version": 1, "language_version" : "1.9",
  "extensions" : {"language_version":"1.9", "number":1e+02,"escaped":"\u4e2d\u6587", "nested":[1, 2]},
  "required_features" : [ "presentation.maps.v1" ], "x-unknown" : "keep\\exact"
}
"#;
    let mut fixture = fixture(SOURCE, Some(manifest));
    let path = fixture.root.join(".world/project.json");
    let mut requested = request(
        &fixture.project,
        LanguageVersion::V1_13,
        &[
            "presentation.maps.v1",
            "content.object_refs.v1",
            "content.character_refs.v1",
        ],
    );
    let plan = fixture.project.plan_capability_enable(&requested).unwrap();
    assert!(plan.can_apply);
    let after = std::str::from_utf8(plan.manifest_bytes_after()).unwrap();
    assert!(after.contains(r#""extensions" : {"language_version":"1.9", "number":1e+02,"escaped":"\u4e2d\u6587", "nested":[1, 2]}"#));
    assert!(after.contains(r#""x-unknown" : "keep\\exact""#));
    assert!(after.contains(r#"[ "presentation.maps.v1" "#));
    assert_eq!(fs::read(&path).unwrap(), manifest.as_bytes());
    let original = fixture.project.clone();
    fixture.project.apply_capability_enable(&plan).unwrap();
    assert_eq!(
        fixture.project.authoring_document(&path).unwrap().bytes(),
        plan.manifest_bytes_after()
    );
    assert_eq!(fs::read(&path).unwrap(), manifest.as_bytes());
    assert!(fixture.project.compile_options().object_refs);
    assert!(fixture.project.compile_options().character_refs);
    assert!(fixture.project.restore(original));
    assert_eq!(
        fixture.project.authoring_document(&path).unwrap().bytes(),
        manifest.as_bytes()
    );
    assert_eq!(fixture.project.language_version(), "1.9");
    requested.expected_baseline = fixture.project.content_baseline();
    assert!(
        fixture
            .project
            .plan_capability_enable(&requested)
            .unwrap()
            .can_apply
    );
}

#[test]
fn missing_manifest_fields_are_appended_without_reencoding_other_bytes() {
    for manifest in [
        r#"{"schema_version":1,"odd":{"a":1e2}}"#,
        r#"{ "schema_version":1, "language_version":"1.9", "required_features": [  ] }"#,
    ] {
        let mut fixture = fixture(SOURCE, Some(manifest));
        let plan = fixture
            .project
            .plan_capability_enable(&request(&fixture.project, LanguageVersion::V1_10, &[]))
            .unwrap();
        assert!(plan.can_apply);
        if manifest.contains("1e2") {
            assert!(std::str::from_utf8(plan.manifest_bytes_after())
                .unwrap()
                .contains(r#""odd":{"a":1e2}"#));
        }
        fixture.project.apply_capability_enable(&plan).unwrap();
        assert_eq!(fixture.project.language_version(), "1.10");
        assert!(fixture
            .project
            .required_features()
            .contains(&"content.entities.v1".into()));
    }
}

#[test]
fn unknown_language_features_and_duplicate_keys_are_read_only_with_exact_raw_bytes() {
    for manifest in [
        r#"{ "schema_version":1,"language_version":"1.14","required_features":[],"x":1e2 }"#,
        r#"{ "schema_version":1,"language_version":"1.9","required_features":["future.v99"],"x":1e2 }"#,
        r#"{ "schema_version":99,"language_version":"1.9","x":1e2 }"#,
        r#"{ "schema_version":1,"language_version":"1.9","language_version":"1.10","x":1e2 }"#,
    ] {
        let fixture = fixture(SOURCE, Some(manifest));
        let path = fixture.root.join(".world/project.json");
        let baseline = fixture.project.content_baseline();
        assert!(fixture
            .project
            .plan_capability_enable(&request(&fixture.project, LanguageVersion::V1_13, &[]))
            .is_err());
        assert_eq!(fixture.project.content_baseline(), baseline);
        assert_eq!(
            fixture.project.authoring_document(&path).unwrap().bytes(),
            manifest.as_bytes()
        );
        assert_eq!(fs::read(&path).unwrap(), manifest.as_bytes());
    }
}

#[test]
fn invalid_combinations_and_downgrades_never_modify_project() {
    let mut fixture = fixture(SOURCE, None);
    let baseline = fixture.project.content_baseline();
    for (version, features) in [
        (LanguageVersion::V1_9, vec!["content.entities.v1"]),
        (
            LanguageVersion::V1_10,
            vec!["content.character_refs.v1", "content.object_refs.v1"],
        ),
        (LanguageVersion::V1_13, vec!["content.character_refs.v1"]),
        (
            LanguageVersion::V1_11,
            vec!["content.choice_presentation.v1"],
        ),
        (LanguageVersion::V1_13, vec!["future.v99"]),
        (LanguageVersion::V1_13, vec!["workspace.source_sets.v1"]),
    ] {
        assert!(fixture
            .project
            .plan_capability_enable(&request(&fixture.project, version, &features))
            .is_err());
        assert_eq!(fixture.project.content_baseline(), baseline);
    }
    let plan = fixture
        .project
        .plan_capability_enable(&request(&fixture.project, LanguageVersion::V1_13, &[]))
        .unwrap();
    fixture.project.apply_capability_enable(&plan).unwrap();
    let baseline = fixture.project.content_baseline();
    assert!(fixture
        .project
        .plan_capability_enable(&request(&fixture.project, LanguageVersion::V1_12, &[]))
        .is_err());
    assert_eq!(fixture.project.content_baseline(), baseline);
    let noop = fixture
        .project
        .plan_capability_enable(&request(&fixture.project, LanguageVersion::V1_13, &[]))
        .unwrap();
    assert!(!noop.manifest_changed);
    assert!(!noop.can_apply);
}

#[test]
fn stale_baselines_and_modified_preview_are_rejected_atomically() {
    let mut fixture = fixture(SOURCE, None);
    let plan = fixture
        .project
        .plan_capability_enable(&request(&fixture.project, LanguageVersion::V1_11, &[]))
        .unwrap();
    let baseline = fixture.project.content_baseline();
    let mut modified = plan.clone();
    modified.compatibility_notes.clear();
    assert!(fixture.project.apply_capability_enable(&modified).is_err());
    assert_eq!(fixture.project.content_baseline(), baseline);
    modified = plan.clone();
    modified.runtime_fingerprint_after ^= 1;
    assert!(fixture.project.apply_capability_enable(&modified).is_err());
    assert_eq!(fixture.project.content_baseline(), baseline);
    let entry = fixture.project.entry.clone();
    fixture
        .project
        .set_text(&entry, format!("{SOURCE}\n// applied unsaved\n"))
        .unwrap();
    let current = fixture.project.content_baseline();
    assert!(fixture.project.apply_capability_enable(&plan).is_err());
    assert_eq!(fixture.project.content_baseline(), current);
    let fresh = fixture
        .project
        .plan_capability_enable(&request(&fixture.project, LanguageVersion::V1_11, &[]))
        .unwrap();
    fixture.project.apply_capability_enable(&fresh).unwrap();
    assert!(fixture
        .project
        .document(&entry)
        .unwrap()
        .contains("applied unsaved"));
    assert_eq!(fs::read_to_string(&entry).unwrap(), SOURCE);
}

#[test]
fn external_changes_and_new_inventory_reject_even_without_refresh() {
    for scenario in [
        "source",
        "new-source",
        "new-manifest",
        "manifest",
        "deleted-source",
    ] {
        let manifest =
            (scenario == "manifest").then_some(r#"{"schema_version":1,"language_version":"1.9"}"#);
        let mut fixture = fixture(SOURCE, manifest);
        let plan = fixture
            .project
            .plan_capability_enable(&request(&fixture.project, LanguageVersion::V1_10, &[]))
            .unwrap();
        let baseline = fixture.project.content_baseline();
        let entry = fixture.root.join("world.wl");
        match scenario {
            "source" => fs::write(&entry, "event changed\n  -> END\n").unwrap(),
            "new-source" => {
                fs::write(fixture.root.join("new.wl"), "event other\n  -> END\n").unwrap()
            }
            "new-manifest" | "manifest" => fs::write(
                fixture.root.join(".world/project.json"),
                r#"{"schema_version":1,"language_version":"1.13","external":true}"#,
            )
            .unwrap(),
            _ => fs::remove_file(&entry).unwrap(),
        }
        assert!(
            fixture.project.apply_capability_enable(&plan).is_err(),
            "{scenario}"
        );
        assert_eq!(fixture.project.content_baseline(), baseline, "{scenario}");
        assert_eq!(fixture.project.language_version(), "1.9");
        assert!(fixture
            .project
            .plan_capability_enable(&request(&fixture.project, LanguageVersion::V1_10, &[]))
            .is_err());
    }
}

#[test]
fn full_applied_sources_expose_new_keyword_errors_and_preserve_bad_draft() {
    let mut fixture = fixture("event start\n  call tomorrow\n  -> END\n", None);
    fixture
        .project
        .add_file(std::path::Path::new("other.wl"))
        .unwrap();
    fixture
        .project
        .set_text(
            &fixture.root.join("other.wl"),
            "event other\n  say absent \"台词\"\n  -> END\n".into(),
        )
        .unwrap();
    let baseline = fixture.project.content_baseline();
    let plan = fixture
        .project
        .plan_capability_enable(&request(&fixture.project, LanguageVersion::V1_11, &[]))
        .unwrap();
    assert!(!plan.can_apply);
    assert!(!plan.fingerprint_comparison_reliable);
    assert!(plan
        .diagnostics_after
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error));
    assert!(!plan.new_diagnostics.is_empty());
    assert!(plan
        .keyword_changes
        .iter()
        .any(|change| change.file.ends_with("world.wl") && change.line == 2));
    assert!(plan
        .keyword_changes
        .iter()
        .any(|change| change.file.ends_with("other.wl") && change.line == 2));
    assert!(fixture.project.apply_capability_enable(&plan).is_err());
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert_eq!(fixture.project.language_version(), "1.9");
}

#[test]
fn valid_keyword_reinterpretation_reports_real_fingerprint_change() {
    let mut fixture = fixture(
        "character guide\nevent start\n  say guide \"台词\"\n  -> END\n",
        None,
    );
    let plan = fixture
        .project
        .plan_capability_enable(&request(&fixture.project, LanguageVersion::V1_11, &[]))
        .unwrap();
    assert!(plan.can_apply, "{:?}", plan.diagnostics_after);
    assert!(plan.fingerprint_comparison_reliable);
    assert_ne!(
        plan.runtime_fingerprint_before,
        plan.runtime_fingerprint_after
    );
    assert!(plan
        .keyword_changes
        .iter()
        .any(|change| change.line == 3 && change.before_kind == "普通正文"));
    assert!(plan
        .compatibility_notes
        .iter()
        .any(|note| note.contains("不能直接载入")));
    fixture.project.apply_capability_enable(&plan).unwrap();
    assert_eq!(
        fixture.project.compile().analysis.fingerprint,
        plan.runtime_fingerprint_after
    );
}

#[test]
fn explicit_source_sets_stay_unchanged_and_archived_keywords_do_not_block() {
    let manifest = r#"{"schema_version":1,"language_version":"1.9","entry":"world.wl","required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":["world.wl"],"archived":["archive.wl"]}}"#;
    let mut fixture = fixture(SOURCE, Some(manifest));
    fs::write(
        fixture.root.join("archive.wl"),
        "event archived\n  call missing\n",
    )
    .unwrap();
    fixture.project.refresh().unwrap();
    let plan = fixture
        .project
        .plan_capability_enable(&request(
            &fixture.project,
            LanguageVersion::V1_13,
            &["workspace.source_sets.v1"],
        ))
        .unwrap();
    assert!(plan.can_apply, "{:?}", plan.diagnostics_after);
    assert!(plan.keyword_changes.is_empty());
    fixture.project.apply_capability_enable(&plan).unwrap();
    assert!(fixture
        .project
        .source_selection()
        .unwrap()
        .is_archived(&fixture.root.join("archive.wl")));
    assert!(fixture
        .project
        .document(&fixture.root.join("archive.wl"))
        .unwrap()
        .contains("call missing"));
}

#[test]
fn unchanged_new_project_template_can_explicitly_select_every_supported_upgrade() {
    let fixture = fixture(SOURCE, None);
    for version in LanguageVersion::SUPPORTED.into_iter().skip(1) {
        let mut project = Project::new(&fixture.root.join(version.as_str()));
        let sources = project.sources();
        let baseline = project.content_baseline();
        let plan = project
            .plan_capability_enable(&request(&project, version, &[]))
            .unwrap();
        assert!(plan.can_apply, "{:?}", plan.diagnostics_after);
        assert_eq!(project.content_baseline(), baseline);
        project.apply_capability_enable(&plan).unwrap();
        assert_eq!(project.language_version_kind(), version);
        assert_eq!(project.sources(), sources);
        assert!(!project.root.exists());
    }
}

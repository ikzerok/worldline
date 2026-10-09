#[cfg(not(target_arch = "wasm32"))]
#[path = "localization/save_refresh_history.rs"]
mod save_refresh_history;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::ast::Stmt;
use worldline_core::localization::{LocalizationExchange, LocalizationPart, LocalizationSelection};
use worldline_core::project::Project;

const SOURCE: &str = concat!(
    "let traveler = \"Ari\"\n",
    "event arrival\n",
    "  Welcome, {traveler} 👋 at [[event:private_event|Harbor]]!\\nSecond line مرحبا 🌙 #wl-localization:welcome\n",
    "  choice \"Continue {traveler}\" if true #wl-localization:reply\n",
    "    -> END\n",
    "  -> END\n",
    "event private_event as \"PRIVATE_EVENT_SENTINEL\"\n",
    "  PRIVATE_SOURCE_SENTINEL\n",
    "  -> END\n",
);

struct Fixture {
    root: PathBuf,
    project: Project,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture(name: &str, source: &str, existing_sidecar: bool) -> Fixture {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "worldline-localization-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();

    let mut manifest = serde_json::json!({
        "schema_version": 1,
        "language_version": "1.10",
        "entry": "world.wl",
        "required_features": ["content.localization.v1"],
        "extension": {"keep": "manifest"}
    });
    if existing_sidecar {
        fs::create_dir_all(root.join(".world/localization")).unwrap();
        manifest["localizations"] = serde_json::json!({
            "zh-Hant": ".world/localization/zh-Hant.json"
        });
        fs::write(
            root.join(".world/localization/zh-Hant.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "required_features": ["content.localization.v1"],
                "source_locale": "en",
                "target_locale": "zh-Hant",
                "entries": {},
                "extension": {"keep": "sidecar"}
            }))
            .unwrap(),
        )
        .unwrap();
    }
    fs::write(
        root.join(".world/project.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    Fixture { root, project }
}

fn selection(ids: &[&str]) -> LocalizationSelection {
    LocalizationSelection {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh-Hant".into(),
        string_ids: ids.iter().map(|id| (*id).into()).collect(),
    }
}

#[test]
fn export_uses_only_selected_ids_and_keeps_expressions_and_link_targets_opaque() {
    let mut fixture = fixture("export", SOURCE, false);
    let compiled = fixture.project.compile();
    let original = compiled.analysis.fingerprint;
    let Stmt::Text(text) = &compiled.program.events[0].body[0] else {
        panic!("the selected text line should compile as text");
    };
    assert_eq!(text.localization_id.as_deref(), Some("welcome"));
    assert!(!text
        .tags
        .iter()
        .any(|tag| tag.starts_with("wl-localization:")));
    let Stmt::Choice(choice) = &compiled.program.events[0].body[1] else {
        panic!("the selected option should compile as a choice");
    };
    assert_eq!(choice.localization_id.as_deref(), Some("reply"));
    let source = fixture
        .project
        .document(&fixture.root.join("world.wl"))
        .unwrap();
    let mut without_ids = fixture.project.clone();
    without_ids
        .set_text(
            &fixture.root.join("world.wl"),
            source
                .replace(" #wl-localization:welcome", "")
                .replace(" #wl-localization:reply", ""),
        )
        .unwrap();
    assert_eq!(
        without_ids.compile().analysis.fingerprint,
        original,
        "source-owned localization IDs must not change runtime fingerprints"
    );

    let selection = selection(&["welcome", "reply"]);
    let before = fixture.project.content_baseline();
    let preview = fixture
        .project
        .preview_localization_export(&selection)
        .unwrap();

    assert!(preview.can_export, "{:?}", preview.diagnostics);
    assert_eq!(preview.content_baseline, before);
    assert_eq!(fixture.project.content_baseline(), before);
    assert_eq!(preview.exchange.string_ids, ["reply", "welcome"]);
    assert_eq!(preview.exchange.entries.len(), 2);
    let welcome = preview
        .exchange
        .entries
        .iter()
        .find(|entry| entry.id == "welcome")
        .unwrap();
    assert_eq!(welcome.source.file, "world.wl");
    assert!(welcome
        .source_parts
        .iter()
        .any(|part| matches!(part, LocalizationPart::Placeholder { token } if token == "p0")));
    assert!(welcome.source_parts.iter().any(
        |part| matches!(part, LocalizationPart::Link { token, label } if token == "l0" && label == "Harbor")
    ));

    let json = serde_json::to_string(&preview.exchange).unwrap();
    assert!(json.contains("مرحبا"));
    assert!(json.contains("🌙"));
    assert!(welcome.source_parts.iter().any(
        |part| matches!(part, LocalizationPart::Text { text } if text.contains("\nSecond line"))
    ));
    assert!(!json.contains("PRIVATE_SOURCE_SENTINEL"));
    assert!(!json.contains("PRIVATE_EVENT_SENTINEL"));
    assert!(
        !json.contains("private_event"),
        "link targets are not exported"
    );
    assert!(
        !json.contains("traveler"),
        "interpolation expressions are not exported"
    );
}

fn translated(mut exchange: LocalizationExchange) -> LocalizationExchange {
    for entry in &mut exchange.entries {
        entry.translation_parts = Some(
            entry
                .source_parts
                .iter()
                .map(|part| match part {
                    LocalizationPart::Text { text } => LocalizationPart::Text {
                        text: text.replace("Welcome", "欢迎").replace("Continue", "继续"),
                    },
                    LocalizationPart::Placeholder { token } => LocalizationPart::Placeholder {
                        token: token.clone(),
                    },
                    LocalizationPart::Link { token, .. } => LocalizationPart::Link {
                        token: token.clone(),
                        label: "港口".into(),
                    },
                })
                .collect(),
        );
    }
    exchange
}

#[test]
fn import_updates_existing_locale_sidecar_preserving_unknown_fields() {
    let mut fixture = fixture("import", SOURCE, true);
    let selection = selection(&["welcome"]);
    let export = fixture
        .project
        .preview_localization_export(&selection)
        .unwrap();
    let exchange = translated(export.exchange);
    let baseline = fixture.project.content_baseline();
    let preview = fixture
        .project
        .preview_localization_import(&selection, &exchange)
        .unwrap();

    assert!(preview.can_apply, "{:?}", preview.diagnostics);
    assert_eq!(preview.content_baseline, baseline);
    let result = fixture
        .project
        .apply_localization_import(&selection, &exchange, &preview.plan_digest)
        .unwrap();

    assert_eq!(result.baseline, baseline);
    assert_eq!(result.new_baseline, fixture.project.content_baseline());
    assert_ne!(result.new_baseline, baseline);
    assert!(!result
        .changed_files
        .contains(&fixture.project.root.join(".world/project.json")));
    let sidecar_path = fixture.root.join(".world/localization/zh-Hant.json");
    assert!(result.changed_files.contains(
        &fixture
            .project
            .root
            .join(".world/localization/zh-Hant.json")
    ));
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.root.join(".world/project.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["extension"]["keep"], "manifest");
    let sidecar: serde_json::Value =
        serde_json::from_slice(&fs::read(sidecar_path).unwrap()).unwrap();
    assert_eq!(sidecar["extension"]["keep"], "sidecar");
    assert_eq!(
        sidecar["entries"]["welcome"]["source_revision"],
        exchange.entries[0].source_revision
    );
    assert_eq!(
        sidecar["entries"]["welcome"]["translation_parts"][0]["text"],
        "欢迎, "
    );
    assert_eq!(
        fixture
            .project
            .document(&fixture.root.join("world.wl"))
            .unwrap(),
        SOURCE,
        "localization import must not rewrite source text"
    );
}

#[test]
fn first_import_registers_a_new_locale_sidecar() {
    let mut fixture = fixture("first-locale", SOURCE, false);
    let selection = selection(&["welcome"]);
    let export = fixture
        .project
        .preview_localization_export(&selection)
        .unwrap();
    let exchange = translated(export.exchange);
    let plan = fixture
        .project
        .preview_localization_import(&selection, &exchange)
        .unwrap();
    assert!(plan.can_apply, "{:?}", plan.diagnostics);
    assert_eq!(plan.affected_ids, vec!["welcome"]);

    let result = fixture
        .project
        .apply_localization_import(&selection, &exchange, &plan.plan_digest)
        .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.root.join(".world/project.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["extension"]["keep"], "manifest");
    assert_eq!(
        manifest["localizations"]["zh-Hant"],
        ".world/localization/zh-Hant.json"
    );
    assert!(result
        .changed_files
        .contains(&fixture.project.root.join(".world/project.json")));
    assert!(result.changed_files.contains(
        &fixture
            .project
            .root
            .join(".world/localization/zh-Hant.json")
    ));
}

#[test]
fn missing_or_invalid_translations_and_stale_sources_block_the_whole_import() {
    let mut fixture = fixture("reject", SOURCE, false);
    let selection = selection(&["welcome"]);
    let exported = fixture
        .project
        .preview_localization_export(&selection)
        .unwrap()
        .exchange;
    let baseline = fixture.project.content_baseline();

    let missing = fixture
        .project
        .preview_localization_import(&selection, &exported)
        .unwrap();
    assert!(!missing.can_apply);
    assert!(missing
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MISSING_TRANSLATION"));
    assert!(fixture
        .project
        .apply_localization_import(&selection, &exported, &missing.plan_digest)
        .is_err());
    assert_eq!(fixture.project.content_baseline(), baseline);

    let mut invalid_tokens = translated(exported.clone());
    invalid_tokens.entries[0]
        .translation_parts
        .as_mut()
        .unwrap()
        .retain(|part| !matches!(part, LocalizationPart::Placeholder { .. }));
    let invalid = fixture
        .project
        .preview_localization_import(&selection, &invalid_tokens)
        .unwrap();
    assert!(!invalid.can_apply);
    assert!(invalid
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "INVALID_TOKEN"));
    assert!(fixture
        .project
        .apply_localization_import(&selection, &invalid_tokens, &invalid.plan_digest)
        .is_err());
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert!(!fixture
        .root
        .join(".world/localization/zh-Hant.json")
        .exists());

    let valid = translated(exported);
    let source = fixture
        .project
        .document(&fixture.root.join("world.wl"))
        .unwrap();
    fixture
        .project
        .set_text(
            &fixture.root.join("world.wl"),
            source.replace("Welcome", "Changed"),
        )
        .unwrap();
    let dirty_baseline = fixture.project.content_baseline();
    let stale = fixture
        .project
        .preview_localization_import(&selection, &valid)
        .unwrap();
    assert!(!stale.can_apply);
    assert!(stale.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "STALE_SOURCE" || diagnostic.code == "BASELINE_MISMATCH"
    }));
    assert!(fixture
        .project
        .apply_localization_import(&selection, &valid, &stale.plan_digest)
        .is_err());
    assert_eq!(fixture.project.content_baseline(), dirty_baseline);
    assert!(!fixture
        .root
        .join(".world/localization/zh-Hant.json")
        .exists());
}

#[test]
fn source_baseline_mismatch_rejects_a_clean_project_import() {
    let fixture = fixture("clean-stale", SOURCE, false);
    let selection = selection(&["welcome"]);
    let exchange = translated(
        fixture
            .project
            .preview_localization_export(&selection)
            .unwrap()
            .exchange,
    );
    fs::write(
        fixture.root.join("world.wl"),
        SOURCE.replace("Welcome", "Changed"),
    )
    .unwrap();
    let mut current_project = Project::open(&fixture.root).unwrap();
    let baseline = current_project.content_baseline();
    let manifest_before = fs::read(fixture.root.join(".world/project.json")).unwrap();
    let stale = current_project
        .preview_localization_import(&selection, &exchange)
        .unwrap();
    assert!(!stale.can_apply);
    assert!(stale.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "STALE_SOURCE" || diagnostic.code == "BASELINE_MISMATCH"
    }));
    assert!(current_project
        .apply_localization_import(&selection, &exchange, &stale.plan_digest)
        .is_err());
    assert_eq!(current_project.content_baseline(), baseline);
    assert_eq!(
        fs::read(fixture.root.join(".world/project.json")).unwrap(),
        manifest_before
    );
    assert!(!fixture
        .root
        .join(".world/localization/zh-Hant.json")
        .exists());
}

#[test]
fn duplicate_source_ids_and_unsupported_package_versions_are_reported() {
    let duplicate_source = SOURCE
        .replace("#wl-localization:welcome", "#wl-localization:duplicate")
        .replace("#wl-localization:reply", "#wl-localization:duplicate");
    let duplicate_fixture = fixture("duplicate", &duplicate_source, false);
    let duplicate = duplicate_fixture
        .project
        .preview_localization_export(&selection(&["duplicate"]))
        .unwrap();
    assert!(!duplicate.can_export);
    assert!(duplicate
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DUPLICATE_ID"));

    let mut fixture = fixture("version", SOURCE, false);
    let selection = selection(&["welcome"]);
    let mut exchange = translated(
        fixture
            .project
            .preview_localization_export(&selection)
            .unwrap()
            .exchange,
    );
    exchange.schema_version = 2;
    let plan = fixture
        .project
        .preview_localization_import(&selection, &exchange)
        .unwrap();
    assert!(!plan.can_apply);
    assert!(plan
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "UNSUPPORTED_VERSION"));
    exchange.schema_version = 1;
    let mut duplicate_package = exchange.clone();
    let duplicate_entry = duplicate_package.entries[0].clone();
    duplicate_package.entries.push(duplicate_entry);
    let duplicate_plan = fixture
        .project
        .preview_localization_import(&selection, &duplicate_package)
        .unwrap();
    assert!(!duplicate_plan.can_apply);
    assert!(duplicate_plan
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DUPLICATE_ID"));
    assert!(fixture
        .project
        .apply_localization_import(&selection, &duplicate_package, &duplicate_plan.plan_digest)
        .is_err());

    let mut expanded_selection = exchange;
    expanded_selection.string_ids.push("reply".into());
    let expanded_plan = fixture
        .project
        .preview_localization_import(&selection, &expanded_selection)
        .unwrap();
    assert!(!expanded_plan.can_apply);
    assert!(expanded_plan
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "SELECTION_MISMATCH"));
    assert!(fixture
        .project
        .apply_localization_import(&selection, &expanded_selection, &expanded_plan.plan_digest)
        .is_err());
    assert!(!fixture
        .root
        .join(".world/localization/zh-Hant.json")
        .exists());
}

#[test]
fn localization_annotation_requires_the_manifest_feature() {
    let fixture = fixture("feature-gate", SOURCE, false);
    let manifest_path = fixture.root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["required_features"] = serde_json::json!([]);
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let project = Project::open(&fixture.root).unwrap();
    let compiled = project.clone().compile();
    assert!(compiled
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "P004"));
    assert!(project
        .preview_localization_export(&selection(&["welcome"]))
        .is_err());
}
#[test]
fn exchange_json_roundtrips_unicode_and_rejects_duplicate_keys() {
    let fixture = fixture("json", SOURCE, false);
    let exchange = fixture
        .project
        .preview_localization_export(&selection(&["welcome"]))
        .unwrap()
        .exchange;
    let bytes = serde_json::to_vec(&exchange).unwrap();
    assert_eq!(
        LocalizationExchange::from_json_bytes(&bytes).unwrap(),
        exchange
    );
    assert!(
        LocalizationExchange::from_json_bytes(br#"{"schema_version":1,"schema_version":1}"#)
            .is_err()
    );
}

#[test]
fn export_writes_a_new_file_only_outside_the_workspace() {
    let fixture = fixture("file-export", SOURCE, false);
    let selection = selection(&["welcome"]);
    let preview = fixture
        .project
        .preview_localization_export(&selection)
        .unwrap();
    let filename = fixture.root.file_name().unwrap().to_string_lossy();
    let destination = fixture
        .root
        .parent()
        .unwrap()
        .join(format!("{filename}.json"));
    let expected = serde_json::to_vec(&preview.exchange).unwrap();
    let exported = fixture
        .project
        .export_localization(&selection, &preview.plan_digest, &destination)
        .unwrap();
    assert_eq!(exported.plan_digest, preview.plan_digest);
    assert_eq!(fs::read(&destination).unwrap(), expected);

    assert!(fixture
        .project
        .export_localization(&selection, &preview.plan_digest, &destination)
        .is_err());
    assert_eq!(fs::read(&destination).unwrap(), expected);

    let inside = fixture.root.join(".world/denied.json");
    assert!(fixture
        .project
        .export_localization(&selection, &preview.plan_digest, &inside)
        .is_err());
    assert!(!inside.exists());
    fs::remove_file(destination).unwrap();
}

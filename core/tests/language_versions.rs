//! 支持集合、机器 Schema 与真实工程读取不得漂移。
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use worldline_core::{project::Project, CompileOptions, LanguageVersion};

fn root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "wl-language-contract-{label}-{}",
        std::process::id()
    ))
}
fn manifest(version: &str) -> Value {
    json!({"schema_version":1,"project_id":"contract","language_version":version,
        "entry":"world.wl","required_features":[],"maps":{},"graph_views":{},
        "manuscripts":{},"presets":{},"comments":{},"proposals":{},"saved_queries":{},
        "templates":{},"localizations":{},"extensions":{"opaque":"保留"}})
}
#[test]
fn published_schema_matches_every_supported_language_and_serde() {
    let schema: Value =
        serde_json::from_str(include_str!("../../spec/schemas/project.schema.json")).unwrap();
    let supported: Vec<_> = LanguageVersion::SUPPORTED
        .iter()
        .map(|version| version.as_str())
        .collect();
    assert_eq!(
        schema["properties"]["language_version"]["enum"],
        json!(supported)
    );
    for version in LanguageVersion::SUPPORTED {
        assert_eq!(
            LanguageVersion::from_supported_str(version.as_str()),
            Some(version)
        );
        assert_eq!(
            serde_json::to_value(version).unwrap(),
            json!(version.as_str())
        );
        assert_eq!(
            serde_json::from_value::<LanguageVersion>(json!(version.as_str())).unwrap(),
            version
        );
    }
    for unknown in ["1.8", "1.14", "1.130", "latest", "2.0", ""] {
        assert_eq!(LanguageVersion::from_supported_str(unknown), None);
        assert!(serde_json::from_value::<LanguageVersion>(json!(unknown)).is_err());
    }
}
#[test]
fn every_supported_full_manifest_opens_without_migration() {
    for version in LanguageVersion::SUPPORTED {
        let root = root(version.as_str());
        fs::create_dir_all(root.join(".world")).unwrap();
        let bytes = serde_json::to_vec_pretty(&manifest(version.as_str())).unwrap();
        fs::write(root.join(".world/project.json"), &bytes).unwrap();
        fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
        let project = Project::open(&root).unwrap();
        assert_eq!(project.language_version_kind(), version);
        assert!(
            project.authoring_diagnostics().is_empty(),
            "{:?}",
            project.authoring_diagnostics()
        );
        assert!(!worldline_core::compile_sources_with_options(
            &root.join("world.wl"),
            &project.sources(),
            project.compile_options()
        )
        .has_errors());
        assert_eq!(
            project
                .authoring_document(&root.join(".world/project.json"))
                .unwrap()
                .bytes(),
            bytes
        );
        assert_eq!(fs::read(root.join(".world/project.json")).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn unknown_versions_and_features_preserve_raw_manifest_and_block_edits() {
    for (label, mut value) in [
        ("unknown-version", manifest("1.14")),
        ("unknown-feature", manifest("1.13")),
    ] {
        if label == "unknown-feature" {
            value["required_features"] = json!(["content.future_refs.v99"]);
        }
        let root = root(label);
        fs::create_dir_all(root.join(".world")).unwrap();
        let path = root.join(".world/project.json");
        let bytes = serde_json::to_vec_pretty(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
        let mut project = Project::open(&root).unwrap();
        assert!(project
            .authoring_diagnostics()
            .iter()
            .any(|d| d.code == "WS003"));
        assert_eq!(project.authoring_document(&path).unwrap().bytes(), bytes);
        assert!(project
            .set_authoring_document(&path, b"{}".to_vec())
            .is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    }
    assert_eq!(
        CompileOptions::default().language_version,
        LanguageVersion::V1_9
    );
    assert!(!CompileOptions::v1_13().object_refs);
    assert!(!CompileOptions::v1_13().character_refs);
}

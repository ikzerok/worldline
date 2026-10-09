//! Pure imported snapshots: no host file/path-length capability is used by these cases.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::localization::*;
use worldline_core::project::Project;

fn long_path(filename: &str) -> String {
    let mut parts: Vec<_> = (0..40)
        .map(|index| format!("segment_{index:02}_{}", "x".repeat(28)))
        .collect();
    parts.push(filename.into());
    let value = parts.join("/");
    assert!(value.len() >= 1_500);
    assert!(parts.iter().all(|part| part.len() < 64));
    value
}

fn fixture(count: usize, long_source: bool, long_sidecar: bool) -> Project {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    #[cfg(not(target_arch = "wasm32"))]
    let root = std::env::temp_dir().join(format!(
        "worldline-localization-metadata-snapshot-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    #[cfg(target_arch = "wasm32")]
    let root = PathBuf::from(format!(
        "/worldline-localization-metadata-snapshot-{}",
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let sidecar_path = if long_sidecar {
        long_path("locale.json")
    } else {
        ".world/localization/zh.json".into()
    };
    let mut body = "  X\n".repeat(count);
    body.push_str("  -> END\n");
    let mut files = BTreeMap::new();
    if long_source {
        let path = long_path("body.wl");
        files.insert(
            PathBuf::from("world.wl"),
            format!("event start\ninclude \"{path}\"\n").into_bytes(),
        );
        files.insert(PathBuf::from(path), body.into_bytes());
    } else {
        files.insert(
            PathBuf::from("world.wl"),
            format!("event start\n{body}").into_bytes(),
        );
    }
    files.insert(PathBuf::from(".world/project.json"), serde_json::to_vec(&serde_json::json!({
        "schema_version":1,"language_version":"1.13","entry":"world.wl","required_features":["content.localization.v1"],
        "localizations":{"zh":sidecar_path}
    })).unwrap());
    files.insert(PathBuf::from(&sidecar_path), serde_json::to_vec(&serde_json::json!({
        "schema_version":1,"required_features":["content.localization.v1"],"source_locale":"en","target_locale":"zh","entries":{}
    })).unwrap());
    #[cfg(target_arch = "wasm32")]
    worldline_core::file_access::mount(
        files
            .iter()
            .map(|(path, bytes)| (root.join(path), bytes.clone()))
            .collect(),
    );
    Project::from_snapshot(&root, Path::new("world.wl"), &files).unwrap()
}

#[test]
fn long_sidecar_metadata_is_counted_before_batch_cloning_and_whole_snapshot_is_bounded() {
    let request = LocalizationPresentationRequest {
        schema_version: 1,
        target_locale: "zh".into(),
        policy: LocalizationPresentationPolicy::SourceFallback,
    };
    let small = fixture(3, false, true);
    let snapshot = small.prepare_localization_presentation(&request).unwrap();
    assert!(snapshot.entries()[0].sidecar_path.as_ref().unwrap().len() >= 1_500);
    assert!(serde_json::to_vec(&snapshot).unwrap().len() < 64 * 1024 * 1024);
    let large = fixture(MAX_LOCALIZATION_UNITS, false, true);
    let baseline = large.content_baseline();
    let state = large.snapshot_state().unwrap();
    let error = large
        .prepare_localization_presentation(&request)
        .unwrap_err();
    assert_eq!(error.code, "BUDGET_EXCEEDED");
    assert!(error.message.contains("完整目录条目元数据"));
    assert_eq!(large.content_baseline(), baseline);
    assert_eq!(large.snapshot_state().unwrap(), state);
    assert!(!large.is_dirty());
    let query = LocalizationCatalogQuery {
        target_locale: Some("zh".into()),
        limit: 1,
        ..Default::default()
    };
    assert_eq!(
        large.query_localization_catalog(&query).unwrap_err().code,
        "BUDGET_EXCEEDED"
    );
}

#[test]
fn long_included_source_metadata_is_bounded_before_catalog_or_presentation_clones_it() {
    let small = fixture(3, true, false);
    let request = LocalizationPresentationRequest {
        schema_version: 1,
        target_locale: "zh".into(),
        policy: LocalizationPresentationPolicy::SourceFallback,
    };
    let snapshot = small.prepare_localization_presentation(&request).unwrap();
    assert!(snapshot.entries()[0].source.file.len() >= 1_500);
    let large = fixture(MAX_LOCALIZATION_UNITS, true, false);
    let baseline = large.content_baseline();
    let state = large.snapshot_state().unwrap();
    let error = large
        .prepare_localization_presentation(&request)
        .unwrap_err();
    assert_eq!(error.code, "BUDGET_EXCEEDED");
    assert!(error.message.contains("完整来源元数据"));
    assert_eq!(large.content_baseline(), baseline);
    assert_eq!(large.snapshot_state().unwrap(), state);
    assert!(!large.is_dirty());
}

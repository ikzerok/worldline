use super::*;
use worldline_core::localization::LocalizationSelection;
const SOURCE: &str = "event start\n  Welcome #wl-localization:greeting\n  -> END\n";

#[test]
fn localization_legacy_baseline_is_stable_and_character_capability_expires_exchange() {
    let f = fixture("localization");
    fs::remove_file(f.root.join("people.wl")).unwrap();
    fs::write(f.root.join("world.wl"), SOURCE).unwrap();
    let manifest = f.root.join(".world/project.json");
    let mut value = serde_json::json!({"schema_version":1,"language_version":"1.12","required_features":["content.object_refs.v1","content.localization.v1"]});
    fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    let selection = LocalizationSelection {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh".into(),
        string_ids: vec!["greeting".into()],
    };
    let old = Project::open(&f.root).unwrap();
    let old_plan = old.preview_localization_export(&selection).unwrap();
    assert!(old_plan.can_export, "{:?}", old_plan.diagnostics);
    // Released pre-character-ref source-baseline encoding, including exact field order.
    assert_eq!(
        old_plan.exchange.source_baseline,
        "fnv1a64:846322dde7b62ed9"
    );
    value["language_version"] = "1.13".into();
    fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    let without = Project::open(&f.root).unwrap();
    let mut exchange = without
        .preview_localization_export(&selection)
        .unwrap()
        .exchange;
    for entry in &mut exchange.entries {
        entry.translation_parts = Some(entry.source_parts.clone());
    }
    let original_source = fs::read(f.root.join("world.wl")).unwrap();
    value["required_features"]
        .as_array_mut()
        .unwrap()
        .push("content.character_refs.v1".into());
    fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut enabled = Project::open(&f.root).unwrap();
    let enabled_plan = enabled.preview_localization_export(&selection).unwrap();
    assert_ne!(
        enabled_plan.exchange.source_baseline,
        exchange.source_baseline
    );
    let rejected = enabled
        .preview_localization_import(&selection, &exchange)
        .unwrap();
    assert!(!rejected.can_apply);
    assert!(rejected
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "BASELINE_MISMATCH"));
    assert!(enabled
        .apply_localization_import(&selection, &exchange, &rejected.plan_digest)
        .is_err());
    assert_eq!(fs::read(f.root.join("world.wl")).unwrap(), original_source);
}

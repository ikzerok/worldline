#![cfg(not(target_arch = "wasm32"))]
#[path = "localization_workbench/support.rs"]
mod support;
use support::*;
use worldline_core::localization::*;

#[test]
fn candidate_json_rejects_duplicate_keys_unknown_fields_and_byte_overflow() {
    let duplicate = br#"{"schema_version":1,"schema_version":1}"#;
    assert_eq!(
        LocalizationEditDraft::from_json_bytes(duplicate)
            .unwrap_err()
            .code,
        "INVALID_JSON"
    );
    assert_eq!(
        LocalizationIdDraft::from_json_bytes(duplicate)
            .unwrap_err()
            .code,
        "INVALID_JSON"
    );
    assert_eq!(
        LocalizationImportDraft::from_json_bytes(
            br#"{"selection":{},"exchange":{"entries":{"x":1,"x":2}}}"#
        )
        .unwrap_err()
        .code,
        "INVALID_JSON"
    );
    let fixture = fixture("json", SOURCE, None);
    let draft = edit(&fixture.project, &["greeting"]);
    let mut bytes = serde_json::to_vec(&draft).unwrap();
    bytes.resize(MAX_LOCALIZATION_JSON_BYTES, b' ');
    assert_eq!(
        LocalizationEditDraft::from_json_bytes(&bytes).unwrap(),
        draft
    );
    bytes.push(b' ');
    assert_eq!(
        LocalizationEditDraft::from_json_bytes(&bytes)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    let mut json = serde_json::to_value(&draft).unwrap();
    json["unexpected"] = true.into();
    assert_eq!(
        LocalizationEditDraft::from_json_bytes(&serde_json::to_vec(&json).unwrap())
            .unwrap_err()
            .code,
        "INVALID_JSON"
    );
    // The old exchange parser retains its former unbounded input contract.
    let selection = LocalizationSelection {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh-Hant".into(),
        string_ids: vec!["greeting".into()],
    };
    let exchange = fixture
        .project
        .preview_localization_export(&selection)
        .unwrap()
        .exchange;
    let mut legacy = serde_json::to_vec(&exchange).unwrap();
    legacy.resize(MAX_LOCALIZATION_JSON_BYTES + 1, b' ');
    assert_eq!(
        LocalizationExchange::from_json_bytes(&legacy).unwrap(),
        exchange
    );
    assert_eq!(
        LocalizationExchange::from_json_bytes_limited(&legacy)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
}

#[test]
fn typed_part_count_and_utf8_bytes_have_exact_boundaries() {
    let fixture = fixture(
        "parts",
        "event start\n  Plain #wl-localization:plain\n  -> END\n",
        None,
    );
    let mut draft = edit(&fixture.project, &["plain"]);
    draft.edits[0].translation_parts = vec![LocalizationPart::Text {
        text: "a".repeat(MAX_LOCALIZATION_UNIT_BYTES),
    }];
    assert!(
        fixture
            .project
            .preview_localization_edit(&draft)
            .unwrap()
            .can_apply
    );
    draft.edits[0].translation_parts = vec![LocalizationPart::Text {
        text: format!("{}🙂", "a".repeat(MAX_LOCALIZATION_UNIT_BYTES - 3)),
    }];
    assert_eq!(
        fixture
            .project
            .preview_localization_edit(&draft)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    draft.edits[0].translation_parts = vec![
        LocalizationPart::Text {
            text: String::new()
        };
        MAX_LOCALIZATION_PARTS
    ];
    assert!(
        fixture
            .project
            .preview_localization_edit(&draft)
            .unwrap()
            .can_apply
    );
    draft.edits[0]
        .translation_parts
        .push(LocalizationPart::Text {
            text: String::new(),
        });
    assert_eq!(
        fixture
            .project
            .preview_localization_edit(&draft)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    draft.edits[0].translation_parts.clear();
    assert!(
        fixture
            .project
            .preview_localization_edit(&draft)
            .unwrap()
            .can_apply,
        "empty translation is data"
    );
}

#[test]
fn query_batch_source_and_serialized_output_budgets_fail_without_partial_results() {
    let mut fixture = fixture("bounds", SOURCE, None);
    let before = disk(&fixture.root);
    let mut request = query();
    request.limit = MAX_LOCALIZATION_PAGE_SIZE;
    assert!(fixture.project.query_localization_catalog(&request).is_ok());
    request.limit += 1;
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&request)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    request.limit = 0;
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&request)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    request.limit = 1;
    request.statuses = vec![LocalizationStatus::Translated; 8];
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&request)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    request.statuses = vec![LocalizationStatus::Translated; 2];
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&request)
            .unwrap_err()
            .code,
        "INVALID_QUERY"
    );
    let mut draft = edit(&fixture.project, &["greeting"]);
    draft
        .edits
        .resize(MAX_LOCALIZATION_BATCH + 1, draft.edits[0].clone());
    assert_eq!(
        fixture
            .project
            .preview_localization_edit(&draft)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    let huge_source = "a".repeat(32 * 1024 * 1024 + 1);
    fixture
        .project
        .set_text(&fixture.root.join("world.wl"), huge_source)
        .unwrap();
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&query())
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    assert_eq!(disk(&fixture.root), before);
    let mut source = "event start\n".to_string();
    for index in 0..200 {
        source.push_str(&format!(
            "  {} #wl-localization:item_{index}\n",
            "x".repeat(12_000)
        ));
    }
    source.push_str("  -> END\n");
    fixture
        .project
        .set_text(&fixture.root.join("world.wl"), source)
        .unwrap();
    request = query();
    request.limit = 200;
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&request)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    request.limit = 1;
    let bounded = fixture
        .project
        .query_localization_catalog(&request)
        .unwrap();
    assert_eq!(
        (bounded.all_total, bounded.total, bounded.entries.len()),
        (200, 200, 1)
    );
}

#[test]
fn diagnostic_budget_rejects_overflow_instead_of_returning_applyable_truncation() {
    for count in [200, 201] {
        let mut source = "event start\n".to_string();
        let mut ids = Vec::new();
        for index in 0..count {
            source.push_str(&format!("  Word #wl-localization:item_{index}\n"));
            ids.push(format!("item_{index}"));
        }
        source.push_str("  -> END\n");
        let fixture = fixture("diagnostics", &source, None);
        let selection = LocalizationSelection {
            schema_version: 1,
            source_locale: "en".into(),
            target_locale: "zh-Hant".into(),
            string_ids: ids,
        };
        let exchange = fixture
            .project
            .preview_localization_export(&selection)
            .unwrap()
            .exchange;
        let result = fixture
            .project
            .preview_localization_import_candidate(&selection, &exchange);
        if count == 200 {
            let plan = result.unwrap();
            assert!(!plan.can_apply);
            assert_eq!(plan.diagnostics.len(), 200);
        } else {
            assert_eq!(result.unwrap_err().code, "BUDGET_EXCEEDED");
        }
        assert!(!fixture.project.is_dirty());
    }
}

#[test]
fn sidecar_overflow_and_duplicate_keys_disable_typed_writes_without_losing_raw_repair() {
    let mut fixture = fixture(
        "sidecar-limit",
        SOURCE,
        Some(sidecar(serde_json::json!({}))),
    );
    let path = fixture.root.join(".world/localization/zh-Hant.json");
    let mut bytes = serde_json::to_vec(&sidecar(serde_json::json!({}))).unwrap();
    bytes.resize(MAX_LOCALIZATION_JSON_BYTES + 1, b' ');
    fixture
        .project
        .set_authoring_document(&path, bytes)
        .unwrap();
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&query())
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    std::fs::write(&path, br#"{"schema_version":1,"required_features":["content.localization.v1"],"source_locale":"en","target_locale":"zh-Hant","entries":{"same":{},"same":{}}}"#).unwrap();
    let mut reopened = worldline_core::project::Project::open(&fixture.root).unwrap();
    let state = page(&reopened);
    assert!(state.read_only);
    assert!(
        !reopened.authoring_document(&path).unwrap().is_read_only(),
        "generic raw-byte repair remains available; only typed localization writes are blocked"
    );
    let baseline = reopened.content_baseline();
    let before = disk(&fixture.root);
    let draft = edit(&reopened, &["greeting"]);
    let plan = reopened.preview_localization_edit(&draft).unwrap();
    assert!(!plan.can_apply);
    assert!(plan
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "SIDECAR_INVALID"));
    assert!(reopened
        .apply_localization_edit(&draft, &plan.plan_digest)
        .is_err());
    assert_eq!(reopened.content_baseline(), baseline);
    assert_eq!(disk(&fixture.root), before);
    assert_eq!(
        state.entries[0].status,
        LocalizationStatus::InvalidTranslation
    );
}

#[test]
fn source_unit_limit_has_exact_inclusive_boundary() {
    let mut source = "event start\n".to_string();
    for _ in 0..MAX_LOCALIZATION_UNITS {
        source.push_str("  Word\n");
    }
    source.push_str("  -> END\n");
    let mut fixture = fixture("unit-count", &source, None);
    let query = LocalizationCatalogQuery {
        limit: 1,
        ..Default::default()
    };
    let page = fixture.project.query_localization_catalog(&query).unwrap();
    assert_eq!(
        (page.all_total, page.total, page.entries.len()),
        (MAX_LOCALIZATION_UNITS, MAX_LOCALIZATION_UNITS, 1)
    );
    source = source.replace("  -> END\n", "  Extra\n  -> END\n");
    fixture
        .project
        .set_text(&fixture.root.join("world.wl"), source)
        .unwrap();
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&query)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
}

#[test]
fn orphan_entry_limit_is_independent_of_current_source_count() {
    let entries: serde_json::Map<_, _> = (0..MAX_LOCALIZATION_UNITS)
        .map(|index| {
            (
                format!("old_{index}"),
                serde_json::json!({"source_revision":"old","translation_parts":[]}),
            )
        })
        .collect();
    let mut fixture = fixture(
        "orphan-count",
        SOURCE,
        Some(sidecar(serde_json::Value::Object(entries))),
    );
    let mut query = query();
    query.limit = 1;
    let page = fixture.project.query_localization_catalog(&query).unwrap();
    assert_eq!(page.all_total, MAX_LOCALIZATION_UNITS + 2);
    assert_eq!(
        page.status_counts[&LocalizationStatus::OrphanTranslation],
        MAX_LOCALIZATION_UNITS
    );
    let draft = edit(&fixture.project, &["greeting"]);
    assert_eq!(
        fixture
            .project
            .preview_localization_edit(&draft)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    let path = fixture.root.join(".world/localization/zh-Hant.json");
    let mut sidecar: serde_json::Value =
        serde_json::from_slice(fixture.project.authoring_document(&path).unwrap().bytes()).unwrap();
    sidecar["entries"]["one_more"] =
        serde_json::json!({"source_revision":"old","translation_parts":[]});
    fixture
        .project
        .set_authoring_document(&path, serde_json::to_vec(&sidecar).unwrap())
        .unwrap();
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&query)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
}

#[test]
fn physical_source_line_budget_accepts_limit_and_minus_one_but_rejects_plus_one() {
    let source = format!(
        "{}event start\r\n  Plain\r\n  -> END\r\n",
        "\r\n".repeat(MAX_LOCALIZATION_SOURCE_LINES - 3)
    );
    let mut fixture = fixture("physical-lines", &source, None);
    let request = LocalizationCatalogQuery {
        limit: 1,
        ..Default::default()
    };
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&request)
            .unwrap()
            .total,
        1
    );
    fixture
        .project
        .set_text(
            &fixture.root.join("world.wl"),
            source.trim_end_matches(['\r', '\n']).into(),
        )
        .unwrap();
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&request)
            .unwrap()
            .total,
        1,
        "no extra line is created or removed at EOF"
    );
    fixture
        .project
        .set_text(
            &fixture.root.join("world.wl"),
            source.strip_prefix("\r\n").unwrap().into(),
        )
        .unwrap();
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&request)
            .unwrap()
            .total,
        1
    );
    fixture
        .project
        .set_text(&fixture.root.join("world.wl"), format!("\n{source}"))
        .unwrap();
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&request)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
}

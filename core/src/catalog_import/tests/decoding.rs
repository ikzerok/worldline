use super::*;
use serde_json::json;
#[test]
fn catalog_import_every_field_variant_is_strict_and_round_trips() {
    let mut valid = vec![
        "kind",
        "id",
        "display",
        "entity_type",
        "description",
        "ignore",
    ]
    .into_iter()
    .map(|kind| json!({"kind":kind}))
    .collect::<Vec<_>>();
    valid.push(json!({"kind":"property","key":"score","value_type":{"kind":"number"}}));
    for value in valid {
        let decoded: CatalogImportField = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), value);
        for key in ["unknown", "target_kind"] {
            let mut invalid = value.clone();
            invalid[key] = json!(true);
            assert!(
                serde_json::from_value::<CatalogImportField>(invalid).is_err(),
                "{value} {key}"
            );
        }
        if value["kind"] != "property" {
            let mut invalid = value.clone();
            invalid["key"] = json!(null);
            assert!(serde_json::from_value::<CatalogImportField>(invalid).is_err());
        }
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("kind");
        assert!(serde_json::from_value::<CatalogImportField>(missing).is_err());
        let duplicate = value.to_string().replacen("{", "{\"kind\":\"kind\",", 1);
        assert!(serde_json::from_str::<CatalogImportField>(&duplicate).is_err());
    }
    for invalid in [
        json!({"kind":"property","key":"score"}),
        json!({"kind":"property","value_type":{"kind":"number"}}),
    ] {
        assert!(serde_json::from_value::<CatalogImportField>(invalid).is_err());
    }
    let duplicate = r#"{"kind":"property","key":"a","key":"b","value_type":{"kind":"text"}}"#;
    assert!(serde_json::from_str::<CatalogImportField>(duplicate).is_err());
}
#[test]
fn catalog_import_every_value_type_is_strict_and_round_trips() {
    let valid = [
        json!({"kind":"text"}),
        json!({"kind":"number"}),
        json!({"kind":"bool"}),
        json!({"kind":"ref","target_kind":"entity"}),
    ];
    for value in valid {
        let decoded: CatalogImportType = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), value);
        let mut invalid = value.clone();
        invalid["unknown"] = json!(true);
        assert!(serde_json::from_value::<CatalogImportType>(invalid).is_err());
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("kind");
        assert!(serde_json::from_value::<CatalogImportType>(missing).is_err());
        let duplicate = value.to_string().replacen("{", "{\"kind\":\"text\",", 1);
        assert!(serde_json::from_str::<CatalogImportType>(&duplicate).is_err());
        if value["kind"] != "ref" {
            let mut invalid = value.clone();
            invalid["target_kind"] = json!(null);
            assert!(serde_json::from_value::<CatalogImportType>(invalid).is_err());
        }
    }
    assert!(serde_json::from_str::<CatalogImportType>(r#"{"kind":"ref"}"#).is_err());
    assert!(serde_json::from_str::<CatalogImportType>(
        r#"{"kind":"ref","target_kind":"entity","target_kind":"character"}"#
    )
    .is_err());
}

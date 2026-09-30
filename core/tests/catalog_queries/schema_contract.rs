use serde_json::{json, Value};
use worldline_core::queries::{CatalogQuerySort, CatalogSortDirection, CatalogSortField};

#[test]
fn published_query_schema_tracks_versions_sort_values_and_document_capability() {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../spec/schemas/saved_query.schema.json"
    ))
    .unwrap();
    let query = &schema["properties"]["query"];
    assert_eq!(schema["properties"]["schema_version"]["const"], 1);
    assert_eq!(query["properties"]["schema_version"]["enum"], json!([1, 2]));
    assert_eq!(
        query["oneOf"][0]["properties"]["schema_version"]["const"],
        1
    );
    assert_eq!(query["oneOf"][0]["properties"]["sort"]["type"], "null");
    assert_eq!(
        query["oneOf"][1]["properties"]["schema_version"]["const"],
        2
    );
    assert_eq!(query["oneOf"][1]["required"], json!(["sort"]));
    assert_eq!(schema["$defs"]["sort"]["additionalProperties"], false);
    assert_eq!(
        schema["allOf"][0]["then"]["required"],
        json!(["required_features"])
    );
    assert_eq!(
        schema["allOf"][0]["then"]["properties"]["required_features"]["contains"]["const"],
        "catalog.query_sort.v1"
    );
    for field in [CatalogSortField::Name, CatalogSortField::Kind] {
        for direction in [
            CatalogSortDirection::Ascending,
            CatalogSortDirection::Descending,
        ] {
            let encoded = serde_json::to_value(CatalogQuerySort { field, direction }).unwrap();
            for key in ["field", "direction"] {
                assert!(schema["$defs"]["sort"]["properties"][key]["enum"]
                    .as_array()
                    .unwrap()
                    .contains(&encoded[key]));
            }
        }
    }
}

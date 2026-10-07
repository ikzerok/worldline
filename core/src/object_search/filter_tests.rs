use super::*;
use crate::catalog::{EntityInfo, TargetRef};
use crate::navigation::AliasInfo;

fn catalog(count: usize) -> Catalog {
    let mut catalog = Catalog::default();
    for index in (0..count).rev() {
        let id = format!("item_{index:04}");
        catalog.objects.push(CatalogObject {
            target: TargetRef::new("entity", &id),
            display: "同名对象 Ä".into(),
            file: "世界/深层/Places.wl".into(),
            line: index as u32 + 1,
        });
        catalog.entities.insert(
            id.clone(),
            EntityInfo {
                id,
                entity_type: if index % 2 == 0 { "place" } else { "record" }.into(),
                display: "同名对象 Ä".into(),
                description: String::new(),
                properties: Default::default(),
                file: "世界/深层/Places.wl".into(),
                line: index as u32 + 1,
            },
        );
    }
    catalog.objects.push(CatalogObject {
        target: TargetRef::new("character", "item_0000"),
        display: "同名对象 Ä".into(),
        file: "人物.wl".into(),
        line: 3,
    });
    for kind in ["entity", "character"] {
        catalog.aliases.push(AliasInfo {
            target: TargetRef::new(kind, "item_0000"),
            name: "共同别名 ÄLIAS".into(),
            file: "别名来源.wl".into(),
            line: 90,
        });
    }
    catalog
}

#[test]
fn filtered_paths_are_explicit_and_never_alias_definition_sources() {
    let catalog = catalog(2);
    let filter = ObjectSearchFilter {
        match_source_path: true,
        ..Default::default()
    };
    for query in ["世界/深层", "places.WL"] {
        assert_eq!(
            catalog
                .search_objects_filtered_page(query, &filter, Default::default())
                .unwrap()
                .total,
            2
        );
        assert_eq!(
            catalog
                .search_objects_page(query, Default::default())
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            catalog
                .search_objects_filtered_page(query, &Default::default(), Default::default())
                .unwrap()
                .total,
            0
        );
    }
    assert_eq!(
        catalog
            .search_objects_filtered_page("别名来源", &filter, Default::default())
            .unwrap()
            .total,
        0
    );
    for query in ["共同别名", "äLIAS", " item_0000 "] {
        let page = catalog
            .search_objects_filtered_page(query, &filter, Default::default())
            .unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(
            page.items[0].target,
            TargetRef::new("character", "item_0000")
        );
        assert_eq!(page.items[0].file, "人物.wl");
        assert_eq!(page.items[1].line, 1);
    }
    // Legacy unpaged semantics also stay unchanged: no kind or source matching.
    assert!(catalog.search_objects("entity").is_empty());
    assert!(catalog.search_objects("Places.wl").is_empty());
}

#[test]
fn kind_and_entity_type_are_exact_intersected_filters_without_budget_bypass() {
    let catalog = catalog(8);
    let mut filter = ObjectSearchFilter {
        allowed_kinds: vec!["entity".into()],
        entity_type: Some("place".into()),
        ..Default::default()
    };
    assert_eq!(
        catalog
            .search_objects_filtered_page("同名", &filter, Default::default())
            .unwrap()
            .total,
        4
    );
    for kind in ["character", "ENTITY", "unknown"] {
        filter.allowed_kinds = vec![kind.into()];
        assert_eq!(
            catalog
                .search_objects_filtered_page("", &filter, Default::default())
                .unwrap()
                .total,
            0
        );
    }
    filter.allowed_kinds.clear();
    for entity_type in ["Place", "", "unknown"] {
        filter.entity_type = Some(entity_type.into());
        assert_eq!(
            catalog
                .search_objects_filtered_page("", &filter, Default::default())
                .unwrap()
                .total,
            0
        );
    }
    assert_eq!(
        catalog
            .search_objects_filtered_page(
                "no-match",
                &filter,
                ObjectSearchOptions {
                    max_candidates: 8,
                    ..Default::default()
                }
            )
            .unwrap_err(),
        ObjectSearchError::CandidateBudgetExceeded {
            candidates: 9,
            budget: 8
        }
    );
}

#[test]
fn fifteen_hundred_filtered_objects_are_reachable_in_stable_complete_pages() {
    let catalog = catalog(1500);
    let filter = ObjectSearchFilter {
        allowed_kinds: vec!["entity".into()],
        ..Default::default()
    };
    let mut options = ObjectSearchOptions::default();
    let mut targets = Vec::new();
    loop {
        let page = catalog
            .search_objects_filtered_page("同名", &filter, options)
            .unwrap();
        assert_eq!(page.total, 1500);
        assert_eq!(page.items.len(), 20);
        targets.extend(page.items.into_iter().map(|object| object.target));
        let Some(next) = page.next_offset else { break };
        options.offset = next;
    }
    assert_eq!(targets.len(), 1500);
    assert!(targets.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(targets.last(), Some(&TargetRef::new("entity", "item_1499")));
    let empty = catalog
        .search_objects_filtered_page(
            "",
            &filter,
            ObjectSearchOptions {
                offset: 1500,
                ..options
            },
        )
        .unwrap();
    assert_eq!(empty.total, 1500);
    assert!(empty.items.is_empty());
    assert!(matches!(
        catalog.search_objects_filtered_page(
            "",
            &filter,
            ObjectSearchOptions {
                offset: usize::MAX,
                ..options
            }
        ),
        Err(ObjectSearchError::InvalidOffset { .. })
    ));
}

#[test]
fn fixed_input_filter_and_page_serialization_are_stable() {
    let catalog = catalog(22);
    let filter: ObjectSearchFilter = serde_json::from_str(
        r#"{"allowed_kinds":["entity"],"match_source_path":true,"entity_type":"place"}"#,
    )
    .unwrap();
    let first = catalog
        .search_objects_filtered_page("深层", &filter, Default::default())
        .unwrap();
    let mut reversed = catalog.clone();
    reversed.objects.reverse();
    reversed.aliases.reverse();
    let second = reversed
        .search_objects_filtered_page("深层", &filter, Default::default())
        .unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
    assert_eq!(
        serde_json::from_str::<ObjectSearchFilter>("{}").unwrap(),
        ObjectSearchFilter::default()
    );
    assert!(serde_json::from_str::<ObjectSearchFilter>(r#"{"match_path":true}"#).is_err());
}

#[test]
fn object_search_snapshot_does_not_refresh_or_load_new_disk_includes() {
    use crate::project::Project;
    let root = std::env::temp_dir().join(format!(
        "object-search-readonly-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut project = Project::new(&root);
    project
        .set_text(
            &project.entry.clone(),
            "include \"new.wl\"\nevent start\n  -> END\n".into(),
        )
        .unwrap();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("new.wl"), "event disk_only\n  -> END\n").unwrap();
    let before = project.content_baseline();
    let snapshot = project.compile_object_search_snapshot();
    assert!(snapshot.has_errors());
    assert!(snapshot
        .analysis
        .catalog
        .object(&TargetRef::new("event", "disk_only"))
        .is_none());
    assert_eq!(project.content_baseline(), before);
    assert_eq!(project.documents.len(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn serialized_options_use_exact_existing_defaults_and_reject_unknown_fields() {
    assert_eq!(
        serde_json::from_str::<ObjectSearchOptions>("{}").unwrap(),
        ObjectSearchOptions::default()
    );
    let partial: ObjectSearchOptions = serde_json::from_str(r#"{"offset":20}"#).unwrap();
    assert_eq!(partial.offset, 20);
    assert_eq!(partial.limit, 20);
    assert_eq!(partial.max_candidates, 10_000);
    assert!(serde_json::from_str::<ObjectSearchOptions>(r#"{"page":2}"#).is_err());
    let encoded = serde_json::to_string(&partial).unwrap();
    assert_eq!(
        serde_json::from_str::<ObjectSearchOptions>(&encoded).unwrap(),
        partial
    );
}

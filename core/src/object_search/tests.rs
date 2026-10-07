use super::*;
use crate::catalog::TargetRef;
use crate::navigation::AliasInfo;

fn catalog(count: usize) -> Catalog {
    Catalog {
        objects: (0..count)
            .rev()
            .map(|index| CatalogObject {
                target: TargetRef::new("entity", &format!("pier_{index:03}")),
                display: "第七码头 Ä".into(),
                file: "lore/places.wl".into(),
                line: index as u32 + 1,
            })
            .collect(),
        aliases: vec![AliasInfo {
            target: TargetRef::new("entity", "pier_028"),
            name: "OldQuay29 旧港 ÄLIAS".into(),
            file: "aliases.wl".into(),
            line: 90,
        }],
        ..Default::default()
    }
}

#[test]
fn aliases_unicode_ids_and_kinds_keep_identity_and_definition_source() {
    let catalog = catalog(30);
    for query in [" oldquay29 ", "旧港", "älias", "PIER_028"] {
        let page = catalog
            .search_objects_page(query, Default::default())
            .unwrap();
        assert_eq!(page.total, 1, "{query}");
        assert_eq!(page.items[0].target, TargetRef::new("entity", "pier_028"));
        assert_eq!(page.items[0].file, "lore/places.wl");
        assert_eq!(page.items[0].line, 29);
    }
    for query in ["ENTITY", "第七码头", "ä", ""] {
        assert_eq!(
            catalog
                .search_objects_page(query, Default::default())
                .unwrap()
                .total,
            30,
            "{query}"
        );
    }
    assert_eq!(
        catalog
            .search_objects_page("aliases.wl", Default::default())
            .unwrap()
            .total,
        0
    );
}

#[test]
fn full_pages_reach_every_same_name_object_in_stable_order() {
    let mut catalog = catalog(45);
    catalog.objects.push(CatalogObject {
        target: TargetRef::new("character", "pier_028"),
        display: "第七码头 Ä".into(),
        file: "people.wl".into(),
        line: 4,
    });
    let mut options = ObjectSearchOptions {
        limit: 8,
        ..Default::default()
    };
    let mut targets = Vec::new();
    loop {
        let page = catalog.search_objects_page("第七码头", options).unwrap();
        assert_eq!(page.total, 46);
        assert_eq!(page.offset, options.offset);
        assert_eq!(page.limit, 8);
        assert!(page.truncated);
        targets.extend(page.items.into_iter().map(|object| object.target));
        let Some(next) = page.next_offset else { break };
        options.offset = next;
    }
    assert_eq!(targets.len(), 46);
    assert!(targets.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(targets.contains(&TargetRef::new("entity", "pier_044")));
    assert!(targets.contains(&TargetRef::new("character", "pier_028")));
    catalog.objects.reverse();
    let first = catalog.search_objects_page("", Default::default()).unwrap();
    assert_eq!(first.items[0].target, targets[0]);
}

#[test]
fn invalid_bounds_are_explicit_and_totals_are_never_partial() {
    let catalog = catalog(30);
    for limit in [0, MAX_OBJECT_SEARCH_LIMIT + 1, usize::MAX] {
        assert!(matches!(
            catalog.search_objects_page(
                "",
                ObjectSearchOptions {
                    limit,
                    ..Default::default()
                }
            ),
            Err(ObjectSearchError::InvalidLimit { .. })
        ));
    }
    for max_candidates in [0, MAX_OBJECT_SEARCH_CANDIDATES + 1, usize::MAX] {
        assert!(matches!(
            catalog.search_objects_page(
                "",
                ObjectSearchOptions {
                    max_candidates,
                    ..Default::default()
                }
            ),
            Err(ObjectSearchError::InvalidCandidateBudget { .. })
        ));
    }
    assert_eq!(
        catalog
            .search_objects_page(
                "no match",
                ObjectSearchOptions {
                    max_candidates: 29,
                    ..Default::default()
                }
            )
            .unwrap_err(),
        ObjectSearchError::CandidateBudgetExceeded {
            candidates: 30,
            budget: 29
        }
    );
    for offset in [31, usize::MAX] {
        assert_eq!(
            catalog
                .search_objects_page(
                    "",
                    ObjectSearchOptions {
                        offset,
                        ..Default::default()
                    }
                )
                .unwrap_err(),
            ObjectSearchError::InvalidOffset { offset, total: 30 }
        );
    }
    let end = catalog
        .search_objects_page(
            "",
            ObjectSearchOptions {
                offset: 30,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(end.items.is_empty());
    assert_eq!(end.next_offset, None);
    let empty = catalog
        .search_objects_page("absent", Default::default())
        .unwrap();
    assert_eq!(empty.total, 0);
    assert!(!empty.truncated);
    assert_eq!(empty.next_offset, None);
    let whole = catalog
        .search_objects_page(
            "",
            ObjectSearchOptions {
                limit: 100,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!whole.truncated);
    assert_eq!(whole.items.len(), whole.total);
}

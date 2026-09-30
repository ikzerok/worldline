use super::CatalogQueryMatch;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSortField {
    Name,
    Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogQuerySort {
    pub field: CatalogSortField,
    pub direction: CatalogSortDirection,
}

fn identity_order(a: &CatalogQueryMatch, b: &CatalogQueryMatch) -> Ordering {
    (&a.target, &a.source.file, a.source.line).cmp(&(&b.target, &b.source.file, b.source.line))
}

pub(super) fn sort_matches(items: &mut Vec<CatalogQueryMatch>, sort: Option<CatalogQuerySort>) {
    let Some(sort) = sort else {
        items.sort_by(identity_order);
        return;
    };
    // Cache folded names once rather than allocating in each comparison.
    let mut keyed: Vec<_> = std::mem::take(items)
        .into_iter()
        .map(|item| {
            let key = match sort.field {
                CatalogSortField::Name => item.display.to_ascii_lowercase(),
                CatalogSortField::Kind => item.target.kind.clone(),
            };
            let missing = key.trim().is_empty();
            (item, key, missing)
        })
        .collect();
    keyed.sort_by(|(a, a_key, a_missing), (b, b_key, b_missing)| {
        let primary = a_missing.cmp(b_missing).then_with(|| {
            if *a_missing && *b_missing {
                return Ordering::Equal;
            }
            let order = a_key.cmp(b_key);
            if sort.direction == CatalogSortDirection::Descending {
                order.reverse()
            } else {
                order
            }
        });
        primary.then_with(|| identity_order(a, b))
    });
    *items = keyed.into_iter().map(|(item, _, _)| item).collect();
}

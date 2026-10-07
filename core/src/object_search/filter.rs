use crate::catalog::{Catalog, CatalogObject};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// 只读候选范围。空 kind 列表不限制；entity_type 存在时只允许该类型的 entity。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ObjectSearchFilter {
    pub allowed_kinds: Vec<String>,
    pub match_source_path: bool,
    pub entity_type: Option<String>,
}

impl ObjectSearchFilter {
    /// 验证已有完整对象是否属于筛选范围；不执行文本搜索或改变目录预算。
    pub fn accepts_object(&self, catalog: &Catalog, object: &CatalogObject) -> bool {
        (self.allowed_kinds.is_empty() || self.allowed_kinds.contains(&object.target.kind))
            && self.entity_type.as_ref().is_none_or(|entity_type| {
                object.target.kind == "entity"
                    && catalog
                        .entities
                        .get(&object.target.id)
                        .is_some_and(|entity| &entity.entity_type == entity_type)
            })
    }

    pub(super) fn matches<'a>(&self, catalog: &'a Catalog, query: &str) -> Vec<&'a CatalogObject> {
        let needle = query.trim().to_lowercase();
        let aliases: BTreeSet<_> = catalog
            .aliases
            .iter()
            .filter(|alias| alias.name.to_lowercase().contains(&needle))
            .map(|alias| &alias.target)
            .collect();
        catalog
            .objects
            .iter()
            .filter(|object| {
                self.accepts_object(catalog, object)
                    && (object.display.to_lowercase().contains(&needle)
                        || object.target.id.to_lowercase().contains(&needle)
                        || object.target.kind.to_lowercase().contains(&needle)
                        || aliases.contains(&object.target)
                        || (self.match_source_path && object.file.to_lowercase().contains(&needle)))
            })
            .collect()
    }
}

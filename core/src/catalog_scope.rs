//! 同一查询/编译/地图快照的临时巡检范围；不构成编辑或发布授权。
mod build;
mod relations;
use crate::catalog::TargetRef;
use crate::queries::{
    CatalogQuery, CatalogQuerySnapshot, CatalogScopeDiagnostic, QueryError, QuerySource,
};
use crate::relations::RelationDirection;
use serde::{Deserialize, Serialize};

pub const MAX_SCOPE_OBJECTS: usize = 100_000;
pub const MAX_SCOPE_PLACEMENTS: usize = 100_000;
pub const MAX_SCOPE_RELATIONS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeRole {
    Match,
    ContextOnly,
    Unresolved,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeObject {
    pub target: TargetRef,
    pub display: String,
    pub source: Option<QuerySource>,
    pub role: ScopeRole,
    pub scope_dimension: String,
    relations: Vec<usize>,
    placements: Vec<usize>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopePlacementKind {
    Placement,
    SceneNode,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopePlacement {
    pub map_id: String,
    pub map_title: String,
    pub placement_id: String,
    pub layer_id: String,
    pub kind: ScopePlacementKind,
    pub target: TargetRef,
    pub role: ScopeRole,
    pub visible: bool,
    pub locked: bool,
    pub read_only: bool,
    pub node_visible: bool,
    pub bounds: Option<[f64; 4]>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeRelation {
    pub id: String,
    pub relation_type: String,
    pub from_ref: TargetRef,
    pub to_ref: TargetRef,
    pub label: String,
    pub inverse_label: String,
    pub direction: RelationDirection,
    pub scope_refs: Vec<TargetRef>,
    pub source_note: Option<String>,
    pub source: QuerySource,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeCounts {
    pub matching_objects: usize,
    pub placed_objects: usize,
    /// 无已知绑定；只有 maps_incomplete=false 时才能认定完整未放置。
    pub unplaced_objects: usize,
    pub matching_placements: usize,
    pub unresolved_placements: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogScopeSnapshot {
    schema_version: u32,
    query: CatalogQuerySnapshot,
    objects: Vec<ScopeObject>,
    placements: Vec<ScopePlacement>,
    relations: Vec<ScopeRelation>,
    counts: ScopeCounts,
    maps_incomplete: bool,
    diagnostics: Vec<CatalogScopeDiagnostic>,
}
impl CatalogScopeSnapshot {
    pub fn query(&self) -> &CatalogQuerySnapshot {
        &self.query
    }
    pub fn counts(&self) -> &ScopeCounts {
        &self.counts
    }
    pub fn maps_incomplete(&self) -> bool {
        self.maps_incomplete
    }
    pub fn diagnostics(&self) -> &[CatalogScopeDiagnostic] {
        &self.diagnostics
    }
    pub fn placements(&self) -> &[ScopePlacement] {
        &self.placements
    }
    pub fn placement_count_for(&self, target: &TargetRef) -> usize {
        self.object(target)
            .map_or(0, |object| object.placements.len())
    }
    pub fn placements_page(
        &self,
        target: &TargetRef,
        offset: usize,
        limit: usize,
    ) -> impl Iterator<Item = &ScopePlacement> {
        self.object(target)
            .into_iter()
            .flat_map(move |object| object.placements.iter().skip(offset).take(limit.min(100)))
            .filter_map(|index| self.placements.get(*index))
    }
    pub fn placements_for(&self, target: &TargetRef) -> impl Iterator<Item = &ScopePlacement> {
        self.object(target)
            .into_iter()
            .flat_map(|object| &object.placements)
            .filter_map(|index| self.placements.get(*index))
    }
    pub fn object(&self, target: &TargetRef) -> Option<&ScopeObject> {
        self.objects
            .binary_search_by(|object| object.target.cmp(target))
            .ok()
            .map(|index| &self.objects[index])
    }
    pub fn role(&self, target: &TargetRef) -> ScopeRole {
        self.object(target)
            .map_or(ScopeRole::Unresolved, |object| object.role)
    }
    pub fn validate_for(
        &self,
        query: &CatalogQuery,
        baseline: &str,
        max_candidates: usize,
    ) -> Result<(), QueryError> {
        use crate::queries::snapshot::{budget_error, encoded_size};
        self.query.validate_wire()?;
        if self.schema_version != 1
            || self.query.snapshot != baseline
            || self.query.max_candidates != max_candidates
            || !self.query.matches_query(query)
            || self.objects.len() > MAX_SCOPE_OBJECTS
            || self.placements.len() > MAX_SCOPE_PLACEMENTS
            || self.relations.len() > MAX_SCOPE_RELATIONS
            || self
                .diagnostics
                .len()
                .saturating_add(self.query.diagnostics.len())
                > crate::queries::MAX_CATALOG_SNAPSHOT_DIAGNOSTICS
            || self.counts.matching_objects != self.query.total()
            || self
                .counts
                .placed_objects
                .checked_add(self.counts.unplaced_objects)
                != Some(self.query.total())
            || !self
                .objects
                .windows(2)
                .all(|pair| pair[0].target < pair[1].target)
        {
            return Err(budget_error("范围身份、数量或次序无效"));
        }
        for (index, relation) in self.relations.iter().enumerate() {
            if [&relation.from_ref, &relation.to_ref].iter().any(|target| {
                self.object(target)
                    .is_none_or(|object| object.relations.binary_search(&index).is_err())
            }) {
                return Err(budget_error("正式关系端点索引无效"));
            }
        }
        for object in &self.objects {
            if object.relations.iter().any(|index| {
                self.relations.get(*index).is_none_or(|edge| {
                    edge.from_ref != object.target && edge.to_ref != object.target
                })
            }) {
                return Err(budget_error("正式关系邻接索引无效"));
            }
        }
        encoded_size(self, crate::queries::MAX_CATALOG_SNAPSHOT_BYTES).map(|_| ())
    }
}

#[cfg(test)]
mod performance_tests;
#[cfg(test)]
mod tests;

impl crate::project::Project {
    /// 仅取上次载入/刷新记录的会话观测；无 IO，不是当前磁盘签名。
    /// 缓存必须与内容基线/本地修改代次一起绑定，不能作为写入授权。
    pub fn catalog_scope_observation_key(&self) -> String {
        self.manuscript_observation_key()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod observation_tests;

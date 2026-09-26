//! 基于当前内容与展示快照的删除影响计划；查询不修改任何文档。

use crate::catalog::{ReferenceInfo, TargetRef};
use crate::deletion_content_references::{affected_by_deletion, content_deletion_references};
use crate::presentation::{build_map_index, MapIndex, MapPlacementRef};
use crate::project::Project;
use crate::{CompileResult, Diagnostic, Severity};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MapRasterRef {
    pub map_id: String,
    pub raster_layer_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeletionImpact {
    pub target: TargetRef,
    pub target_exists: bool,
    pub content_references: Vec<ReferenceInfo>,
    pub map_placements: Vec<MapPlacementRef>,
    pub map_scopes: Vec<MapPlacementRef>,
    pub map_rasters: Vec<MapRasterRef>,
    pub graph_views: Vec<crate::graph_views::GraphViewReference>,
    pub comments: Vec<crate::collaboration::CommentReference>,
    /// 有错误的内容、地图或协作文档可能隐藏引用，不能将部分结果当作无引用。
    pub complete: bool,
    pub diagnostics: Vec<Diagnostic>,
}

impl DeletionImpact {
    pub fn can_delete(&self) -> bool {
        self.target_exists
            && self.complete
            && self.content_references.is_empty()
            && self.map_placements.is_empty()
            && self.map_scopes.is_empty()
            && self.map_rasters.is_empty()
            && self.graph_views.is_empty()
            && self.comments.is_empty()
    }
}

/// 仅检查内容和地图；有网络视图的工作区应调用 deletion_impact_with_views 或 Project 方法。
pub fn deletion_impact(
    content: &CompileResult,
    maps: &MapIndex,
    target: &TargetRef,
) -> DeletionImpact {
    let mut diagnostics = content
        .diagnostics
        .iter()
        .chain(&maps.diagnostics)
        .cloned()
        .collect::<Vec<_>>();
    crate::diagnostic::sort_diagnostics(&mut diagnostics);
    let map_scopes = maps
        .maps
        .iter()
        .flat_map(|(map_id, map)| {
            map.placements
                .values()
                .filter(|placement| {
                    placement
                        .scope_refs
                        .iter()
                        .any(|reference| affected_by_deletion(reference, target))
                })
                .map(|placement| MapPlacementRef {
                    map_id: map_id.clone(),
                    placement_id: placement.id.clone(),
                })
        })
        .collect();
    let map_rasters = maps
        .maps
        .iter()
        .flat_map(|(map_id, map)| {
            map.raster_layers
                .iter()
                .filter(|layer| &layer.asset == target)
                .map(|layer| MapRasterRef {
                    map_id: map_id.clone(),
                    raster_layer_id: layer.id.clone(),
                })
        })
        .collect();
    DeletionImpact {
        target: target.clone(),
        target_exists: content.analysis.catalog.object(target).is_some(),
        content_references: content_deletion_references(content, target),
        map_placements: maps
            .placements_by_target
            .iter()
            .filter(|(reference, _)| affected_by_deletion(reference, target))
            .flat_map(|(_, placements)| placements.iter().cloned())
            .collect(),
        map_scopes,
        map_rasters,
        graph_views: Vec::new(),
        comments: Vec::new(),
        complete: diagnostics
            .iter()
            .all(|item| item.severity != Severity::Error),
        diagnostics,
    }
}

/// 使用同一内容、地图和网络视图快照，避免 UI 每帧重新编译。
pub fn deletion_impact_with_views(
    content: &CompileResult,
    maps: &MapIndex,
    views: &crate::graph_views::GraphViewIndex,
    target: &TargetRef,
) -> DeletionImpact {
    let mut impact = deletion_impact(content, maps, target);
    impact.graph_views = views.references_to(target);
    for diagnostic in &views.diagnostics {
        if !impact.diagnostics.iter().any(|existing| {
            existing.code == diagnostic.code
                && existing.file == diagnostic.file
                && existing.span == diagnostic.span
                && existing.severity == diagnostic.severity
                && existing.message == diagnostic.message
                && existing.note == diagnostic.note
                && existing.suggestion == diagnostic.suggestion
                && existing.related == diagnostic.related
        }) {
            impact.diagnostics.push(diagnostic.clone());
        }
    }
    crate::diagnostic::sort_diagnostics(&mut impact.diagnostics);
    impact.complete = impact
        .diagnostics
        .iter()
        .all(|item| item.severity != Severity::Error);
    impact
}

/// 使用同一内容、地图、网络视图和批注快照生成完整删除影响。
pub fn deletion_impact_with_collaboration(
    content: &CompileResult,
    maps: &MapIndex,
    views: &crate::graph_views::GraphViewIndex,
    comments: &crate::collaboration::CommentIndex,
    target: &TargetRef,
) -> DeletionImpact {
    let mut impact = deletion_impact_with_views(content, maps, views, target);
    impact.comments = comments.references_to(target);
    for diagnostic in &comments.diagnostics {
        if !impact.diagnostics.iter().any(|existing| {
            existing.code == diagnostic.code
                && existing.file == diagnostic.file
                && existing.span == diagnostic.span
                && existing.severity == diagnostic.severity
                && existing.message == diagnostic.message
                && existing.note == diagnostic.note
                && existing.suggestion == diagnostic.suggestion
                && existing.related == diagnostic.related
        }) {
            impact.diagnostics.push(diagnostic.clone());
        }
    }
    crate::diagnostic::sort_diagnostics(&mut impact.diagnostics);
    impact.complete = impact
        .diagnostics
        .iter()
        .all(|item| item.severity != Severity::Error);
    impact
}

impl Project {
    /// 返回当前缓冲的影响计划；调用删除命令时仍须重新检查，不能用旧计划授权写入。
    pub fn deletion_impact(&self, target: &TargetRef) -> DeletionImpact {
        let content = self.compile_current();
        let maps = build_map_index(self, &content);
        let views = crate::graph_views::build_graph_view_index(self, &content);
        let comments = crate::collaboration::build_comment_index(self, &content, &maps);
        deletion_impact_with_collaboration(&content, &maps, &views, &comments, target)
    }
}

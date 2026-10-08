use super::*;
use crate::project::Project;
use crate::queries::snapshot::{budget_error, check_clone_bytes, encoded_size};
use crate::queries::{MAX_CATALOG_SNAPSHOT_BYTES, MAX_CATALOG_SNAPSHOT_DIAGNOSTICS};
use crate::Severity;
use std::collections::{BTreeMap, BTreeSet};

impl Project {
    pub fn catalog_scope_snapshot(
        &self,
        query: &CatalogQuery,
        max_candidates: usize,
    ) -> Result<CatalogScopeSnapshot, QueryError> {
        self.catalog_scope_snapshot_cancellable(query, max_candidates, || false)
    }
    pub fn catalog_scope_snapshot_cancellable(
        &self,
        query: &CatalogQuery,
        max_candidates: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<CatalogScopeSnapshot, QueryError> {
        query.validate(&self.root)?;
        if !(1..=crate::queries::MAX_CATALOG_QUERY_CANDIDATES).contains(&max_candidates) {
            return Err(budget_error("候选上限必须在 1–100,000 之间"));
        }
        if cancelled() {
            return Err(QueryError::Cancelled);
        }
        let baseline = self.content_baseline();
        let content = self.compile_problems_snapshot();
        let query = CatalogQuerySnapshot::from_content(
            &self.root,
            baseline,
            &content,
            query,
            max_candidates,
            &mut cancelled,
        )?;
        if cancelled() {
            return Err(QueryError::Cancelled);
        }
        let mut maps = crate::presentation::build_map_index(self, &content);
        // Resolve source paths once for this build, with the registry's ordinary
        // boundary/identity checks; never reparse the manifest once per map.
        let registry = self
            .authoring_document(&crate::workspace_documents::manifest_path(&self.root))
            .ok()
            .filter(|document| !document.is_deleted())
            .map(|document| {
                crate::workspace_documents::parse_registry(&self.root, document.bytes())
            })
            .unwrap_or_default();
        for diagnostic in &registry.diagnostics {
            if !maps.diagnostics.iter().any(|old| {
                old.code == diagnostic.code
                    && old.severity == diagnostic.severity
                    && old.file == diagnostic.file
                    && old.span == diagnostic.span
                    && old.message == diagnostic.message
            }) {
                maps.diagnostics.push(diagnostic.clone());
            }
        }
        let mut bytes = encoded_size(&query, MAX_CATALOG_SNAPSHOT_BYTES)?;
        let catalog = &content.analysis.catalog;
        if catalog.relations.len() > MAX_SCOPE_RELATIONS {
            return Err(budget_error("正式关系超过 100,000"));
        }
        let matched = query
            .matches()
            .iter()
            .map(|item| item.target.clone())
            .collect::<BTreeSet<_>>();
        let mut objects = BTreeMap::<TargetRef, ScopeObject>::new();
        for (index, object) in catalog.objects.iter().enumerate() {
            check_cancel(index, &mut cancelled)?;
            encoded_size(object, MAX_CATALOG_SNAPSHOT_BYTES.saturating_sub(bytes))?;
            let item = ScopeObject {
                target: object.target.clone(),
                display: object.display.clone(),
                source: Some(QuerySource {
                    file: object.file.clone(),
                    line: object.line,
                }),
                role: if matched.contains(&object.target) {
                    ScopeRole::Match
                } else {
                    ScopeRole::ContextOnly
                },
                scope_dimension: if object.target.kind == "entity"
                    && catalog
                        .entities
                        .get(&object.target.id)
                        .is_some_and(|entity| entity.entity_type == "version")
                {
                    "version".into()
                } else if matches!(object.target.kind.as_str(), "event" | "scene") {
                    "story".into()
                } else {
                    object.target.kind.clone()
                },
                relations: Vec::new(),
                placements: Vec::new(),
            };
            account_item(&item, &mut bytes)?;
            objects.entry(object.target.clone()).or_insert(item);
        }
        let mut relations = Vec::new();
        for relation in catalog.relations.values() {
            check_cancel(relations.len(), &mut cancelled)?;
            let index = relations.len();
            for target in [&relation.from_ref, &relation.to_ref] {
                ensure_object(&mut objects, target, &mut bytes)?;
                let entry = objects.get_mut(target).expect("ensured endpoint");
                if entry.relations.last() != Some(&index) {
                    entry.relations.push(index);
                    bytes += 8;
                    if bytes > MAX_CATALOG_SNAPSHOT_BYTES {
                        return Err(budget_error("邻接索引超过字节预算"));
                    }
                }
            }
            let kind = catalog.relation_types.get(&relation.relation_type);
            let raw_label = kind.map_or(relation.relation_type.as_str(), |kind| {
                kind.display.as_str()
            });
            let raw_inverse = kind
                .and_then(|kind| kind.inverse_display.as_deref())
                .unwrap_or(raw_label);
            check_clone_bytes(
                [
                    relation.id.as_str(),
                    relation.relation_type.as_str(),
                    relation.from_ref.kind.as_str(),
                    relation.from_ref.id.as_str(),
                    relation.to_ref.kind.as_str(),
                    relation.to_ref.id.as_str(),
                    raw_label,
                    raw_inverse,
                    relation.file.as_str(),
                    relation.source_note.as_deref().unwrap_or(""),
                ]
                .into_iter()
                .chain(
                    relation
                        .scope_refs
                        .iter()
                        .flat_map(|target| [target.kind.as_str(), target.id.as_str()]),
                ),
                MAX_CATALOG_SNAPSHOT_BYTES.saturating_sub(bytes),
            )?;
            let label = raw_label.to_owned();
            let item = ScopeRelation {
                id: relation.id.clone(),
                relation_type: relation.relation_type.clone(),
                from_ref: relation.from_ref.clone(),
                to_ref: relation.to_ref.clone(),
                inverse_label: kind
                    .and_then(|kind| kind.inverse_display.clone())
                    .unwrap_or_else(|| label.clone()),
                label,
                direction: kind.map_or(RelationDirection::Directed, |kind| kind.direction),
                scope_refs: relation.scope_refs.clone(),
                source_note: relation.source_note.clone(),
                source: QuerySource {
                    file: relation.file.clone(),
                    line: relation.line,
                },
            };
            account_item(&item, &mut bytes)?;
            relations.push(item);
        }
        let mut placements = Vec::new();
        let mut examined = 0;
        for map in maps.maps.values() {
            let read_only = registry
                .maps
                .get(&map.id)
                .and_then(|path| self.authoring_document(path).ok())
                .is_none_or(|document| document.is_read_only());
            for placement in map.placements.values() {
                let Some(target) = &placement.target_ref else {
                    continue;
                };
                examined += 1;
                check_binding_count(examined, &mut cancelled)?;
                let role = objects
                    .get(target)
                    .map_or(ScopeRole::Unresolved, |object| object.role);
                if role == ScopeRole::ContextOnly {
                    continue;
                }
                ensure_object(&mut objects, target, &mut bytes)?;
                let layer = map.layers.get(&placement.layer_id);
                check_clone_bytes(
                    [
                        map.id.as_str(),
                        map.title.as_str(),
                        placement.id.as_str(),
                        placement.layer_id.as_str(),
                        target.kind.as_str(),
                        target.id.as_str(),
                    ],
                    MAX_CATALOG_SNAPSHOT_BYTES.saturating_sub(bytes),
                )?;
                let item = ScopePlacement {
                    map_id: map.id.clone(),
                    map_title: map.title.clone(),
                    placement_id: placement.id.clone(),
                    layer_id: placement.layer_id.clone(),
                    kind: ScopePlacementKind::Placement,
                    target: target.clone(),
                    role,
                    visible: layer.is_some_and(|layer| layer.visible_default),
                    locked: layer.is_none_or(|layer| layer.locked),
                    read_only,
                    node_visible: true,
                    bounds: normalized_bounds(placement.geometry.points()),
                };
                account_item(&item, &mut bytes)?;
                objects
                    .get_mut(target)
                    .expect("ensured binding")
                    .placements
                    .push(placements.len());
                placements.push(item);
            }
            if let Some(scene) = &map.scene {
                for node in scene.nodes.values() {
                    let Some(target) = &node.target_ref else {
                        continue;
                    };
                    examined += 1;
                    check_binding_count(examined, &mut cancelled)?;
                    let role = objects
                        .get(target)
                        .map_or(ScopeRole::Unresolved, |object| object.role);
                    if role == ScopeRole::ContextOnly {
                        continue;
                    }
                    ensure_object(&mut objects, target, &mut bytes)?;
                    let state = crate::vector_scene::node_state(scene, &node.id)
                        .map_err(|error| budget_error(&error.message))?;
                    let layer = map.layers.get(&node.layer_id);
                    check_clone_bytes(
                        [
                            map.id.as_str(),
                            map.title.as_str(),
                            node.id.as_str(),
                            node.layer_id.as_str(),
                            target.kind.as_str(),
                            target.id.as_str(),
                        ],
                        MAX_CATALOG_SNAPSHOT_BYTES.saturating_sub(bytes),
                    )?;
                    let item = ScopePlacement {
                        map_id: map.id.clone(),
                        map_title: map.title.clone(),
                        placement_id: node.id.clone(),
                        layer_id: node.layer_id.clone(),
                        kind: ScopePlacementKind::SceneNode,
                        target: target.clone(),
                        role,
                        visible: state.visible && layer.is_some_and(|layer| layer.visible_default),
                        locked: state.locked || layer.is_none_or(|layer| layer.locked),
                        read_only,
                        node_visible: state.visible,
                        bounds: None,
                    };
                    account_item(&item, &mut bytes)?;
                    objects
                        .get_mut(target)
                        .expect("ensured binding")
                        .placements
                        .push(placements.len());
                    placements.push(item);
                }
            }
        }
        let placed = placements
            .iter()
            .filter(|p| p.role == ScopeRole::Match)
            .map(|p| &p.target)
            .collect::<BTreeSet<_>>();
        let placed_objects = query
            .matches()
            .iter()
            .filter(|item| placed.contains(&item.target))
            .count();
        let counts = ScopeCounts {
            matching_objects: query.total(),
            placed_objects,
            unplaced_objects: query.total() - placed_objects,
            matching_placements: placements
                .iter()
                .filter(|p| p.role == ScopeRole::Match)
                .count(),
            unresolved_placements: placements
                .iter()
                .filter(|p| p.role == ScopeRole::Unresolved)
                .count(),
        };
        if maps
            .diagnostics
            .len()
            .saturating_add(query.diagnostics.len())
            > MAX_CATALOG_SNAPSHOT_DIAGNOSTICS
        {
            return Err(budget_error("地图诊断超过 10,000"));
        }
        let mut diagnostics = Vec::new();
        for diagnostic in &maps.diagnostics {
            encoded_size(diagnostic, MAX_CATALOG_SNAPSHOT_BYTES.saturating_sub(bytes))?;
            let item = CatalogScopeDiagnostic::from(diagnostic);
            account_item(&item, &mut bytes)?;
            diagnostics.push(item);
        }
        let snapshot = CatalogScopeSnapshot {
            schema_version: 1,
            query,
            objects: objects.into_values().collect(),
            placements,
            relations,
            counts,
            maps_incomplete: diagnostics.iter().any(|d| d.severity == Severity::Error),
            diagnostics,
        };
        if cancelled() {
            return Err(QueryError::Cancelled);
        }
        encoded_size(&snapshot, MAX_CATALOG_SNAPSHOT_BYTES)?;
        Ok(snapshot)
    }
}
fn account_item(value: &impl Serialize, bytes: &mut usize) -> Result<(), QueryError> {
    *bytes += encoded_size(value, MAX_CATALOG_SNAPSHOT_BYTES.saturating_sub(*bytes))?;
    Ok(())
}
fn ensure_object(
    objects: &mut BTreeMap<TargetRef, ScopeObject>,
    target: &TargetRef,
    bytes: &mut usize,
) -> Result<(), QueryError> {
    if !objects.contains_key(target) {
        if objects.len() >= MAX_SCOPE_OBJECTS {
            return Err(budget_error("对象含未解析端点超过 100,000"));
        }
        check_clone_bytes(
            [
                target.kind.as_str(),
                target.kind.as_str(),
                target.id.as_str(),
                target.id.as_str(),
            ],
            MAX_CATALOG_SNAPSHOT_BYTES.saturating_sub(*bytes),
        )?;
        let object = ScopeObject {
            target: target.clone(),
            display: target.id.clone(),
            source: None,
            role: ScopeRole::Unresolved,
            scope_dimension: target.kind.clone(),
            relations: Vec::new(),
            placements: Vec::new(),
        };
        account_item(&object, bytes)?;
        objects.insert(target.clone(), object);
    }
    Ok(())
}
fn check_cancel(index: usize, cancelled: &mut impl FnMut() -> bool) -> Result<(), QueryError> {
    if index.is_multiple_of(64) && cancelled() {
        Err(QueryError::Cancelled)
    } else {
        Ok(())
    }
}
fn check_binding_count(
    count: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), QueryError> {
    if count > MAX_SCOPE_PLACEMENTS {
        return Err(budget_error("地图绑定超过 100,000"));
    }
    check_cancel(count, cancelled)
}

fn normalized_bounds(points: &[[f64; 2]]) -> Option<[f64; 4]> {
    let first = points.first()?;
    let mut bounds = [first[0], first[1], first[0], first[1]];
    for point in points {
        bounds[0] = bounds[0].min(point[0]);
        bounds[1] = bounds[1].min(point[1]);
        bounds[2] = bounds[2].max(point[0]);
        bounds[3] = bounds[3].max(point[1]);
    }
    Some(bounds)
}

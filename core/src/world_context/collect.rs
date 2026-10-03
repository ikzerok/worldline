use super::*;
use crate::{ast::{Property, PropertyValue}, catalog::CatalogDecl, wiki::KeywordIndex, CompileResult};

/// 按现有有类型资料投影；callback(None) 为扫描期间的取消检查点。
pub(super) fn visit(
    result: &CompileResult, current: &TargetRef, wiki: Option<&KeywordIndex>,
    mut callback: impl FnMut(Option<WorldContextRecord>) -> bool,
) -> bool {
    let catalog = &result.analysis.catalog;
    for id in catalog.relation_index.get(current).into_iter().flatten() {
        let Some(relation) = catalog.relations.get(id) else { continue };
        let info = catalog.relation_types.get(&relation.relation_type);
        let mut record = record(
            WorldContextKind::FormalRelation, &relation.from_ref, &relation.to_ref,
            &relation.file, relation.line, None, 0,
            info.map(|info| info.display.clone()).unwrap_or_else(|| relation.relation_type.clone()),
            WorldContextProvenance::FormalRelation {
                relation_id: relation.id.clone(), relation_type: relation.relation_type.clone(),
            },
        );
        record.id = format!("relation:{}", relation.id);
        record.identity = WorldContextIdentity::PersistentRelation;
        record.direction = info.map(|info| info.direction).unwrap_or_default();
        if !callback(Some(record)) { return false; }
    }
    for relation in &catalog.legacy_relations {
        let handle = &relation.handle;
        if !callback(None) { return false; }
        if current != &handle.source && current != &handle.target { continue; }
        if !callback(Some(record(
            WorldContextKind::LegacyCharacterRelation, &handle.source, &handle.target,
            &handle.file, handle.line, None, handle.occurrence as usize, handle.label.clone(),
            WorldContextProvenance::LegacyCharacterRelation { occurrence: handle.occurrence },
        ))) { return false; }
    }
    let program = &result.program;
    let properties = program.worlds.first().into_iter().map(|world| (
        TargetRef::new("world", &world.name), world.file.as_str(), world.properties.as_slice(),
    )).chain(program.characters.iter().map(|object| (
        TargetRef::new("character", &object.name), object.file.as_str(), object.properties.as_slice(),
    ))).chain(program.entities.iter().map(|object| (
        TargetRef::new("entity", &object.name), object.file.as_str(), object.properties.as_slice(),
    ))).chain(program.relations.iter().map(|object| (
        TargetRef::new("relation", &object.id), object.file.as_str(), object.properties.as_slice(),
    ))).chain(program.catalog.iter().filter_map(|declaration| match declaration {
        CatalogDecl::Tag(object) => Some((TargetRef::new("tag", &object.name), object.file.as_str(), object.properties.as_slice())),
        CatalogDecl::Anchor(object) => Some((TargetRef::new("anchor", &object.name), object.file.as_str(), object.properties.as_slice())),
        _ => None,
    }));
    for (source, file, properties) in properties {
        if !visit_properties(current, &source, file, properties, &mut callback) { return false; }
    }
    // 使用与 topic/history 相同的事件人物投影；保留原事件头位置。
    for node in result.analysis.graph.nodes.iter().filter(|node| node.is_event) {
        for (occurrence, character) in node.characters.iter().enumerate() {
            if !callback(None) { return false; }
            let source = TargetRef::new("event", &node.name);
            let target = TargetRef::new("character", character);
            if current != &source && current != &target { continue; }
            if !callback(Some(record(
                WorldContextKind::EventParticipation, &source, &target,
                &node.file, node.line, None, occurrence, "参与".into(),
                WorldContextProvenance::EventParticipation { event: node.name.clone() },
            ))) { return false; }
        }
    }
    for (occurrence, link) in catalog.text_links.iter().enumerate() {
        if !callback(None) { return false; }
        if current != &link.source && current != &link.target { continue; }
        if !callback(Some(record(
            WorldContextKind::ExplicitBodyLink, &link.source, &link.target,
            &link.file, link.line, None, occurrence, "显式链接".into(),
            WorldContextProvenance::ExplicitBodyLink { label: link.label.clone() },
        ))) { return false; }
    }
    if let Some(wiki) = wiki {
        for (occurrence, hit) in wiki.occurrences(current).iter().enumerate() {
            let file = hit.file.to_string_lossy();
            let source = TargetRef::new("file", &file);
            if !callback(Some(record(
                WorldContextKind::TextMention, &source, current,
                &file, hit.line, Some(hit.column), occurrence, "文字提及".into(),
                WorldContextProvenance::TextMention { preview: hit.preview.clone() },
            ))) { return false; }
        }
    }
    true
}

fn visit_properties(
    current: &TargetRef, source: &TargetRef, file: &str, properties: &[Property],
    callback: &mut impl FnMut(Option<WorldContextRecord>) -> bool,
) -> bool {
    for (occurrence, property) in properties.iter().enumerate() {
        if !callback(None) { return false; }
        let PropertyValue::Ref(target) = &property.value else { continue; };
        if current != source && current != target { continue; }
        if !callback(Some(record(
            WorldContextKind::PropertyReference, source, target, file, property.loc.line,
            Some(property.loc.column), occurrence, property.name.clone(),
            WorldContextProvenance::PropertyReference { property: property.name.clone() },
        ))) { return false; }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn record(
    kind: WorldContextKind, from_ref: &TargetRef, to_ref: &TargetRef,
    file: &str, line: u32, column: Option<u32>, occurrence: usize,
    role: String, provenance: WorldContextProvenance,
) -> WorldContextRecord {
    let identity = serde_json::to_vec(&(kind, from_ref, to_ref, file, line, column, occurrence, &provenance))
        .expect("上下文身份可序列化");
    WorldContextRecord {
        id: format!("occurrence:{}", digest([identity])),
        identity: WorldContextIdentity::SnapshotOccurrence,
        kind, from_ref: from_ref.clone(), to_ref: to_ref.clone(),
        direction: RelationDirection::Directed, role, provenance,
        source: WorldContextSource {
            file: file.into(), line, column,
            precision: if column.is_some() { WorldContextPrecision::Column } else { WorldContextPrecision::Line },
        },
    }
}

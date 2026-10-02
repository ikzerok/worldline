use super::site::{html_escape, relative_url};
use super::*;

pub(super) fn kind_label(kind: &str) -> &str {
    match kind {
        "event" => "事件",
        "fragment" => "片段",
        "rule" => "规则",
        "scene" => "场景",
        "character" => "人物",
        "entity" => "实体",
        "world" => "世界",
        "storyline" => "故事线",
        "period" => "时段",
        "anchor" => "锚点",
        "state" => "状态",
        "tag" => "标签",
        "relation" => "关系",
        "variable" => "变量",
        "map" => "地图",
        "chapter" => "章节",
        "timeline" => "时间结构",
        "relations" => "关系目录",
        "stories" => "故事目录",
        _ => kind,
    }
}

pub(super) fn link(
    compiled: &CompileResult,
    routes: &BTreeMap<TargetRef, String>,
    current: &str,
    target: &TargetRef,
) -> (String, String) {
    if let Some((route, object)) = routes
        .get(target)
        .zip(compiled.analysis.catalog.object(target))
    {
        (
            format!(
                "<a href=\"{}\">{}</a>",
                html_escape(&relative_url(current, route)),
                html_escape(&object.display)
            ),
            object.display.clone(),
        )
    } else {
        (
            "<span class=\"unavailable\">未公开内容</span>".into(),
            "未公开内容".into(),
        )
    }
}

pub(super) fn entry(page: &mut PublicPage, label: &str, html: &str, plain: &str) {
    page.body_html.push_str(&format!(
        "<dl><dt>{}</dt><dd>{html}</dd></dl>",
        html_escape(label)
    ));
    page.searchable_text
        .push_str(&format!("\n{label}：{plain}\n"));
}

pub(super) fn append_object(
    compiled: &CompileResult,
    target: &TargetRef,
    routes: &BTreeMap<TargetRef, String>,
    public_field_references: &BTreeSet<(TargetRef, TargetRef)>,
    page: &mut PublicPage,
) -> Result<(), String> {
    let current = page.output_path.to_string_lossy().into_owned();
    entry(
        page,
        "类型",
        &html_escape(kind_label(&target.kind)),
        kind_label(&target.kind),
    );
    if !page.aliases.is_empty() {
        let aliases = page.aliases.join("、");
        entry(page, "别名", &html_escape(&aliases), &aliases);
    }
    let mut references = Vec::<(String, TargetRef)>::new();
    match target.kind.as_str() {
        "entity" => {
            if let Some(entity) = compiled.analysis.catalog.entities.get(&target.id) {
                entry(
                    page,
                    "实体类别",
                    &html_escape(&entity.entity_type),
                    &entity.entity_type,
                );
            }
        }
        "state" => {
            if let Some(state) = compiled.analysis.catalog.states.get(&target.id) {
                references.push(("所属对象".into(), state.target.clone()));
            }
        }
        "anchor" => {
            if let Some(anchor) = compiled.analysis.catalog.anchors.get(&target.id) {
                references.extend(
                    anchor
                        .links
                        .iter()
                        .map(|link| ("关联对象".into(), link.target.clone())),
                );
            }
        }
        "variable" => {
            if let Some(variable) = compiled.analysis.symbols.vars.get(&target.id) {
                let kind = variable.kind.map(|kind| kind.label()).unwrap_or("动态类型");
                entry(page, "值类型", kind, kind);
            }
        }
        "event" => {
            if let Some(event) = compiled.program.events.iter().find(|e| e.name == target.id) {
                if let Some(period) = &event.period {
                    references.push(("时段".into(), TargetRef::new("period", period)));
                }
                references.push((
                    "故事线".into(),
                    TargetRef::new("storyline", &event.storyline),
                ));
                references.extend(
                    event
                        .characters
                        .iter()
                        .map(|id| ("人物".into(), TargetRef::new("character", id))),
                );
                references.extend(
                    event
                        .predecessors
                        .iter()
                        .map(|id| ("显式前置事件".into(), TargetRef::new("event", id))),
                );
            }
        }
        "period" => {
            if let Some(parent) = compiled
                .program
                .periods
                .iter()
                .find(|p| p.name == target.id)
                .and_then(|p| p.parent.as_ref())
            {
                references.push(("父时段".into(), TargetRef::new("period", parent)));
            }
            references.extend(
                compiled
                    .program
                    .periods
                    .iter()
                    .filter(|p| p.parent.as_deref() == Some(&target.id))
                    .filter(|p| routes.contains_key(&TargetRef::new("period", &p.name)))
                    .map(|p| ("子时段".into(), TargetRef::new("period", &p.name))),
            );
            references.extend(
                compiled
                    .program
                    .events
                    .iter()
                    .filter(|e| e.period.as_deref() == Some(&target.id))
                    .filter(|e| routes.contains_key(&TargetRef::new("event", &e.name)))
                    .map(|e| ("成员事件".into(), TargetRef::new("event", &e.name))),
            );
        }
        "relation" => {
            if let Some(relation) = compiled.analysis.catalog.relations.get(&target.id) {
                if let Some(kind) = compiled
                    .analysis
                    .catalog
                    .relation_types
                    .get(&relation.relation_type)
                {
                    entry(page, "关系类型", &html_escape(&kind.display), &kind.display);
                    let direction = match kind.direction {
                        crate::relations::RelationDirection::Directed => "有向",
                        crate::relations::RelationDirection::Undirected => "无向",
                    };
                    entry(page, "方向", direction, direction);
                }
                references.push(("起点".into(), relation.from_ref.clone()));
                references.push(("终点".into(), relation.to_ref.clone()));
                references.extend(
                    relation
                        .scope_refs
                        .iter()
                        .cloned()
                        .map(|target| ("适用范围".into(), target)),
                );
            }
        }
        _ => {}
    }
    for (label, reference) in references {
        let (html, text) = link(compiled, routes, &current, &reference);
        entry(page, &label, &html, &text);
    }
    let mut related = BTreeSet::new();
    for reference in &compiled.analysis.catalog.text_links {
        if reference.target == *target {
            if let Some(source) = public_source(&reference.source, routes) {
                related.insert(source);
            }
        }
    }
    for (source, reference) in public_field_references {
        if reference == target && routes.contains_key(source) {
            related.insert(source.clone());
        }
    }
    related.extend(structural_backlinks(compiled, target, routes));
    for relation in compiled.analysis.catalog.relations.values() {
        if relation.from_ref != *target
            && relation.to_ref != *target
            && !relation.scope_refs.contains(target)
        {
            continue;
        }
        let relation_target = TargetRef::new("relation", &relation.id);
        if routes.contains_key(&relation_target) {
            related.insert(relation_target);
        }
    }
    related.remove(target);
    for reference in related {
        let (html, text) = link(compiled, routes, &current, &reference);
        entry(page, "相关公开内容", &html, &text);
    }
    for relation in &compiled.analysis.catalog.legacy_relations {
        let relation = &relation.handle;
        if relation.source == *target && routes.contains_key(&relation.target) {
            let (html, text) = link(compiled, routes, &current, &relation.target);
            entry(page, &relation.label, &html, &text);
        }
    }
    let graph = super::relation_graph::relation_graph(compiled, routes, Some(target), &current);
    if !graph.html.is_empty() {
        page.body_html.push_str(&graph.html);
        page.searchable_text.push_str(&graph.text);
    }
    Ok(())
}

fn structural_backlinks(
    compiled: &CompileResult,
    target: &TargetRef,
    routes: &BTreeMap<TargetRef, String>,
) -> BTreeSet<TargetRef> {
    let mut related = BTreeSet::new();
    for event in &compiled.program.events {
        let references_target = match target.kind.as_str() {
            "character" => event.characters.contains(&target.id),
            "period" => event.period.as_deref() == Some(target.id.as_str()),
            "storyline" => event.storyline == target.id,
            "event" => event.predecessors.contains(&target.id),
            _ => false,
        };
        if references_target {
            let source = TargetRef::new("event", &event.name);
            if routes.contains_key(&source) {
                related.insert(source);
            }
        }
    }
    for state in compiled.analysis.catalog.states.values() {
        if state.target != *target {
            continue;
        }
        let source = TargetRef::new("state", &state.id);
        if routes.contains_key(&source) {
            related.insert(source);
        }
    }
    for anchor in compiled.analysis.catalog.anchors.values() {
        if !anchor.links.iter().any(|link| link.target == *target) {
            continue;
        }
        let source = TargetRef::new("anchor", &anchor.id);
        if routes.contains_key(&source) {
            related.insert(source);
        }
    }
    related
}

fn public_source(source: &TargetRef, routes: &BTreeMap<TargetRef, String>) -> Option<TargetRef> {
    if routes.contains_key(source) {
        return Some(source.clone());
    }
    if source.kind != "scene" {
        return None;
    }
    let mut name = source.id.as_str();
    while let Some((parent, _)) = name.rsplit_once('.') {
        let target = TargetRef::new(
            if parent.contains('.') {
                "scene"
            } else {
                "event"
            },
            parent,
        );
        if routes.contains_key(&target) {
            return Some(target);
        }
        name = parent;
    }
    None
}

pub(super) fn append_map_backlinks(
    project: &Project,
    selection: &ReaderExportSelection,
    routes: &BTreeMap<TargetRef, String>,
    overrides: &[ReaderProfileRoute],
    pages: &mut [PublicPage],
) -> Result<(), String> {
    if selection.maps.is_empty() {
        return Ok(());
    }
    let index = project.map_index();
    for (map_index, choice) in selection.maps.iter().enumerate() {
        let map = index.maps.get(&choice.id).ok_or("公开地图不存在")?;
        let map_route = super::routes::target_route(
            selection,
            &TargetRef::new("map", &choice.id),
            format!("maps/m{:04}.html", map_index + 1),
            overrides,
        )?;
        for id in &choice.placements {
            let target = map
                .placements
                .get(id)
                .and_then(|p| p.target_ref.as_ref())
                .or_else(|| {
                    map.scene
                        .as_ref()
                        .and_then(|s| s.nodes.get(id))
                        .and_then(|n| n.target_ref.as_ref())
                });
            let Some(route) = target.and_then(|target| routes.get(target)) else {
                continue;
            };
            let Some(page) = pages
                .iter_mut()
                .find(|page| page.output_path.as_path() == std::path::Path::new(route))
            else {
                continue;
            };
            let href = format!(
                "{}#{}",
                relative_url(route, &map_route),
                super::routes::public_anchor(&choice.id, id)
            );
            let html = format!(
                "<a href=\"{}\">{}</a>",
                html_escape(&href),
                html_escape(&map.title)
            );
            entry(page, "地图位置", &html, &map.title);
        }
    }
    Ok(())
}

use super::{
    style, svg_elements::*, svg_path, svg_transform, validate, viewport, Affine, MapScene,
    SceneError, SceneGeometry, SceneLimits, SceneNode, SceneProgress, SceneStyle, SvgScenePreview,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn preview_scene(source: &str) -> Result<SvgScenePreview, SceneError> {
    preview_scene_with_control(source, &SceneLimits::default(), &mut |_| true)
}

pub(super) fn preview_scene_with_control(
    source: &str,
    limits: &SceneLimits,
    progress: &mut dyn FnMut(SceneProgress) -> bool,
) -> Result<SvgScenePreview, SceneError> {
    validate::check_limits(limits)?;
    if source.len() > limits.max_source_bytes {
        return Err(validate::limit("SVG 原文大小"));
    }
    let arena = parse_xml(source, limits, progress)?;
    let root = &arena[0];
    let (mut scene, width, height) =
        dimensions(root).map_err(|e| root.error(source, e, "viewBox"))?;
    let mut importer = Importer {
        source,
        arena: &arena,
        limits,
        progress,
        scene: &mut scene,
        reserved: BTreeSet::new(),
        next_id: 1,
        segments: 0,
        shapes: 0,
        clips: BTreeMap::new(),
        references: BTreeSet::new(),
        positions: BTreeMap::new(),
    };
    importer.reserve_ids()?;
    importer.read_clips()?;
    let id = importer.build(0, None, false, Affine::IDENTITY, &SceneStyle::default())?;
    let clip = viewport::root_clip(importer.scene, width, height)
        .map_err(|e| root.error(source, e, "viewBox"))?;
    let node = importer.scene.nodes.get_mut(&id).unwrap();
    node.clip_rect = Some(clip);
    node.extra.insert("svg_root".into(), Value::Bool(true));
    importer
        .scene
        .root_order
        .insert("svg".into(), vec![id.clone()]);
    if let Some((_, (_, index))) = importer
        .clips
        .iter()
        .find(|(id, _)| !importer.references.contains(*id))
    {
        return Err(arena[*index].error(
            source,
            profile("SVG viewport 裁剪定义必须且只能被引用一次"),
            "id",
        ));
    }
    if !root.attrs.contains_key("id") {
        collapse_root(importer.scene, &id);
    }
    report(
        importer.progress,
        "svg_validate",
        0,
        importer.scene.nodes.len(),
    )?;
    super::dash::declare_scene(importer.scene)?;
    let check = validate::validate_scene(importer.scene, limits).and_then(|()| {
        let matrix = super::view_box_transform(
            importer.scene.view_box,
            width,
            height,
            &importer.scene.preserve_aspect_ratio,
        )?;
        super::world_bounds::viewport(importer.scene, matrix)
    });
    check.map_err(|e| {
        let index = e
            .node_id
            .as_ref()
            .and_then(|id| importer.positions.get(id))
            .copied()
            .unwrap_or(0);
        arena[index].error(source, e, "geometry")
    })?;
    report(
        importer.progress,
        "svg_validate",
        importer.scene.nodes.len(),
        importer.scene.nodes.len(),
    )?;
    Ok(SvgScenePreview {
        scene,
        width,
        height,
        diagnostics: Vec::new(),
    })
}

fn dimensions(root: &Element) -> Result<(MapScene, f64, f64), SceneError> {
    let view_box = root
        .attrs
        .get("viewBox")
        .map(|value| svg_path::numbers(value))
        .transpose()?;
    if view_box.as_ref().is_some_and(|v| v.len() != 4) {
        return Err(profile("SVG viewBox 必须包含四个数值"));
    }
    let default_width = view_box.as_ref().map(|v| v[2]).unwrap_or(0.0);
    let default_height = view_box.as_ref().map(|v| v[3]).unwrap_or(0.0);
    let width = scalar(root, "width", default_width)?;
    let height = scalar(root, "height", default_height)?;
    let mut scene = MapScene::new(width, height);
    if let Some(v) = view_box {
        scene.view_box = [v[0], v[1], v[2], v[3]];
    }
    if let Some(aspect) = root.attrs.get("preserveAspectRatio") {
        scene.preserve_aspect_ratio = aspect.clone();
    }
    super::view_box_transform(scene.view_box, width, height, &scene.preserve_aspect_ratio)?;
    Ok((scene, width, height))
}

struct Importer<'a> {
    source: &'a str,
    arena: &'a [Element],
    limits: &'a SceneLimits,
    progress: &'a mut dyn FnMut(SceneProgress) -> bool,
    scene: &'a mut MapScene,
    reserved: BTreeSet<String>,
    next_id: usize,
    segments: usize,
    shapes: usize,
    clips: BTreeMap<String, ([f64; 4], usize)>,
    references: BTreeSet<String>,
    positions: BTreeMap<String, usize>,
}

impl Importer<'_> {
    fn reserve_ids(&mut self) -> Result<(), SceneError> {
        let mut identity_bytes = 0usize;
        for element in self.arena {
            if let Some(id) = element.attrs.get("id") {
                identity_bytes = identity_bytes
                    .saturating_add(id.len().saturating_mul(3 + element.children().count()));
                if identity_bytes > self.limits.max_document_bytes {
                    return Err(element.error(
                        self.source,
                        validate::limit("派生节点身份字节数"),
                        "id",
                    ));
                }
            }
        }
        for element in self.arena {
            if let Some(id) = element.attrs.get("id") {
                if !self.reserved.insert(id.clone()) {
                    return Err(element.error(self.source, profile("SVG id 重复"), "id"));
                }
            }
        }
        Ok(())
    }

    fn id(&mut self, index: usize) -> String {
        if let Some(id) = self.arena[index]
            .attrs
            .get("id")
            .filter(|id| crate::workspace_documents::valid_id(id))
        {
            return id.clone();
        }
        loop {
            let id = format!("svg_{}", self.next_id);
            self.next_id += 1;
            if self.reserved.insert(id.clone()) {
                return id;
            }
        }
    }

    fn read_clips(&mut self) -> Result<(), SceneError> {
        for defs_index in self.arena[0]
            .children()
            .filter(|i| self.arena[*i].tag == "defs")
        {
            for index in self.arena[defs_index].children() {
                let element = &self.arena[index];
                let parse = || {
                    let id = element
                        .attrs
                        .get("id")
                        .ok_or_else(|| profile("viewport clipPath 缺少 id"))?;
                    if !clip_id(id)
                        || element.attrs.get("clipPathUnits").map(String::as_str)
                            != Some("userSpaceOnUse")
                    {
                        return Err(profile(
                            "只允许 wl-viewport-十进制编号 与 userSpaceOnUse 的 clipPath",
                        ));
                    }
                    let children: Vec<_> = element.children().collect();
                    if children.len() != 1 {
                        return Err(profile("viewport clipPath 必须恰好包含一个 rect"));
                    }
                    let rect = &self.arena[children[0]];
                    if rect
                        .attrs
                        .keys()
                        .any(|k| !matches!(k.as_str(), "x" | "y" | "width" | "height"))
                        || rect.children().next().is_some()
                    {
                        return Err(rect.error(
                            self.source,
                            profile("viewport 裁剪 rect 不允许其它属性、变换、样式或子元素"),
                            "element",
                        ));
                    }
                    let values = [
                        scalar(rect, "x", 0.0)?,
                        scalar(rect, "y", 0.0)?,
                        scalar(rect, "width", 0.0)?,
                        scalar(rect, "height", 0.0)?,
                    ];
                    if values[2] <= 0.0 || values[3] <= 0.0 {
                        return Err(rect.error(
                            self.source,
                            profile("viewport 裁剪矩形宽高必须为正"),
                            "width",
                        ));
                    }
                    viewport::number(values[0] + values[2])?;
                    viewport::number(values[1] + values[3])?;
                    Ok((id.clone(), values))
                };
                let (id, rect) = parse().map_err(|e| element.error(self.source, e, "clipPath"))?;
                self.clips.insert(id, (rect, index));
            }
        }
        Ok(())
    }

    fn build(
        &mut self,
        index: usize,
        parent: Option<&str>,
        inherited_space: bool,
        inherited_matrix: Affine,
        inherited_style: &SceneStyle,
    ) -> Result<String, SceneError> {
        let arena = self.arena;
        let element = &arena[index];
        let id = self.id(index);
        let parse = || {
            let mut node = SceneNode::new(
                &id,
                "svg",
                geometry(
                    element,
                    self.limits.max_segments.saturating_sub(self.segments),
                )?,
            );
            node.name = element.attrs.get("id").cloned().unwrap_or_default();
            node.parent_id = parent.map(str::to_owned);
            node.style = style::parse_style(&element.attrs).map_err(|mut e| {
                if e.field.is_none() {
                    e.field = Some("style".into());
                }
                e
            })?;
            node.transform = element
                .attrs
                .get("transform")
                .map(|value| svg_transform::parse_transform(value))
                .transpose()
                .map_err(|mut e| {
                    e.field = Some("transform".into());
                    e
                })?
                .unwrap_or_default();
            let world = viewport::checked_affine(inherited_matrix.then(node.transform))?;
            if let SceneGeometry::Text { runs, .. } = &mut node.geometry {
                *runs = text_runs(self.arena, index, inherited_space, self.source)?;
                super::svg_location::check_text_dashes(
                    self.arena,
                    index,
                    runs,
                    &node.style.inherited(inherited_style),
                    self.source,
                )?;
            }
            Ok::<_, SceneError>((node, world))
        };
        let (mut node, world) = parse().map_err(|mut e| {
            if e.node_id.is_none() {
                e.node_id = Some(id.clone());
            }
            element.error(self.source, e, "geometry")
        })?;
        if element.attrs.contains_key("clip-path")
            || element.attrs.contains_key("data-worldline-viewport")
        {
            let reference = element
                .attrs
                .get("clip-path")
                .and_then(|v| v.strip_prefix("url(#"))
                .and_then(|v| v.strip_suffix(')'));
            if element
                .attrs
                .get("data-worldline-viewport")
                .map(String::as_str)
                != Some("1")
                || reference.is_none_or(|v| !clip_id(v))
            {
                return Err(element.error(
                    self.source,
                    profile("viewport 组必须包含成对的受控标记和精确本地 clip-path"),
                    "clip-path",
                ));
            }
            let reference = reference.unwrap();
            let Some((clip, _)) = self.clips.get(reference) else {
                return Err(element.error(
                    self.source,
                    profile("viewport clip-path 引用不存在"),
                    "clip-path",
                ));
            };
            if !self.references.insert(reference.into()) {
                return Err(element.error(
                    self.source,
                    profile("viewport 裁剪定义不能被重复引用"),
                    "clip-path",
                ));
            }
            node.clip_rect = Some(*clip);
            node.extra.insert("svg_root".into(), Value::Bool(true));
        }
        match &node.geometry {
            SceneGeometry::Path { segments } => self.segments += segments.len(),
            SceneGeometry::Polyline { points } | SceneGeometry::Polygon { points } => {
                self.segments += points.len()
            }
            _ => {}
        }
        if !matches!(node.geometry, SceneGeometry::Group { .. }) {
            self.shapes += 1;
        }
        if self.shapes > self.limits.max_import_shapes
            || self.segments > self.limits.max_segments
            || self.scene.nodes.len() >= self.limits.max_nodes
        {
            return Err(element.error(
                self.source,
                validate::limit("SVG 图形、节点或路径段数"),
                "geometry",
            ));
        }
        report(
            self.progress,
            "svg_import",
            self.scene.nodes.len(),
            self.arena.len(),
        )?;
        let effective_style = node.style.inherited(inherited_style);
        self.positions.insert(id.clone(), index);
        self.scene.nodes.insert(id.clone(), node);
        if matches!(element.tag.as_str(), "svg" | "g") {
            let mut children = Vec::new();
            for child in element.children().filter(|i| arena[*i].tag != "defs") {
                children.push(self.build(
                    child,
                    Some(&id),
                    element.preserve(inherited_space),
                    world,
                    &effective_style,
                )?);
            }
            self.scene.nodes.get_mut(&id).unwrap().geometry = SceneGeometry::Group { children };
        }
        Ok(id)
    }
}

fn clip_id(id: &str) -> bool {
    id.strip_prefix("wl-viewport-")
        .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
}

fn collapse_root(scene: &mut MapScene, root_id: &str) {
    let root = &scene.nodes[root_id];
    if root.transform != Affine::IDENTITY || root.style != SceneStyle::default() {
        return;
    }
    let SceneGeometry::Group { children } = &root.geometry else {
        return;
    };
    if children.len() != 1 {
        return;
    }
    let child_id = children[0].clone();
    let child = &scene.nodes[&child_id];
    if !matches!(child.geometry, SceneGeometry::Group { .. })
        || child.transform != Affine::IDENTITY
        || child.clip_rect != root.clip_rect
        || child.extra.get("svg_root") != Some(&Value::Bool(true))
    {
        return;
    }
    scene.nodes.remove(root_id);
    scene.nodes.get_mut(&child_id).unwrap().parent_id = None;
    scene.root_order.insert("svg".into(), vec![child_id]);
}

#[cfg(test)]
#[path = "svg_scene_tests.rs"]
mod tests;

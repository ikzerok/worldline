//! 原生可编辑矢量场景。格式与安全边界见 spec/vector-scene.md。
//! SVG、作者 UI 与读者发布共用本模型；临时采样不得成为持久化真源。
mod affine;
mod batch;
mod contract;
mod dash;
mod dash_budget;
mod edit_tree;
mod entity;
mod legacy;
mod operations;
mod preserve;
mod projection;
mod render;
mod structure;
mod style;
mod svg_elements;
mod svg_location;
mod svg_path;
mod svg_scene;
mod svg_transform;
mod validate;
mod viewport;
mod world_bounds;

pub use affine::Affine;
pub use batch::{
    apply_batch, apply_batch_with_control, preview_batch, preview_batch_with_control, ScenePlan,
};
pub use contract::{
    SceneBatch, SceneEntityRequest, SceneError, SceneLimits, SceneOp, SceneProgress,
    ScenePublicLink, SvgScenePreview,
};
pub use entity::{
    apply_entity_binding, preview_entity_binding, SceneEntityPlan, SceneEntityResult,
    SceneSourceChange,
};
pub use projection::{node_state, node_world_transform, project_scene, SceneNodeState};
pub use render::{
    map_to_safe_svg, scene_to_safe_svg, to_safe_svg, to_safe_svg_layers_with_links,
    to_safe_svg_with_links,
};
pub(crate) fn preview_scene(source: &str) -> Result<SvgScenePreview, SceneError> {
    svg_scene::preview_scene(source)
}
pub(crate) fn preview_scene_with_control(
    source: &str,
    limits: &SceneLimits,
    progress: &mut dyn FnMut(SceneProgress) -> bool,
) -> Result<SvgScenePreview, SceneError> {
    svg_scene::preview_scene_with_control(source, limits, progress)
}
pub(crate) use dash::{
    document_read_only as dash_document_read_only, uses_dash as dash_feature_required,
};
pub use validate::validate_scene;
pub use viewport::{import_view_transform, view_box_transform};

use crate::catalog::TargetRef;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub const SCENE_FEATURE: &str = "presentation.vector_scene.v1";
pub const SCENE_DASH_FEATURE: &str = "presentation.vector_stroke_dash.v1";
pub const MAX_DASH_ENTRIES: usize = 64;
pub const MAX_DASH_WORK: usize = 100_000;
pub const SCENE_SCHEMA_VERSION: u64 = 1;
pub const WORLD_LIMIT: f64 = 1_000_000_000.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapScene {
    pub schema_version: u64,
    pub view_box: [f64; 4],
    pub preserve_aspect_ratio: String,
    pub root_order: BTreeMap<String, Vec<String>>,
    pub nodes: BTreeMap<String, SceneNode>,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

impl MapScene {
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            schema_version: SCENE_SCHEMA_VERSION,
            view_box: [0.0, 0.0, width, height],
            preserve_aspect_ratio: "xMidYMid meet".into(),
            root_order: BTreeMap::new(),
            nodes: BTreeMap::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneNode {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub layer_id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub geometry: SceneGeometry,
    #[serde(default)]
    pub transform: Affine,
    #[serde(default)]
    pub style: SceneStyle,
    #[serde(default = "visible_default")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    /// 根 SVG viewport 的局部 `[x,y,width,height]` 矩形裁剪；只允许导入根组。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_rect: Option<[f64; 4]>,
    #[serde(default)]
    pub target_ref: Option<TargetRef>,
    #[serde(default)]
    pub annotation: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub label_override: Option<String>,
    #[serde(default)]
    pub navigation: Option<SceneNavigation>,
    #[serde(default)]
    pub scope_refs: Vec<TargetRef>,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

fn visible_default() -> bool {
    true
}

impl SceneNode {
    pub fn new(
        id: impl Into<String>,
        layer_id: impl Into<String>,
        geometry: SceneGeometry,
    ) -> Self {
        Self {
            id: id.into(),
            name: String::new(),
            layer_id: layer_id.into(),
            parent_id: None,
            geometry,
            transform: Affine::IDENTITY,
            style: SceneStyle::default(),
            visible: true,
            locked: false,
            clip_rect: None,
            target_ref: None,
            annotation: String::new(),
            role: String::new(),
            label_override: None,
            navigation: None,
            scope_refs: Vec::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneNavigation {
    pub map_id: String,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneGeometry {
    Group {
        children: Vec<String>,
    },
    Point {
        position: [f64; 2],
    },
    Polyline {
        points: Vec<[f64; 2]>,
    },
    Polygon {
        points: Vec<[f64; 2]>,
    },
    Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        rx: f64,
        ry: f64,
    },
    Ellipse {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
    },
    Path {
        segments: Vec<PathSegment>,
    },
    Text {
        x: f64,
        y: f64,
        runs: Vec<TextRun>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PathSegment {
    Move {
        to: [f64; 2],
    },
    Line {
        to: [f64; 2],
    },
    Cubic {
        control1: [f64; 2],
        control2: [f64; 2],
        to: [f64; 2],
    },
    Quadratic {
        control: [f64; 2],
        to: [f64; 2],
    },
    Arc {
        rx: f64,
        ry: f64,
        rotation: f64,
        large_arc: bool,
        sweep: bool,
        to: [f64; 2],
    },
    Close,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub dx: f64,
    #[serde(default)]
    pub dy: f64,
    #[serde(default)]
    pub style: SceneStyle,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneStyle {
    pub fill: Option<String>,
    pub stroke: Option<String>,
    pub stroke_width: Option<f64>,
    /// None 继承；空数组显式实线；奇数数列按 SVG 逻辑重复。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_dasharray: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_dashoffset: Option<f64>,
    pub opacity: Option<f64>,
    pub fill_opacity: Option<f64>,
    pub stroke_opacity: Option<f64>,
    pub fill_rule: Option<String>,
    pub line_cap: Option<String>,
    pub line_join: Option<String>,
    pub miter_limit: Option<f64>,
    pub font_size: Option<f64>,
    pub font_family: Option<String>,
    pub font_weight: Option<String>,
    pub font_style: Option<String>,
    pub text_anchor: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl SceneStyle {
    /// SVG opacity 是节点合成而非继承；其它已知样式按 child 优先合并。
    /// 未知字段只保留本节点自身值，不沿树放大用户元数据。
    pub fn inherited(&self, parent: &Self) -> Self {
        Self {
            fill: self.fill.clone().or_else(|| parent.fill.clone()),
            stroke: self.stroke.clone().or_else(|| parent.stroke.clone()),
            stroke_width: self.stroke_width.or(parent.stroke_width),
            stroke_dasharray: self
                .stroke_dasharray
                .clone()
                .or_else(|| parent.stroke_dasharray.clone()),
            stroke_dashoffset: self.stroke_dashoffset.or(parent.stroke_dashoffset),
            opacity: self.opacity,
            fill_opacity: self.fill_opacity.or(parent.fill_opacity),
            stroke_opacity: self.stroke_opacity.or(parent.stroke_opacity),
            fill_rule: self.fill_rule.clone().or_else(|| parent.fill_rule.clone()),
            line_cap: self.line_cap.clone().or_else(|| parent.line_cap.clone()),
            line_join: self.line_join.clone().or_else(|| parent.line_join.clone()),
            miter_limit: self.miter_limit.or(parent.miter_limit),
            font_size: self.font_size.or(parent.font_size),
            font_family: self
                .font_family
                .clone()
                .or_else(|| parent.font_family.clone()),
            font_weight: self
                .font_weight
                .clone()
                .or_else(|| parent.font_weight.clone()),
            font_style: self
                .font_style
                .clone()
                .or_else(|| parent.font_style.clone()),
            text_anchor: self
                .text_anchor
                .clone()
                .or_else(|| parent.text_anchor.clone()),
            extra: self.extra.clone(),
        }
    }
}

/// 临时绘制/命中投影，不序列化回场景。paths 已乘累计节点矩阵。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScenePrimitive {
    pub node_id: String,
    pub paths: Vec<Vec<[f64; 2]>>,
    pub closed: Vec<bool>,
    pub style: SceneStyle,
    pub text: Vec<TextRun>,
    pub text_origin: [f64; 2],
    pub transform: Affine,
    /// 场景坐标包围盒 `[min_x,min_y,max_x,max_y]`。
    pub bounds: [f64; 4],
    /// 祖先 viewport 裁剪及其累计矩阵，供命中检测消费同一核心投影。
    pub clips: Vec<SceneClip>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneClip {
    pub rect: [f64; 4],
    pub transform: Affine,
}

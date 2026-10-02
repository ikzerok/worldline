use super::{MapScene, SceneNode};
use crate::authoring::EntityDraft;
use crate::presentation_commands::Revision;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneBatch {
    pub map_id: String,
    pub expected_revision: Revision,
    pub expected_documents: BTreeMap<PathBuf, String>,
    pub operations: Vec<SceneOp>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneOp {
    EnableScene,
    Insert {
        node: SceneNode,
        index: Option<usize>,
    },
    Update {
        node: SceneNode,
    },
    Delete {
        node_ids: Vec<String>,
    },
    Reorder {
        parent_id: Option<String>,
        layer_id: String,
        node_ids: Vec<String>,
    },
    Group {
        group_id: String,
        node_ids: Vec<String>,
        name: String,
    },
    Ungroup {
        node_id: String,
    },
    Duplicate {
        node_ids: Vec<String>,
        id_prefix: String,
        offset: [f64; 2],
    },
    ImportSvg {
        layer_id: String,
        title: String,
        source: String,
    },
    ImportScene {
        layer_id: String,
        title: String,
        scene: MapScene,
        width: f64,
        height: f64,
    },
    MoveToLayer {
        node_ids: Vec<String>,
        layer_id: String,
    },
    MigratePlacements {
        node_ids: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneError {
    pub code: String,
    pub message: String,
    pub node_id: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub field: Option<String>,
    pub operation_index: Option<usize>,
}

impl SceneError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            node_id: None,
            line: None,
            column: None,
            field: None,
            operation_index: None,
        }
    }

    pub fn at(mut self, node_id: impl Into<String>, field: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self.field = Some(field.into());
        self
    }
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for SceneError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneProgress {
    pub stage: String,
    pub completed: usize,
    pub total: usize,
}

/// 调用方只能调低；实现入口必须再核硬上限，不能信任反序列化预算。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneLimits {
    pub max_source_bytes: usize,
    pub max_nodes: usize,
    pub max_import_shapes: usize,
    pub max_depth: usize,
    pub max_segments: usize,
    pub max_text_bytes: usize,
    pub max_operations: usize,
    pub max_document_bytes: usize,
    pub max_projection_points: usize,
}

impl Default for SceneLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 2 * 1024 * 1024,
            max_nodes: 5000,
            max_import_shapes: 1000,
            max_depth: 32,
            max_segments: 100_000,
            max_text_bytes: 1024 * 1024,
            max_operations: 10_000,
            max_document_bytes: 16 * 1024 * 1024,
            max_projection_points: 200_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SvgScenePreview {
    pub scene: MapScene,
    pub width: f64,
    pub height: f64,
    pub diagnostics: Vec<SceneError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenePublicLink {
    pub href: Option<String>,
    pub anchor: String,
    pub label: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SceneEntityRequest {
    pub expected_baseline: String,
    pub expected_revision: Revision,
    pub expected_documents: BTreeMap<PathBuf, String>,
    pub map_id: String,
    pub node_id: String,
    pub path: PathBuf,
    pub draft: EntityDraft,
}

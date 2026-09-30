//! 展示文档结构化命令。
//!
//! 地图 DTO 是只读查询；本模块把结构化修改落到 `Project` 保留的原始 JSON
//! 缓冲，并只触碰命令明确的字段。这样未知字段与扩展值仍保留其结构化内容；
//! JSON 空白和键顺序由结构化写回重新格式化，展示修改也不会经过故事可运行性
//! 门槛。

mod document;
mod layers;
mod measurement;
mod placements;
mod transaction;

use crate::catalog::TargetRef;
use crate::presentation::MapGeometry;
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry, valid_id};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// 进程内展示命令的乐观修订令牌。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Revision {
    pub workspace_generation: u64,
    pub content_generation: u64,
    pub presentation_generation: u64,
}

impl Revision {
    /// 只增加展示修订；地图命令不改变内容代或故事指纹。
    pub fn next_presentation(self) -> Self {
        Self {
            presentation_generation: self.presentation_generation.wrapping_add(1),
            ..self
        }
    }
}

/// 命令提交时的工作区基线。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CommandEnvelope {
    pub expected_revision: Revision,
    /// 路径使用 `Project` 的绝对路径；值是命令读取到的原始字节 hash。
    #[serde(default)]
    pub expected_documents: BTreeMap<PathBuf, String>,
    pub command: Command,
}

/// M1 展示修改命令。每个命令只负责一个可撤销的用户意图。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    SetMapMeasurement {
        map_id: String,
        measurement: crate::presentation::MapMeasurement,
    },
    CreatePlacement {
        map_id: String,
        placement_id: String,
        layer_id: String,
        target_ref: Option<TargetRef>,
        geometry: MapGeometry,
        annotation: String,
        role: String,
        label_override: Option<String>,
    },
    UpdatePlacement {
        map_id: String,
        placement_id: String,
        /// `None` means leave the field unchanged.
        geometry: Option<MapGeometry>,
        /// `Some(None)` clears a nullable target field; `None` leaves it unchanged.
        target_ref: Option<Option<TargetRef>>,
        annotation: Option<String>,
        role: Option<String>,
        /// `Some(None)` clears the custom label; `None` leaves it unchanged.
        label_override: Option<Option<String>>,
        layer_id: Option<String>,
    },
    DeletePlacement {
        map_id: String,
        placement_id: String,
    },
    CreateLayer {
        map_id: String,
        layer_id: String,
        title: String,
        visible_default: bool,
        locked: bool,
    },
    /// 更新一个已有图层的已知字段；未知字段保持不变。
    SetLayer {
        map_id: String,
        layer_id: String,
        title: Option<String>,
        visible_default: Option<bool>,
        locked: Option<bool>,
        /// 可选的完整显示顺序；必须恰好包含每个已有图层一次。
        layer_order: Option<Vec<String>>,
    },
    /// 只在图层没有标记时删除图层，避免隐式删除作者标记。
    DeleteLayer { map_id: String, layer_id: String },
}

/// 命令验证或提交失败的稳定错误域。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditError {
    InvalidSchema {
        message: String,
    },
    InvalidGeometry {
        message: String,
    },
    MissingReference {
        message: String,
    },
    UnresolvedReference {
        message: String,
    },
    StaleRevision {
        expected: Revision,
        actual: Revision,
    },
    StaleContent {
        message: String,
    },
    ExternalConflict {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    ReadOnlyFeature {
        message: String,
    },
    AssetLimit {
        message: String,
    },
    StorageFailure {
        message: String,
    },
}

impl std::fmt::Display for EditError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchema { message }
            | Self::InvalidGeometry { message }
            | Self::MissingReference { message }
            | Self::UnresolvedReference { message }
            | Self::ReadOnlyFeature { message }
            | Self::AssetLimit { message }
            | Self::StorageFailure { message } => formatter.write_str(message),
            Self::StaleContent { message } => formatter.write_str(message),
            Self::StaleRevision { expected, actual } => write!(
                formatter,
                "展示命令修订过期：期望 {:?}，当前 {:?}",
                expected, actual
            ),
            Self::ExternalConflict {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "展示文档已被外部修改：{}（期望 {}，当前 {}）",
                path.display(),
                expected,
                actual
            ),
        }
    }
}

impl std::error::Error for EditError {}

/// 一次命令改变的一个展示文档。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentChange {
    pub path: PathBuf,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

/// 一次命令的可逆记录。Undo 仍是一个新的内存操作，不直接写盘。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UndoRecord {
    pub base_revision: Revision,
    pub applied_revision: Revision,
    pub changes: Vec<DocumentChange>,
}

/// 结构化命令成功结果。
#[derive(Clone, Debug)]
pub struct CommandResult {
    pub new_revision: Revision,
    pub changed_files: Vec<PathBuf>,
    pub affected_refs: Vec<TargetRef>,
    pub undo_record: UndoRecord,
    pub diagnostics: Vec<crate::Diagnostic>,
}

struct AppliedDocument {
    path: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
    affected_refs: Vec<TargetRef>,
    diagnostics: Vec<crate::Diagnostic>,
}

/// 读取文档原始字节的稳定 hash；不依赖平台路径格式或随机哈希种子。
pub fn document_hash(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// 返回已注册地图的 authoring 文档路径，供 UI 组装 expected_documents。
pub fn map_document_path(project: &Project, map_id: &str) -> Result<PathBuf, EditError> {
    let manifest = manifest_path(&project.root);
    let document = project
        .authoring_document(&manifest)
        .map_err(|message| EditError::MissingReference { message })?;
    let registry = parse_registry(&project.root, document.bytes());
    registry
        .maps
        .get(map_id)
        .cloned()
        .ok_or_else(|| EditError::MissingReference {
            message: format!("地图 `{map_id}` 未注册"),
        })
}

/// 以调用方已经持有的内容编译快照构造地图索引，避免展示命令重新编译故事。
pub fn map_index_with_content(
    project: &Project,
    content: &crate::CompileResult,
) -> crate::presentation::MapIndex {
    crate::presentation::build_map_index(project, content)
}

/// 检查命令基线、验证展示约束并把结果写回 Project 内存缓冲。
pub fn apply(
    project: &mut Project,
    revision: &mut Revision,
    envelope: CommandEnvelope,
) -> Result<CommandResult, EditError> {
    let content = project.compile();
    transaction::apply_inner(project, revision, envelope, &content)
}

/// 使用现有内容编译快照提交展示命令；展示文档修改不会重新编译故事。
pub fn apply_with_content(
    project: &mut Project,
    revision: &mut Revision,
    envelope: CommandEnvelope,
    content: &crate::CompileResult,
) -> Result<CommandResult, EditError> {
    if content.sources != project.sources() {
        return Err(EditError::StaleContent {
            message: "内容编译快照已过期，请重新检查工程后再提交展示命令".into(),
        });
    }
    transaction::apply_inner(project, revision, envelope, content)
}

/// 恢复一个命令的原始文档。调用方须在发起本次撤销时捕获修订基线，
/// 不能在延迟执行时补填最新修订。记录只描述逆操作，不代表当前写入许可。
/// 连续撤销由历史栈逐次发起新的意图；每次仍推进展示修订。
pub fn undo(
    project: &mut Project,
    revision: &mut Revision,
    expected_revision: Revision,
    record: &UndoRecord,
) -> Result<(), EditError> {
    if *revision != expected_revision {
        return Err(EditError::StaleRevision {
            expected: expected_revision,
            actual: *revision,
        });
    }
    for change in &record.changes {
        let document = project
            .authoring_document(&change.path)
            .map_err(|message| EditError::ExternalConflict {
                path: change.path.clone(),
                expected: document_hash(&change.after),
                actual: format!("缺失:{message}"),
            })?;
        if document.bytes() != change.after.as_slice() {
            return Err(EditError::ExternalConflict {
                path: change.path.clone(),
                expected: document_hash(&change.after),
                actual: document_hash(document.bytes()),
            });
        }
    }
    for change in &record.changes {
        project
            .set_authoring_document(&change.path, change.before.clone())
            .map_err(|message| EditError::StorageFailure { message })?;
    }
    *revision = revision.next_presentation();
    Ok(())
}

impl Command {
    fn map_id(&self) -> &str {
        match self {
            Self::SetMapMeasurement { map_id, .. }
            | Self::CreatePlacement { map_id, .. }
            | Self::UpdatePlacement { map_id, .. }
            | Self::DeletePlacement { map_id, .. }
            | Self::CreateLayer { map_id, .. }
            | Self::SetLayer { map_id, .. }
            | Self::DeleteLayer { map_id, .. } => map_id,
        }
    }
}

fn layers_mut(object: &mut Map<String, Value>) -> Result<&mut Map<String, Value>, EditError> {
    object
        .get_mut("layers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| EditError::InvalidSchema {
            message: "地图 layers 必须是对象".into(),
        })
}

fn validate_id(id: &str, kind: &str) -> Result<(), EditError> {
    if valid_id(id) {
        Ok(())
    } else {
        Err(EditError::InvalidSchema {
            message: format!("{kind} `{id}` 无效"),
        })
    }
}

fn validate_text(value: &str, field: &str) -> Result<(), EditError> {
    if value.trim().is_empty() {
        Err(EditError::InvalidSchema {
            message: format!("{field} 不能为空"),
        })
    } else {
        Ok(())
    }
}

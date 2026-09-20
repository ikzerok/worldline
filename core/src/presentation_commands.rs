//! 展示文档结构化命令。
//!
//! 地图 DTO 是只读查询；本模块把结构化修改落到 `Project` 保留的原始 JSON
//! 缓冲，并只触碰命令明确的字段。这样未知字段与扩展值仍保留其结构化内容；
//! JSON 空白和键顺序由结构化写回重新格式化，展示修改也不会经过故事可运行性
//! 门槛。

use crate::catalog::TargetRef;
use crate::presentation::MapGeometry;
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json, valid_id};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
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
    apply_inner(project, revision, envelope, &content)
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
    apply_inner(project, revision, envelope, content)
}

fn apply_inner(
    project: &mut Project,
    revision: &mut Revision,
    envelope: CommandEnvelope,
    content: &crate::CompileResult,
) -> Result<CommandResult, EditError> {
    if envelope.expected_revision != *revision {
        return Err(EditError::StaleRevision {
            expected: envelope.expected_revision,
            actual: *revision,
        });
    }

    let map_path = map_document_path(project, envelope.command.map_id())?;
    let _expected_map =
        envelope
            .expected_documents
            .get(&map_path)
            .ok_or_else(|| EditError::ExternalConflict {
                path: map_path.clone(),
                expected: "缺少地图文档基线".into(),
                actual: project
                    .authoring_document(&map_path)
                    .map(|document| document_hash(document.bytes()))
                    .unwrap_or_else(|error| format!("缺失:{error}")),
            })?;
    for (path, expected) in &envelope.expected_documents {
        let document =
            project
                .authoring_document(path)
                .map_err(|message| EditError::ExternalConflict {
                    path: path.clone(),
                    expected: expected.clone(),
                    actual: format!("缺失:{message}"),
                })?;
        let actual = document_hash(document.bytes());
        if &actual != expected {
            return Err(EditError::ExternalConflict {
                path: path.clone(),
                expected: expected.clone(),
                actual,
            });
        }
    }

    let before_revision = *revision;
    let applied = apply_to_document(project, &envelope.command, content)?;
    let path = applied.path;
    let applied_revision = revision.next_presentation();
    let changed_files = vec![path.clone()];
    let undo_record = UndoRecord {
        base_revision: before_revision,
        applied_revision,
        changes: vec![DocumentChange {
            path,
            before: applied.before,
            after: applied.after,
        }],
    };
    *revision = applied_revision;
    Ok(CommandResult {
        new_revision: applied_revision,
        changed_files,
        affected_refs: applied.affected_refs,
        undo_record,
        diagnostics: applied.diagnostics,
    })
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

fn apply_to_document(
    project: &mut Project,
    command: &Command,
    content: &crate::CompileResult,
) -> Result<AppliedDocument, EditError> {
    let map_id = command.map_id();
    let path = map_document_path(project, map_id)?;
    let document = project
        .authoring_document(&path)
        .map_err(|message| EditError::MissingReference { message })?;
    if document.is_read_only() {
        return Err(EditError::ReadOnlyFeature {
            message: format!("地图 `{map_id}` 是只读展示文档"),
        });
    }
    let before = document.bytes().to_vec();
    let mut root =
        parse_unique_json(&before).map_err(|message| EditError::InvalidSchema { message })?;
    let object = root
        .as_object_mut()
        .ok_or_else(|| EditError::InvalidSchema {
            message: "地图 JSON 顶层必须是对象".into(),
        })?;
    if object.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(EditError::ReadOnlyFeature {
            message: "地图 schema_version 不受支持，只能只读查看".into(),
        });
    }
    if object.get("id").and_then(Value::as_str) != Some(map_id) {
        return Err(EditError::InvalidSchema {
            message: format!("地图 ID `{map_id}` 与注册表不一致"),
        });
    }

    let source_unresolved = content.has_errors();
    let catalog = &content.analysis.catalog;
    let mut affected = BTreeSet::new();
    match command {
        Command::CreatePlacement {
            placement_id,
            layer_id,
            target_ref,
            geometry,
            annotation,
            role,
            label_override,
            ..
        } => {
            validate_id(placement_id, "标记 ID")?;
            validate_geometry(geometry)?;
            ensure_geometry_feature(object, geometry)?;
            validate_text(annotation, "annotation")?;
            validate_text(role, "role")?;
            validate_target(
                target_ref.as_ref(),
                catalog,
                source_unresolved,
                &mut affected,
            )?;
            let layers = layers_mut(object)?;
            let layer = layers
                .get(layer_id)
                .ok_or_else(|| EditError::MissingReference {
                    message: format!("图层 `{layer_id}` 不存在"),
                })?;
            if layer.get("locked").and_then(Value::as_bool) == Some(true) {
                return Err(EditError::ReadOnlyFeature {
                    message: format!("图层 `{layer_id}` 已锁定"),
                });
            }
            let placements = placements_mut(object)?;
            if placements.contains_key(placement_id) {
                return Err(EditError::InvalidSchema {
                    message: format!("标记 `{placement_id}` 已存在"),
                });
            }
            let mut placement = Map::new();
            placement.insert("layer_id".into(), Value::String(layer_id.clone()));
            placement.insert(
                "target_ref".into(),
                target_ref.as_ref().map(target_value).unwrap_or(Value::Null),
            );
            placement.insert("geometry".into(), serde_json::to_value(geometry).unwrap());
            placement.insert("annotation".into(), Value::String(annotation.clone()));
            placement.insert("role".into(), Value::String(role.clone()));
            if let Some(label) = label_override {
                placement.insert("label_override".into(), Value::String(label.clone()));
            }
            placements.insert(placement_id.clone(), Value::Object(placement));
        }
        Command::UpdatePlacement {
            placement_id,
            geometry,
            target_ref,
            annotation,
            role,
            label_override,
            layer_id,
            ..
        } => {
            let original_layer = object
                .get("placements")
                .and_then(Value::as_object)
                .and_then(|placements| placements.get(placement_id))
                .and_then(Value::as_object)
                .and_then(|placement| placement.get("layer_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| EditError::MissingReference {
                    message: format!("标记 `{placement_id}` 不存在或缺少 layer_id"),
                })?
                .to_owned();
            let old_layer = object
                .get("layers")
                .and_then(Value::as_object)
                .and_then(|layers| layers.get(&original_layer))
                .ok_or_else(|| EditError::MissingReference {
                    message: format!("图层 `{original_layer}` 不存在"),
                })?;
            if old_layer.get("locked").and_then(Value::as_bool) == Some(true) {
                return Err(EditError::ReadOnlyFeature {
                    message: format!("图层 `{original_layer}` 已锁定"),
                });
            }
            if let Some(layer_id) = layer_id {
                validate_id(layer_id, "图层 ID")?;
                let new_layer = object
                    .get("layers")
                    .and_then(Value::as_object)
                    .and_then(|layers| layers.get(layer_id))
                    .ok_or_else(|| EditError::MissingReference {
                        message: format!("图层 `{layer_id}` 不存在"),
                    })?;
                if new_layer.get("locked").and_then(Value::as_bool) == Some(true) {
                    return Err(EditError::ReadOnlyFeature {
                        message: format!("图层 `{layer_id}` 已锁定"),
                    });
                }
            }
            if let Some(geometry) = geometry {
                validate_geometry(geometry)?;
                ensure_geometry_feature(object, geometry)?;
            }
            if let Some(target_ref) = target_ref {
                validate_target(
                    target_ref.as_ref(),
                    catalog,
                    source_unresolved,
                    &mut affected,
                )?;
            }
            if let Some(annotation) = annotation {
                validate_text(annotation, "annotation")?;
            }
            if let Some(role) = role {
                validate_text(role, "role")?;
            }
            let placement = placement_mut(object, placement_id)?;
            if let Some(geometry) = geometry {
                placement.insert("geometry".into(), serde_json::to_value(geometry).unwrap());
            }
            if let Some(target_ref) = target_ref {
                placement.insert(
                    "target_ref".into(),
                    target_ref.as_ref().map(target_value).unwrap_or(Value::Null),
                );
            }
            if let Some(annotation) = annotation {
                placement.insert("annotation".into(), Value::String(annotation.clone()));
            }
            if let Some(role) = role {
                placement.insert("role".into(), Value::String(role.clone()));
            }
            if let Some(label_override) = label_override {
                placement.insert(
                    "label_override".into(),
                    label_override
                        .as_ref()
                        .map(|label| Value::String(label.clone()))
                        .unwrap_or(Value::Null),
                );
            }
            if let Some(layer_id) = layer_id {
                placement.insert("layer_id".into(), Value::String(layer_id.clone()));
            }
        }
        Command::DeletePlacement { placement_id, .. } => {
            let layer_id = object
                .get("placements")
                .and_then(Value::as_object)
                .and_then(|placements| placements.get(placement_id))
                .ok_or_else(|| EditError::MissingReference {
                    message: format!("标记 `{placement_id}` 不存在"),
                })?
                .as_object()
                .and_then(|placement| placement.get("layer_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| EditError::InvalidSchema {
                    message: format!("标记 `{placement_id}` 缺少有效 layer_id"),
                })?
                .to_owned();
            let locked = object
                .get("layers")
                .and_then(Value::as_object)
                .and_then(|layers| layers.get(&layer_id))
                .and_then(|layer| layer.get("locked"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if locked {
                return Err(EditError::ReadOnlyFeature {
                    message: format!("图层 `{layer_id}` 已锁定"),
                });
            }
            let old = placements_mut(object)?
                .remove(placement_id)
                .ok_or_else(|| EditError::MissingReference {
                    message: format!("标记 `{placement_id}` 不存在"),
                })?;
            if let Some(target) = old.get("target_ref").and_then(parse_target_value) {
                affected.insert(target);
            }
        }
        Command::CreateLayer {
            layer_id,
            title,
            visible_default,
            locked,
            ..
        } => {
            validate_id(layer_id, "图层 ID")?;
            validate_text(title, "title")?;
            let layers = layers_mut(object)?;
            if layers.contains_key(layer_id) {
                return Err(EditError::InvalidSchema {
                    message: format!("图层 `{layer_id}` 已存在"),
                });
            }
            let mut layer = Map::new();
            layer.insert("title".into(), Value::String(title.clone()));
            layer.insert("visible_default".into(), Value::Bool(*visible_default));
            layer.insert("locked".into(), Value::Bool(*locked));
            layers.insert(layer_id.clone(), Value::Object(layer));
            let order = layer_order_mut(object)?;
            order.push(Value::String(layer_id.clone()));
        }
        Command::SetLayer {
            layer_id,
            title,
            visible_default,
            locked,
            layer_order,
            ..
        } => {
            let layer = layers_mut(object)?.get_mut(layer_id).ok_or_else(|| {
                EditError::MissingReference {
                    message: format!("图层 `{layer_id}` 不存在"),
                }
            })?;
            let layer = layer
                .as_object_mut()
                .ok_or_else(|| EditError::InvalidSchema {
                    message: format!("图层 `{layer_id}` 必须是对象"),
                })?;
            if let Some(title) = title {
                validate_text(title, "title")?;
                layer.insert("title".into(), Value::String(title.clone()));
            }
            if let Some(visible_default) = visible_default {
                layer.insert("visible_default".into(), Value::Bool(*visible_default));
            }
            if let Some(locked) = locked {
                layer.insert("locked".into(), Value::Bool(*locked));
            }
            if let Some(layer_order) = layer_order {
                validate_layer_order(object, layer_order)?;
                object.insert(
                    "layer_order".into(),
                    Value::Array(layer_order.iter().cloned().map(Value::String).collect()),
                );
            }
        }
        Command::DeleteLayer { layer_id, .. } => {
            let layer = object
                .get("layers")
                .and_then(Value::as_object)
                .and_then(|layers| layers.get(layer_id))
                .ok_or_else(|| EditError::MissingReference {
                    message: format!("图层 `{layer_id}` 不存在"),
                })?;
            if layer.get("locked").and_then(Value::as_bool) == Some(true) {
                return Err(EditError::ReadOnlyFeature {
                    message: format!("图层 `{layer_id}` 已锁定"),
                });
            }
            let has_placements = object
                .get("placements")
                .and_then(Value::as_object)
                .is_some_and(|placements| {
                    placements.values().any(|placement| {
                        placement.get("layer_id").and_then(Value::as_str) == Some(layer_id)
                    })
                });
            if has_placements {
                return Err(EditError::ReadOnlyFeature {
                    message: format!("图层 `{layer_id}` 仍包含标记，请先移动或删除标记"),
                });
            }
            layers_mut(object)?.remove(layer_id);
            let order = layer_order_mut(object)?;
            order.retain(|value| value.as_str() != Some(layer_id));
        }
    }

    let after = serde_json::to_vec_pretty(&root).map_err(|error| EditError::StorageFailure {
        message: format!("地图 JSON 序列化失败:{error}"),
    })?;
    if before == after {
        return Err(EditError::InvalidSchema {
            message: "展示命令没有产生修改".into(),
        });
    }
    project
        .set_authoring_document(&path, after.clone())
        .map_err(|message| EditError::StorageFailure { message })?;
    // 重新从 Project 派生诊断，命令自身不把其他地图诊断变成失败。
    let diagnostics = map_index_with_content(project, content).diagnostics;
    Ok(AppliedDocument {
        path,
        before,
        after,
        affected_refs: affected.into_iter().collect(),
        diagnostics,
    })
}

impl Command {
    fn map_id(&self) -> &str {
        match self {
            Self::CreatePlacement { map_id, .. }
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

fn placements_mut(object: &mut Map<String, Value>) -> Result<&mut Map<String, Value>, EditError> {
    object
        .get_mut("placements")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| EditError::InvalidSchema {
            message: "地图 placements 必须是对象".into(),
        })
}

fn placement_mut<'a>(
    object: &'a mut Map<String, Value>,
    placement_id: &str,
) -> Result<&'a mut Map<String, Value>, EditError> {
    placements_mut(object)?
        .get_mut(placement_id)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| EditError::MissingReference {
            message: format!("标记 `{placement_id}` 不存在"),
        })
}

fn layer_order_mut(object: &mut Map<String, Value>) -> Result<&mut Vec<Value>, EditError> {
    object
        .get_mut("layer_order")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| EditError::InvalidSchema {
            message: "地图 layer_order 必须是数组".into(),
        })
}

fn validate_layer_order(object: &Map<String, Value>, order: &[String]) -> Result<(), EditError> {
    let layers = object
        .get("layers")
        .and_then(Value::as_object)
        .ok_or_else(|| EditError::InvalidSchema {
            message: "地图 layers 必须是对象".into(),
        })?;
    let expected: BTreeSet<_> = layers.keys().cloned().collect();
    let actual: BTreeSet<_> = order.iter().cloned().collect();
    if expected != actual || actual.len() != order.len() {
        return Err(EditError::InvalidSchema {
            message: "layer_order 必须恰好包含每个图层一次".into(),
        });
    }
    Ok(())
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

fn validate_target(
    target: Option<&TargetRef>,
    catalog: &crate::catalog::Catalog,
    source_unresolved: bool,
    affected: &mut BTreeSet<TargetRef>,
) -> Result<(), EditError> {
    let Some(target) = target else {
        return Ok(());
    };
    if catalog.object(target).is_none() {
        return Err(if source_unresolved {
            EditError::UnresolvedReference {
                message: format!(
                    "对象 {} `{}` 尚未解析，无法建立新的地图引用",
                    target.kind, target.id
                ),
            }
        } else {
            EditError::MissingReference {
                message: format!("对象 {} `{}` 不存在", target.kind, target.id),
            }
        });
    }
    affected.insert(target.clone());
    Ok(())
}

fn target_value(target: &TargetRef) -> Value {
    serde_json::json!({"kind": target.kind, "id": target.id})
}

fn parse_target_value(value: &Value) -> Option<TargetRef> {
    let object = value.as_object()?;
    Some(TargetRef::new(
        object.get("kind")?.as_str()?,
        object.get("id")?.as_str()?,
    ))
}

fn validate_geometry(geometry: &MapGeometry) -> Result<(), EditError> {
    let points = geometry.points();
    let minimum = match geometry {
        MapGeometry::Point { .. } => 1,
        MapGeometry::Polyline { .. } => 2,
        MapGeometry::Polygon { .. } => 3,
    };
    if points.len() < minimum {
        return Err(EditError::InvalidGeometry {
            message: format!("几何至少需要 {minimum} 个点"),
        });
    }
    if points.iter().any(|point| {
        point
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    }) {
        return Err(EditError::InvalidGeometry {
            message: "几何坐标必须是有限数且在 [0,1] 内".into(),
        });
    }
    if let MapGeometry::Polygon { points } = geometry {
        if points.first() == points.last() || points.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(EditError::InvalidGeometry {
                message: "多边形不能重复首尾点或相邻点".into(),
            });
        }
        if polygon_area(points).abs() <= f64::EPSILON || polygon_self_intersects(points) {
            return Err(EditError::InvalidGeometry {
                message: "多边形必须是非退化的简单多边形".into(),
            });
        }
    }
    Ok(())
}

fn ensure_geometry_feature(
    object: &mut Map<String, Value>,
    geometry: &MapGeometry,
) -> Result<(), EditError> {
    let needs_feature = matches!(
        geometry,
        MapGeometry::Polyline { .. } | MapGeometry::Polygon { .. }
    );
    if !needs_feature {
        return Ok(());
    }
    let features = object
        .entry("required_features")
        .or_insert_with(|| Value::Array(Vec::new()));
    let values = features
        .as_array_mut()
        .ok_or_else(|| EditError::InvalidSchema {
            message: "地图 required_features 必须是数组".into(),
        })?;
    if values
        .iter()
        .any(|feature| feature.as_str() == Some("presentation.geometry.line_area.v1"))
    {
        return Ok(());
    }
    values.push(Value::String("presentation.geometry.line_area.v1".into()));
    Ok(())
}

fn polygon_area(points: &[[f64; 2]]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum::<f64>()
        * 0.5
}

fn polygon_self_intersects(points: &[[f64; 2]]) -> bool {
    let len = points.len();
    for first in 0..len {
        let first_next = (first + 1) % len;
        for second in (first + 1)..len {
            let second_next = (second + 1) % len;
            if first == second
                || first_next == second
                || second_next == first
                || (first == 0 && second_next == 0)
            {
                continue;
            }
            if segments_intersect(
                points[first],
                points[first_next],
                points[second],
                points[second_next],
            ) {
                return true;
            }
        }
    }
    false
}

fn segments_intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    fn orientation(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }
    fn on_segment(a: [f64; 2], b: [f64; 2], point: [f64; 2]) -> bool {
        point[0] >= a[0].min(b[0])
            && point[0] <= a[0].max(b[0])
            && point[1] >= a[1].min(b[1])
            && point[1] <= a[1].max(b[1])
    }
    let ab_c = orientation(a, b, c);
    let ab_d = orientation(a, b, d);
    let cd_a = orientation(c, d, a);
    let cd_b = orientation(c, d, b);
    let epsilon = 1e-12;
    (ab_c * ab_d < -epsilon && cd_a * cd_b < -epsilon)
        || (ab_c.abs() <= epsilon && on_segment(a, b, c))
        || (ab_d.abs() <= epsilon && on_segment(a, b, d))
        || (cd_a.abs() <= epsilon && on_segment(c, d, a))
        || (cd_b.abs() <= epsilon && on_segment(c, d, b))
}

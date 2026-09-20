//! 创建空白地图的展示命令。
//!
//! 地图创建只改 `Project` 持有的展示文档缓冲，不编译故事，也不触碰任何
//! `.wl` 文档。清单和地图文档先在副本中完成全部检查，最后一次性替换工程，
//! 这样即使第二个文件创建失败，调用方看到的原工程也不会留下半个注册项。

use crate::presentation_commands::{EditError, Revision};
use crate::project::Project;
use crate::workspace_documents::{
    manifest_path, parse_registry, parse_unique_json, registered_path, valid_id,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 预期文档不存在时使用的显式基线值。
///
/// 创建地图的表单在打开时会记录清单存在与否；因此旧工程若在表单打开后
/// 外部出现清单，提交会按外部冲突失败，而不会把新清单静默合并进去。
pub const MISSING_DOCUMENT_HASH: &str = "<missing>";

/// 创建地图所需的用户输入。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateMapRequest {
    /// 地图稳定 ID，同时决定默认文档名 `.world/maps/{id}.json`。
    pub id: String,
    /// 地图显示标题。
    pub title: String,
    /// 地图逻辑坐标系宽度，必须为正整数且不超过 MapDocument v1 上限。
    pub width: u32,
    /// 地图逻辑坐标系高度，必须为正整数且不超过 MapDocument v1 上限。
    pub height: u32,
}

impl CreateMapRequest {
    pub fn new(id: impl Into<String>, title: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            width,
            height,
        }
    }
}

/// 创建地图命令的并发基线。
///
/// `expected_documents` 中的路径使用 `Project` 的绝对路径，值使用
/// [`crate::presentation_commands::document_hash`] 计算。旧工程没有清单时不需要
/// 为不存在的清单提供虚构 hash；有清单的调用方必须提供清单基线。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateMapCommand {
    pub expected_revision: Revision,
    pub expected_documents: BTreeMap<PathBuf, String>,
    pub request: CreateMapRequest,
}

/// 一次创建地图修改的可逆记录。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateMapUndoRecord {
    pub base_revision: Revision,
    pub applied_revision: Revision,
    pub changes: Vec<CreateMapDocumentChange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateMapDocumentChange {
    pub path: PathBuf,
    /// `None` 表示该展示文档在创建前不存在，撤销时应标记删除。
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
}

/// 创建地图命令的结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateMapResult {
    pub map_id: String,
    pub manifest_path: PathBuf,
    pub map_path: PathBuf,
    pub changed_files: Vec<PathBuf>,
    pub new_revision: Revision,
    pub undo_record: CreateMapUndoRecord,
}

struct PreparedCreate {
    manifest_path: PathBuf,
    map_path: PathBuf,
    manifest_before: Option<Vec<u8>>,
    map_before: Option<Vec<u8>>,
    manifest_after: Vec<u8>,
    map_after: Vec<u8>,
}

/// 提交一次创建地图命令。
pub fn apply(
    project: &mut Project,
    revision: &mut Revision,
    command: CreateMapCommand,
) -> Result<CreateMapResult, EditError> {
    if command.expected_revision != *revision {
        return Err(EditError::StaleRevision {
            expected: command.expected_revision,
            actual: *revision,
        });
    }

    let prepared = prepare(project, &command.request, &command.expected_documents)?;

    // Project::create_authoring_document/set_authoring_document 已包含展示文档
    // 注册生命周期检查。先在副本中调用，避免两个文件的跨文档操作部分提交。
    let mut candidate = project.clone();
    if prepared.manifest_before.is_some() {
        candidate
            .set_authoring_document(&prepared.manifest_path, prepared.manifest_after.clone())
            .map_err(storage_error)?;
    } else {
        candidate
            .create_authoring_document(&prepared.manifest_path, prepared.manifest_after.clone())
            .map_err(storage_error)?;
    }
    candidate
        .create_authoring_document(&prepared.map_path, prepared.map_after.clone())
        .map_err(storage_error)?;

    let base_revision = *revision;
    let new_revision = revision.next_presentation();
    let changed_files = vec![prepared.manifest_path.clone(), prepared.map_path.clone()];
    let undo_record = CreateMapUndoRecord {
        base_revision,
        applied_revision: new_revision,
        changes: vec![
            CreateMapDocumentChange {
                path: prepared.manifest_path.clone(),
                before: prepared.manifest_before,
                after: prepared.manifest_after,
            },
            CreateMapDocumentChange {
                path: prepared.map_path.clone(),
                before: prepared.map_before,
                after: prepared.map_after,
            },
        ],
    };

    *project = candidate;
    *revision = new_revision;
    Ok(CreateMapResult {
        map_id: command.request.id,
        manifest_path: changed_files[0].clone(),
        map_path: changed_files[1].clone(),
        changed_files,
        new_revision,
        undo_record,
    })
}

/// 便于没有自定义命令队列的调用方提交创建地图。
pub fn create_map(
    project: &mut Project,
    revision: &mut Revision,
    request: CreateMapRequest,
    expected_documents: BTreeMap<PathBuf, String>,
) -> Result<CreateMapResult, EditError> {
    apply(
        project,
        revision,
        CreateMapCommand {
            expected_revision: *revision,
            expected_documents,
            request,
        },
    )
}

/// 撤销创建地图；当前文档若已被其他操作改变则拒绝覆盖。
pub fn undo(
    project: &mut Project,
    revision: &mut Revision,
    expected_revision: Revision,
    record: &CreateMapUndoRecord,
) -> Result<(), EditError> {
    if expected_revision != *revision {
        return Err(EditError::StaleRevision {
            expected: expected_revision,
            actual: *revision,
        });
    }
    let mut candidate = project.clone();
    for change in &record.changes {
        let actual = current_loaded_bytes(project, &change.path)?;
        if actual.as_deref() != Some(change.after.as_slice()) {
            return Err(EditError::ExternalConflict {
                path: change.path.clone(),
                expected: crate::presentation_commands::document_hash(&change.after),
                actual: actual
                    .as_deref()
                    .map(crate::presentation_commands::document_hash)
                    .unwrap_or_else(|| "缺失".into()),
            });
        }
    }
    for change in &record.changes {
        match &change.before {
            Some(bytes) => candidate
                .set_authoring_document(&change.path, bytes.clone())
                .map_err(storage_error)?,
            None => candidate
                .delete_authoring_document(&change.path)
                .map_err(storage_error)?,
        }
    }
    *project = candidate;
    *revision = revision.next_presentation();
    Ok(())
}

fn prepare(
    project: &Project,
    request: &CreateMapRequest,
    expected_documents: &BTreeMap<PathBuf, String>,
) -> Result<PreparedCreate, EditError> {
    validate_request(request)?;

    let manifest = manifest_path(&project.root);
    let relative_map = format!(".world/maps/{}.json", request.id);
    let map_path = registered_path(&project.root, &relative_map).map_err(invalid_schema)?;
    let manifest_state = current_document_state(project, &manifest)?;
    let map_state = current_document_state(project, &map_path)?;

    verify_expected_documents(project, expected_documents)?;
    let has_manifest_baseline = expected_documents
        .keys()
        .any(|path| crate::compiler::source_path(path) == manifest);
    if manifest_state.bytes.is_some() && !has_manifest_baseline {
        return Err(EditError::ExternalConflict {
            path: manifest.clone(),
            expected: "缺少清单基线".into(),
            actual: manifest_state
                .bytes
                .as_deref()
                .map(crate::presentation_commands::document_hash)
                .unwrap_or_else(|| MISSING_DOCUMENT_HASH.into()),
        });
    }

    // A loaded manifest is the only safe source for updating a registered
    // project. A disk file that is not in the Project session must be refreshed
    // first; otherwise the command could overwrite an external version.
    if manifest_state.bytes.is_some() && !manifest_state.loaded {
        return Err(EditError::ExternalConflict {
            path: manifest.clone(),
            expected: "清单已载入".into(),
            actual: manifest_state
                .bytes
                .as_deref()
                .map(crate::presentation_commands::document_hash)
                .unwrap_or_else(|| "缺失".into()),
        });
    }
    if map_state.bytes.is_some() && !map_state.loaded {
        return Err(EditError::ExternalConflict {
            path: map_path.clone(),
            expected: "地图文档已载入".into(),
            actual: map_state
                .bytes
                .as_deref()
                .map(crate::presentation_commands::document_hash)
                .unwrap_or_else(|| "缺失".into()),
        });
    }
    if map_state.bytes.is_some() {
        return Err(EditError::InvalidSchema {
            message: format!("地图文档已存在，不能覆盖：{}", map_path.display()),
        });
    }

    let manifest_after = match manifest_state.bytes.as_deref() {
        Some(bytes) => update_manifest(
            project,
            &manifest,
            bytes,
            &map_path,
            &request.id,
            &relative_map,
        )?,
        None => new_manifest(project, &request.id, &relative_map)?,
    };
    let map_after = new_map_document(request)?;

    Ok(PreparedCreate {
        manifest_path: manifest,
        map_path,
        manifest_before: manifest_state.bytes,
        map_before: map_state.bytes,
        manifest_after,
        map_after,
    })
}

fn validate_request(request: &CreateMapRequest) -> Result<(), EditError> {
    if !valid_id(&request.id) {
        return Err(EditError::InvalidSchema {
            message: format!("地图 ID `{}` 无效", request.id),
        });
    }
    if request.title.trim().is_empty() {
        return Err(EditError::InvalidSchema {
            message: "地图 title 不能为空".into(),
        });
    }
    if request.width == 0 || request.height == 0 {
        return Err(EditError::InvalidSchema {
            message: "地图 canvas 宽高必须是正整数".into(),
        });
    }
    if request.width > i32::MAX as u32 || request.height > i32::MAX as u32 {
        return Err(EditError::InvalidSchema {
            message: "地图 canvas 宽高不能超过 2^31-1".into(),
        });
    }
    Ok(())
}

fn new_manifest(project: &Project, id: &str, relative_map: &str) -> Result<Vec<u8>, EditError> {
    let entry = project
        .entry
        .strip_prefix(&project.root)
        .map_err(|_| invalid_schema("工程入口必须位于工作区内"))?
        .to_string_lossy()
        .replace('\\', "/");
    let mut maps = Map::new();
    maps.insert(id.into(), Value::String(relative_map.into()));
    let value = serde_json::json!({
        "schema_version": 1,
        "project_id": new_project_id(project),
        "language_version": "1.9",
        "entry": entry,
        "required_features": ["presentation.maps.v1"],
        "maps": maps,
        "graph_views": {},
        "extensions": {}
    });
    serialize_json(value)
}

fn new_project_id(project: &Project) -> String {
    // 旧工程没有身份字段，首次创建清单时从工作区目录名生成合法候选；
    // 后续更新沿用清单中的既有值。
    let name = project
        .root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    let mut id: String = name
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || *character == '_' || *character == '-'
        })
        .collect();
    if id.is_empty() {
        return "project".into();
    }
    if !id
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
    {
        id.insert_str(0, "project_");
    }
    id
}

fn update_manifest(
    project: &Project,
    manifest_path: &Path,
    bytes: &[u8],
    map_path: &Path,
    id: &str,
    relative_map: &str,
) -> Result<Vec<u8>, EditError> {
    let document = project
        .authoring_document(manifest_path)
        .map_err(|message| EditError::ExternalConflict {
            path: manifest_path.to_path_buf(),
            expected: "清单已载入".into(),
            actual: message,
        })?;
    if document.is_deleted() {
        return Err(EditError::ExternalConflict {
            path: manifest_path.to_path_buf(),
            expected: "清单未删除".into(),
            actual: "清单已标记删除".into(),
        });
    }
    if document.is_read_only() {
        return Err(EditError::ReadOnlyFeature {
            message: "工程清单包含当前工具不支持的格式或必需能力，只能只读查看".into(),
        });
    }

    let registry = parse_registry(&project.root, bytes);
    // WS004 表示清单注册边界不安全；Project 仍保留原始清单供修复，
    // 但本次编辑必须在构造候选清单、注册新地图前停止。
    if let Some(diagnostic) = registry
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "WS004")
    {
        return Err(EditError::InvalidSchema {
            message: format!(
                "工程清单包含无效注册项，无法创建地图：{}",
                diagnostic.message
            ),
        });
    }
    if registry.read_only(manifest_path) {
        return Err(EditError::ReadOnlyFeature {
            message: "工程清单包含当前工具不支持的格式或必需能力，只能只读查看".into(),
        });
    }
    if registry.maps.values().any(|path| path == map_path) {
        return Err(EditError::InvalidSchema {
            message: format!("地图路径已注册：{relative_map}"),
        });
    }
    let mut value = parse_unique_json(bytes).map_err(invalid_schema)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| invalid_schema("清单 JSON 顶层必须是对象"))?;
    if object.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(EditError::ReadOnlyFeature {
            message: "工程清单 schema_version 不受支持，只能只读查看".into(),
        });
    }

    let had_maps = object.contains_key("maps");
    if !had_maps {
        object.insert("maps".into(), Value::Object(Map::new()));
    }
    let maps = match object.get_mut("maps") {
        Some(value) => value
            .as_object_mut()
            .ok_or_else(|| invalid_schema("清单 maps 必须是对象"))?,
        None => unreachable!("maps 已在上面插入"),
    };
    if maps.contains_key(id) {
        return Err(EditError::InvalidSchema {
            message: format!("地图 ID `{id}` 已注册"),
        });
    }
    if maps
        .values()
        .any(|value| value.as_str() == Some(relative_map))
    {
        return Err(EditError::InvalidSchema {
            message: format!("地图路径已注册：{relative_map}"),
        });
    }
    maps.insert(id.into(), Value::String(relative_map.into()));

    match object.get_mut("required_features") {
        Some(features) => {
            let features = features
                .as_array_mut()
                .ok_or_else(|| invalid_schema("清单 required_features 必须是数组"))?;
            if !features
                .iter()
                .any(|feature| feature.as_str() == Some("presentation.maps.v1"))
            {
                features.push(Value::String("presentation.maps.v1".into()));
            }
        }
        None => {
            object.insert(
                "required_features".into(),
                Value::Array(vec![Value::String("presentation.maps.v1".into())]),
            );
        }
    }
    serialize_json(value)
}

fn new_map_document(request: &CreateMapRequest) -> Result<Vec<u8>, EditError> {
    let value = serde_json::json!({
        "schema_version": 1,
        "id": request.id,
        "title": request.title,
        "raster_layers": [],
        "canvas": {
            "width": request.width,
            "height": request.height,
            "unit": "normalized"
        },
        "layer_order": ["places"],
        "layers": {
            "places": {
                "title": "地点",
                "visible_default": true,
                "locked": false
            }
        },
        "placements": {},
        "extensions": {}
    });
    serialize_json(value)
}

fn serialize_json(value: Value) -> Result<Vec<u8>, EditError> {
    serde_json::to_vec_pretty(&value).map_err(|error| EditError::StorageFailure {
        message: format!("展示文档序列化失败：{error}"),
    })
}

struct DocumentState {
    bytes: Option<Vec<u8>>,
    loaded: bool,
}

fn current_document_state(project: &Project, path: &Path) -> Result<DocumentState, EditError> {
    if let Some(document) = project.authoring_documents.get(path) {
        return Ok(DocumentState {
            bytes: (!document.is_deleted()).then(|| document.bytes().to_vec()),
            loaded: true,
        });
    }
    match crate::file_access::read(path) {
        Ok(bytes) => Ok(DocumentState {
            bytes: Some(bytes),
            loaded: false,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DocumentState {
            bytes: None,
            loaded: false,
        }),
        Err(error) => Err(EditError::StorageFailure {
            message: format!("无法读取展示文档 {}：{error}", path.display()),
        }),
    }
}

fn current_loaded_bytes(project: &Project, path: &Path) -> Result<Option<Vec<u8>>, EditError> {
    Ok(project
        .authoring_documents
        .get(path)
        .filter(|document| !document.is_deleted())
        .map(|document| document.bytes().to_vec()))
}

fn verify_expected_documents(
    project: &Project,
    expected_documents: &BTreeMap<PathBuf, String>,
) -> Result<(), EditError> {
    for (path, expected) in expected_documents {
        let path = crate::compiler::source_path(path);
        crate::file_access::within(&project.root, &path).map_err(invalid_schema)?;
        let actual = current_document_state(project, &path)?.bytes;
        let actual_hash = actual
            .as_deref()
            .map(crate::presentation_commands::document_hash)
            .unwrap_or_else(|| MISSING_DOCUMENT_HASH.into());
        if actual_hash != *expected {
            return Err(EditError::ExternalConflict {
                path,
                expected: expected.clone(),
                actual: actual_hash,
            });
        }
    }
    Ok(())
}

fn storage_error(message: String) -> EditError {
    EditError::StorageFailure { message }
}

fn invalid_schema(message: impl Into<String>) -> EditError {
    EditError::InvalidSchema {
        message: message.into(),
    }
}

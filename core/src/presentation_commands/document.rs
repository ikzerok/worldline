use super::*;
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) fn apply_to_document(
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
    let mut root = crate::workspace_documents::parse_unique_json(&before)
        .map_err(|message| EditError::InvalidSchema { message })?;
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
        Command::SetMapMeasurement { measurement, .. } => {
            measurement::apply_measurement(object, measurement)?;
        }
        Command::CreatePlacement { .. }
        | Command::UpdatePlacement { .. }
        | Command::DeletePlacement { .. } => {
            placements::apply_placement(
                object,
                command,
                catalog,
                source_unresolved,
                &mut affected,
            )?;
        }
        Command::CreateLayer { .. } | Command::SetLayer { .. } | Command::DeleteLayer { .. } => {
            layers::apply_layer(object, command)?;
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

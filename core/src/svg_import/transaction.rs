//! 对副本逐项验证后一次提交；失败不会创建半个图层。
use super::*;
use crate::presentation_commands::{
    self as commands, Command, CommandEnvelope, EditError, Revision,
};
use crate::project::Project;
use std::path::PathBuf;
/// 导入原始 SVG，解析与修改都在 core，调用方须提供打开预览时的基线。
pub fn apply(
    project: &mut Project,
    revision: &mut Revision,
    map_id: &str,
    layer_id: &str,
    source: &str,
    expected_revision: Revision,
    expected_documents: BTreeMap<PathBuf, String>,
) -> Result<usize, EditError> {
    let invalid = |message| EditError::InvalidGeometry { message };
    let preview = preview(source).map_err(invalid)?;
    let mut candidate = project.clone();
    let mut next = *revision;
    let content = project.compile();
    commands::apply_with_content(
        &mut candidate,
        &mut next,
        CommandEnvelope {
            expected_revision,
            expected_documents,
            command: Command::CreateLayer {
                map_id: map_id.into(),
                layer_id: layer_id.into(),
                title: "SVG 绘图".into(),
                visible_default: true,
                locked: false,
            },
        },
        &content,
    )?;
    let path = commands::map_document_path(&candidate, map_id)?;
    for (i, shape) in preview.shapes.iter().enumerate() {
        let hash = commands::document_hash(
            candidate
                .authoring_document(&path)
                .map_err(|message| EditError::StorageFailure { message })?
                .bytes(),
        );
        let expected_revision = next;
        commands::apply_with_content(
            &mut candidate,
            &mut next,
            CommandEnvelope {
                expected_revision,
                expected_documents: BTreeMap::from([(path.clone(), hash)]),
                command: Command::CreatePlacement {
                    map_id: map_id.into(),
                    placement_id: format!("{layer_id}_{:04}", i + 1),
                    layer_id: layer_id.into(),
                    target_ref: None,
                    geometry: shape.geometry.clone(),
                    annotation: "SVG 图形".into(),
                    role: "illustration".into(),
                    label_override: None,
                },
            },
            &content,
        )?;
    }
    // style 已经过严格白名单检查，只向刚创建的图形补充安全展示属性。
    let mut document: Value = serde_json::from_slice(
        candidate
            .authoring_document(&path)
            .map_err(|message| EditError::StorageFailure { message })?
            .bytes(),
    )
    .map_err(|e| invalid(e.to_string()))?;
    for (i, shape) in preview.shapes.iter().enumerate() {
        document["placements"][format!("{layer_id}_{:04}", i + 1)]["style"] =
            Value::Object(shape.style.clone());
    }
    candidate
        .set_authoring_document(
            &path,
            serde_json::to_vec_pretty(&document).map_err(|e| invalid(e.to_string()))?,
        )
        .map_err(|message| EditError::StorageFailure { message })?;
    *project = candidate;
    *revision = revision.next_presentation();
    Ok(preview.shapes.len())
}

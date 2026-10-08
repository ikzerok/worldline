use super::*;

pub(super) fn create(
    project: &Project,
    source: ProjectTemplateDraftSource,
    content: &CompileResult,
) -> Result<ProjectTemplateDraft, String> {
    let context = DraftContext::new(project);
    let value = match source {
        ProjectTemplateDraftSource::Json { bytes } => {
            return Ok(reservations::initialize(bytes, None));
        }
        ProjectTemplateDraftSource::Existing { id } => {
            let bytes = existing_bytes(project, &context, &id)?;
            return Ok(reservations::initialize(bytes, Some(id)));
        }
        ProjectTemplateDraftSource::New => json!({
            "schema_version": 1, "id": context.next_id("new_template"),
            "title": "新工程模板", "applies_to": {"kind": "entity", "entity_type": "place"},
            "fields": []
        }),
        ProjectTemplateDraftSource::Copy { id } => {
            let mut value = if let Some(builtin) = crate::content_templates::builtin_templates()
                .templates
                .iter()
                .find(|template| template.id == id)
            {
                // 使用内置目录的原对象，保留 typed catalog 未投影的可选扩展。
                let catalog =
                    parse_unique_json(include_bytes!("../../../../spec/templates.catalog.json"))?;
                let mut value = catalog["templates"]
                    .as_array()
                    .and_then(|templates| {
                        templates
                            .iter()
                            .find(|template| template["id"].as_str() == Some(builtin.id.as_str()))
                    })
                    .cloned()
                    .ok_or("内置模板原文不存在")?;
                let fields = value["fields"]
                    .as_array_mut()
                    .ok_or("内置模板 fields 无效")?;
                for (value, field) in fields.iter_mut().zip(&builtin.fields) {
                    value["id"] = field.key.clone().into();
                    value["type"] = match field.widget.as_str() {
                        "number" => "number",
                        "boolean" => "boolean",
                        _ => "text",
                    }
                    .into();
                }
                value
            } else {
                let draft =
                    reservations::initialize(existing_bytes(project, &context, &id)?, Some(id));
                require_editable(&project.project_template_draft_projection(&draft, content))?;
                parse_unique_json(&draft.source_bytes)?
            };
            value["id"] = context.next_id("template_copy").into();
            value["title"] = format!("{} 副本", value["title"].as_str().unwrap_or("模板")).into();
            value
        }
    };
    // 新建不承接任何注册身份；仅显式应用才创建文档。
    let draft = reservations::initialize(
        serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
        None,
    );
    require_editable(&project.project_template_draft_projection(&draft, content))?;
    Ok(draft)
}

fn existing_bytes(project: &Project, context: &DraftContext, id: &str) -> Result<Vec<u8>, String> {
    let path = context.template_path(project, id).ok_or("工程模板未注册")?;
    project
        .authoring_documents
        .get(&path)
        .filter(|document| !document.is_deleted())
        .map(|document| document.bytes().to_vec())
        .ok_or_else(|| "模板文档未载入".into())
}

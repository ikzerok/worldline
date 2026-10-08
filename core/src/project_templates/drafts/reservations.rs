use super::*;

pub(super) fn initialize(
    source_bytes: Vec<u8>,
    existing_id: Option<String>,
) -> ProjectTemplateDraft {
    let mut ids = BTreeSet::new();
    let mut keys = BTreeSet::new();
    if let Ok(value) = parse_unique_json(&source_bytes) {
        edits::collect_names(&value, &mut ids, &mut keys);
    }
    ProjectTemplateDraft {
        source_bytes,
        existing_id,
        reserved_field_ids: ids.into_iter().collect(),
        reserved_keys: keys.into_iter().collect(),
    }
}

/// 保留输入历史、当前树及绑定 Project 原树；旧 DTO 缺少历史也不会复用原字段。
pub(super) fn for_edit(
    project: &Project,
    draft: &ProjectTemplateDraft,
    value: &Value,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut ids = draft.reserved_field_ids.iter().cloned().collect();
    let mut keys = draft.reserved_keys.iter().cloned().collect();
    edits::collect_names(value, &mut ids, &mut keys);
    let context = DraftContext::new(project);
    if let Some(id) = draft
        .existing_id
        .as_deref()
        .or_else(|| value.get("id").and_then(Value::as_str))
    {
        if let Some(path) = context.template_path(project, id) {
            if let Some(document) = project
                .authoring_documents
                .get(&path)
                .filter(|document| !document.is_deleted())
            {
                if let Ok(value) = parse_unique_json(document.bytes()) {
                    edits::collect_names(&value, &mut ids, &mut keys);
                }
            }
        }
    }
    (ids, keys)
}

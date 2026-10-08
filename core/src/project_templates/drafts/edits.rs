use super::*;

pub(super) fn apply(
    value: &mut Value,
    edit: &ProjectTemplateDraftEdit,
    ids: &mut BTreeSet<String>,
    keys: &mut BTreeSet<String>,
) -> Result<(), String> {
    match edit {
        ProjectTemplateDraftEdit::SetMetadata { title, applies_to } => {
            value["title"] = title.clone().into();
            update_target(&mut value["applies_to"], applies_to);
        }
        ProjectTemplateDraftEdit::AddField {
            parent_id,
            index,
            field_type,
        } => {
            let field = new_field(field_type, ids, keys);
            insert(value, parent_id.as_deref(), *index, field)?;
        }
        ProjectTemplateDraftEdit::UpdateField {
            field_id,
            properties,
        } => {
            let field = find_mut(value, field_id).ok_or("待修改字段不存在")?;
            update_field(field, properties)?;
            if properties.field_type == ProjectTemplateFieldType::ObjectRef
                && properties
                    .target
                    .as_ref()
                    .is_some_and(|target| target.kind == "character")
            {
                let features = value
                    .as_object_mut()
                    .ok_or("模板顶层必须为对象")?
                    .entry("required_features")
                    .or_insert_with(|| json!([]));
                let features = features
                    .as_array_mut()
                    .ok_or("required_features 必须为数组")?;
                if !features
                    .iter()
                    .any(|v| v.as_str() == Some(CHARACTER_REFS_REQUIRED_FEATURE))
                {
                    features.push(CHARACTER_REFS_REQUIRED_FEATURE.into());
                }
            }
        }
        ProjectTemplateDraftEdit::DeleteField { field_id } => {
            remove(value, field_id).ok_or("待删除字段不存在")?;
        }
        ProjectTemplateDraftEdit::MoveField {
            field_id,
            parent_id,
            index,
        } => {
            let field = find_mut(value, field_id).ok_or("待移动字段不存在")?;
            if parent_id
                .as_deref()
                .is_some_and(|id| contains_id(field, id))
            {
                return Err("不能将分组移动到自身或后代".into());
            }
            let field = remove(value, field_id).ok_or("待移动字段不存在")?;
            insert(value, parent_id.as_deref(), *index, field)?;
        }
    }
    Ok(())
}

fn update_target(value: &mut Value, target: &ProjectTemplateDraftTarget) {
    if !value.is_object() {
        *value = json!({});
    }
    let object = value.as_object_mut().expect("已建立目标对象");
    object.insert("kind".into(), target.kind.clone().into());
    object.remove("entity_type");
    if let Some(entity_type) = &target.entity_type {
        object.insert("entity_type".into(), entity_type.clone().into());
    }
}

fn update_field(
    field: &mut Value,
    properties: &ProjectTemplateFieldProperties,
) -> Result<(), String> {
    let was_group = field.get("type").and_then(Value::as_str) == Some("group");
    if was_group != (properties.field_type == ProjectTemplateFieldType::Group) {
        return Err("分组与普通字段不能隐式互换类型；请明确删除后新增".into());
    }
    let mut old_target = field.get("target").cloned().unwrap_or_else(|| json!({}));
    let object = field.as_object_mut().ok_or("字段必须为对象")?;
    for key in [
        "key", "label", "type", "required", "choices", "target", "default",
    ] {
        object.remove(key);
    }
    object.insert("label".into(), properties.label.clone().into());
    object.insert("type".into(), properties.field_type.as_str().into());
    if let Some(key) = &properties.key {
        object.insert("key".into(), key.clone().into());
    }
    if !was_group || properties.required {
        object.insert("required".into(), properties.required.into());
    }
    if properties.field_type == ProjectTemplateFieldType::Enum || !properties.choices.is_empty() {
        object.insert("choices".into(), json!(properties.choices));
    }
    if let Some(target) = &properties.target {
        update_target(&mut old_target, target);
        object.insert("target".into(), old_target);
    }
    if let Some(default) = &properties.default {
        object.insert("default".into(), default.clone());
    }
    Ok(())
}

pub(super) fn collect_names(
    value: &Value,
    ids: &mut BTreeSet<String>,
    keys: &mut BTreeSet<String>,
) {
    if let Some(fields) = value.get("fields").and_then(Value::as_array) {
        for field in fields {
            if let Some(id) = field.get("id").and_then(Value::as_str) {
                ids.insert(id.into());
            }
            if let Some(key) = field.get("key").and_then(Value::as_str) {
                keys.insert(key.into());
            }
            collect_names(field, ids, keys);
        }
    }
}

fn allocate(names: &mut BTreeSet<String>, prefix: &str) -> String {
    let name = (1..)
        .map(|n| format!("{prefix}_{n}"))
        .find(|name| !names.contains(name))
        .expect("可用字段名称");
    names.insert(name.clone());
    name
}

fn new_field(
    kind: &ProjectTemplateFieldType,
    ids: &mut BTreeSet<String>,
    keys: &mut BTreeSet<String>,
) -> Value {
    let id = allocate(
        ids,
        if *kind == ProjectTemplateFieldType::Group {
            "group"
        } else {
            "field"
        },
    );
    if *kind == ProjectTemplateFieldType::Group {
        return json!({"id": id, "label": "新分组", "type": "group",
            "fields": [new_field(&ProjectTemplateFieldType::Text, ids, keys)]});
    }
    let key = allocate(keys, "property");
    let mut field =
        json!({"id": id, "key": key, "label": "新字段", "type": kind.as_str(), "required": false});
    match kind {
        ProjectTemplateFieldType::Enum => field["choices"] = json!(["选项一", "选项二"]),
        ProjectTemplateFieldType::ObjectRef => field["target"] = json!({"kind": "entity"}),
        _ => {}
    }
    field
}

fn find_mut<'a>(value: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    for field in value.get_mut("fields")?.as_array_mut()? {
        if field.get("id").and_then(Value::as_str) == Some(id) {
            return Some(field);
        }
        if let Some(found) = find_mut(field, id) {
            return Some(found);
        }
    }
    None
}

fn contains_id(value: &Value, id: &str) -> bool {
    value.get("id").and_then(Value::as_str) == Some(id)
        || value
            .get("fields")
            .and_then(Value::as_array)
            .is_some_and(|fields| fields.iter().any(|field| contains_id(field, id)))
}

fn remove(value: &mut Value, id: &str) -> Option<Value> {
    let fields = value.get_mut("fields")?.as_array_mut()?;
    if let Some(index) = fields
        .iter()
        .position(|field| field.get("id").and_then(Value::as_str) == Some(id))
    {
        return Some(fields.remove(index));
    }
    fields.iter_mut().find_map(|field| remove(field, id))
}

fn insert(
    value: &mut Value,
    parent: Option<&str>,
    index: usize,
    field: Value,
) -> Result<(), String> {
    let parent = if let Some(id) = parent {
        let parent = find_mut(value, id).ok_or("目标分组不存在")?;
        if parent.get("type").and_then(Value::as_str) != Some("group") {
            return Err("目标父字段必须为分组".into());
        }
        parent
    } else {
        value
    };
    let fields = parent
        .get_mut("fields")
        .and_then(Value::as_array_mut)
        .ok_or("目标 fields 必须为数组")?;
    if index > fields.len() {
        return Err("目标插入位置越界".into());
    }
    fields.insert(index, field);
    Ok(())
}

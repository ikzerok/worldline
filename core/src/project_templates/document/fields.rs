use super::*;
mod impacts;

fn parse_template_document(
    bytes: &[u8],
    file: &str,
    registered_id: &str,
    manifest_features: &BTreeSet<String>,
    registered_read_only: bool,
    language_version: crate::LanguageVersion,
    content: &CompileResult,
) -> ProjectTemplateDocument {
    let mut entry = ProjectTemplateDocument {
        file: file.into(),
        template: None,
        source_document: None,
        source_bytes: bytes.to_vec(),
        diagnostics: Vec::new(),
        read_only: registered_read_only,
    };
    let value = match parse_unique_json(bytes) {
        Ok(value) => value,
        Err(error) => {
            entry.error("TPL001", file, 1, format!("模板 JSON 无法解析：{error}"));
            entry.read_only = true;
            return entry;
        }
    };
    entry.source_document = Some(value.clone());
    let Some(object) = value.as_object() else {
        entry.error("TPL001", file, 1, "模板文档顶层必须是对象");
        entry.read_only = true;
        return entry;
    };
    if object.get("schema_version").and_then(Value::as_u64) != Some(1) {
        entry.error(
            "TPL002",
            file,
            line_for(bytes, "schema_version"),
            "模板格式版本不受支持，按只读处理",
        );
        entry.read_only = true;
    }
    if let Some(features) = object.get("required_features") {
        match features.as_array() {
            Some(features) => {
                for feature in features {
                    let Some(feature) = feature.as_str() else {
                        entry.error(
                            "TPL002",
                            file,
                            line_for(bytes, "required_features"),
                            "模板必需能力必须是字符串，按只读处理",
                        );
                        entry.read_only = true;
                        continue;
                    };
                    if !supported_template_feature(feature) {
                        entry.error(
                            "TPL002",
                            file,
                            line_for(bytes, feature),
                            format!("模板包含未知必需能力 `{feature}`，按只读处理"),
                        );
                        entry.read_only = true;
                    }
                }
            }
            None => {
                entry.error(
                    "TPL002",
                    file,
                    line_for(bytes, "required_features"),
                    "required_features 必须是数组，按只读处理",
                );
                entry.read_only = true;
            }
        }
    }
    let Some(id) = object.get("id").and_then(Value::as_str) else {
        entry.error(
            "TPL003",
            file,
            line_for(bytes, "id"),
            "模板 id 必须是字符串",
        );
        return entry;
    };
    if !crate::workspace_documents::valid_template_id(id) {
        entry.error(
            "TPL003",
            file,
            line_for(bytes, id),
            "工程模板 ID 必须使用 project: 命名空间",
        );
        return entry;
    }
    if id != registered_id {
        entry.error(
            "TPL003",
            file,
            line_for(bytes, id),
            format!("模板 id `{id}` 与清单注册 ID `{registered_id}` 不一致"),
        );
        return entry;
    }
    let title = match object.get("title").and_then(Value::as_str) {
        Some(title) if !title.trim().is_empty() => title.to_owned(),
        _ => {
            entry.error(
                "TPL004",
                file,
                line_for(bytes, "title"),
                "模板 title 必须是非空字符串",
            );
            return entry;
        }
    };
    let Some(applies_to) = object.get("applies_to").and_then(Value::as_object) else {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, "applies_to"),
            "模板 applies_to 必须是对象",
        );
        return entry;
    };
    let Some(kind) = applies_to.get("kind").and_then(Value::as_str) else {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, "kind"),
            "模板适用类型必须是字符串",
        );
        return entry;
    };
    if !matches!(kind, "world" | "character" | "entity") {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, kind),
            format!("模板适用目标 `{kind}` 无效"),
        );
        return entry;
    }
    let entity_type = match applies_to.get("entity_type") {
        None => None,
        Some(Value::String(value)) if !value.trim().is_empty() => Some(value.clone()),
        Some(_) => {
            entry.error(
                "TPL004",
                file,
                line_for(bytes, "entity_type"),
                "applies_to.entity_type 必须是非空字符串",
            );
            return entry;
        }
    };
    if kind != "entity" && entity_type.is_some() {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, "entity_type"),
            "只有 entity 模板可以声明 entity_type",
        );
        return entry;
    }
    let Some(fields) = object.get("fields").and_then(Value::as_array) else {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, "fields"),
            "模板 fields 必须是数组",
        );
        return entry;
    };
    let mut ids = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut parsed_fields = Vec::new();
    for field in fields {
        match parse_field(field, bytes, file, &mut ids, &mut keys, &mut entry) {
            Some(field) => parsed_fields.push(field),
            None => continue,
        }
    }
    if parsed_fields.len() != fields.len() {
        return entry;
    }
    let has_object_ref = parsed_fields.iter().any(field_contains_object_ref);
    if has_object_ref && !language_version.supports_entities() {
        entry.error(
            "TPL005",
            file,
            line_for(bytes, "object_ref"),
            "对象引用字段要求语言 1.10，模板按只读处理",
        );
        entry.read_only = true;
    }
    if has_object_ref && !manifest_features.contains(OBJECT_REFS_REQUIRED_FEATURE) {
        entry.error(
            "TPL005",
            file,
            line_for(bytes, "object_ref"),
            format!("对象引用字段要求清单声明 `{OBJECT_REFS_REQUIRED_FEATURE}`，模板按只读处理"),
        );
        entry.read_only = true;
    }
    if parsed_fields.iter().any(field_contains_character_ref) {
        let document_declares = object
            .get("required_features")
            .and_then(Value::as_array)
            .is_some_and(|features| {
                features
                    .iter()
                    .any(|feature| feature.as_str() == Some(CHARACTER_REFS_REQUIRED_FEATURE))
            });
        if !language_version.supports_language_113()
            || !manifest_features.contains(CHARACTER_REFS_REQUIRED_FEATURE)
            || !document_declares
        {
            entry.error("TPL005", file, line_for(bytes, "character"),
                "人物引用字段要求显式语言 1.13、清单及模板文档声明 content.character_refs.v1，模板按只读处理");
            entry.read_only = true;
        }
    }
    for field in &parsed_fields {
        validate_field_targets(field, &content.analysis.catalog, bytes, file, &mut entry);
    }
    entry.template = Some(ProjectTemplate {
        id: id.to_owned(),
        title,
        applies_to: TargetRef::new(kind, ""),
        applies_to_entity_type: entity_type,
        fields: parsed_fields,
    });
    entry
}

fn parse_field(
    value: &Value,
    bytes: &[u8],
    file: &str,
    ids: &mut BTreeSet<String>,
    keys: &mut BTreeSet<String>,
    entry: &mut ProjectTemplateDocument,
) -> Option<ProjectTemplateField> {
    let Some(object) = value.as_object() else {
        entry.error("TPL004", file, 1, "模板字段必须是对象");
        return None;
    };
    let get_string = |name: &str| object.get(name).and_then(Value::as_str);
    let Some(id) = get_string("id") else {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, "id"),
            "模板字段 id 必须是字符串",
        );
        return None;
    };
    let line = line_for(bytes, id);
    if !crate::workspace_documents::valid_id(id) {
        entry.error("TPL004", file, line, format!("字段 ID `{id}` 无效"));
        return None;
    }
    if !ids.insert(id.to_owned()) {
        entry.error(
            "TPL004",
            file,
            line_for_last(bytes, id),
            format!("字段 ID `{id}` 重复"),
        );
        return None;
    }
    let label = match get_string("label") {
        Some(label) if !label.trim().is_empty() => label.to_owned(),
        _ => {
            entry.error(
                "TPL004",
                file,
                line_for(bytes, "label"),
                format!("字段 `{id}` 的 label 必须非空"),
            );
            return None;
        }
    };
    let field_type = match get_string("type") {
        Some(kind)
            if matches!(
                kind,
                "text" | "number" | "boolean" | "enum" | "object_ref" | "group"
            ) =>
        {
            kind.to_owned()
        }
        _ => {
            entry.error("TPL004", file, line, format!("字段 `{id}` 的 type 无效"));
            return None;
        }
    };
    let key = get_string("key").map(str::to_owned);
    let required = object
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let choices = match object.get("choices") {
        Some(Value::Array(values)) if values.iter().all(Value::is_string) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        Some(_) => {
            entry.error(
                "TPL004",
                file,
                line_for(bytes, "choices"),
                format!("字段 `{id}` 的 choices 必须是字符串数组"),
            );
            return None;
        }
        None => Vec::new(),
    };
    let target = object
        .get("target")
        .and_then(Value::as_object)
        .and_then(|target| Some(TargetRef::new(target.get("kind")?.as_str()?, "")));
    let target_entity_type = object
        .get("target")
        .and_then(Value::as_object)
        .and_then(|target| target.get("entity_type"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    if object
        .get("target")
        .and_then(Value::as_object)
        .is_some_and(|target| target.contains_key("entity_type"))
        && target_entity_type
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
    {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, "entity_type"),
            format!("字段 `{id}` target.entity_type 必须是非空字符串"),
        );
        return None;
    }
    let default = object.get("default").cloned();
    let children = object.get("fields").and_then(Value::as_array);
    let mut parsed_children = Vec::new();
    if let Some(children) = children {
        for child in children {
            if let Some(field) = parse_field(child, bytes, file, ids, keys, entry) {
                parsed_children.push(field);
            }
        }
    }

    if field_type == "group" {
        if key.is_some()
            || object.contains_key("required")
            || object.contains_key("choices")
            || object.contains_key("target")
            || default.is_some()
        {
            entry.error(
                "TPL004",
                file,
                line,
                format!("分组 `{id}` 不能声明 key、required、choices、target 或 default"),
            );
            return None;
        }
        if parsed_children.is_empty() {
            entry.error("TPL004", file, line, format!("分组 `{id}` 必须包含字段"));
            return None;
        }
    } else {
        if children.is_some() {
            entry.error(
                "TPL004",
                file,
                line,
                format!("非分组字段 `{id}` 不能包含子字段"),
            );
            return None;
        }
        if !key
            .as_deref()
            .is_some_and(crate::workspace_documents::valid_id)
        {
            entry.error(
                "TPL004",
                file,
                line,
                format!("字段 `{id}` 必须提供有效 key"),
            );
            return None;
        }
        if !keys.insert(key.clone().unwrap()) {
            entry.error(
                "TPL004",
                file,
                line_for_last(bytes, key.as_deref().unwrap()),
                format!("实例字段 key `{}` 重复", key.as_deref().unwrap()),
            );
            return None;
        }
        if !object.get("required").is_some_and(Value::is_boolean) {
            entry.error(
                "TPL004",
                file,
                line,
                format!("字段 `{id}` 的 required 必须是布尔值"),
            );
            return None;
        }
        if field_type == "enum" {
            let mut seen = BTreeSet::new();
            if choices.is_empty()
                || choices
                    .iter()
                    .any(|item| item.trim().is_empty() || !seen.insert(item))
            {
                entry.error(
                    "TPL004",
                    file,
                    line,
                    format!("枚举字段 `{id}` 必须有非空且不重复的 choices"),
                );
                return None;
            }
        } else if object.contains_key("choices") {
            entry.error(
                "TPL004",
                file,
                line,
                format!("字段 `{id}` 只有 enum 可以声明 choices"),
            );
            return None;
        }
        if field_type == "object_ref" {
            let valid_target = target.as_ref().is_some_and(|target| {
                crate::catalog::OBJECT_REFERENCE_TARGET_KINDS.contains(&target.kind.as_str())
                    && (target.kind == "entity" || target_entity_type.is_none())
            });
            if !valid_target {
                entry.error(
                    "TPL004",
                    file,
                    line,
                    format!("对象引用字段 `{id}` 必须指定有效 target"),
                );
                return None;
            }
        } else if object.contains_key("target") {
            entry.error(
                "TPL004",
                file,
                line,
                format!("字段 `{id}` 只有 object_ref 可以声明 target"),
            );
            return None;
        }
        if !valid_default(&field_type, default.as_ref(), &choices, target.as_ref()) {
            entry.error(
                "TPL004",
                file,
                line_for(bytes, "default"),
                format!("字段 `{id}` 的 default 与类型不匹配"),
            );
            return None;
        }
    }
    Some(ProjectTemplateField {
        id: id.to_owned(),
        key,
        label,
        field_type,
        required,
        choices,
        target,
        target_entity_type,
        default,
        fields: parsed_children,
    })
}

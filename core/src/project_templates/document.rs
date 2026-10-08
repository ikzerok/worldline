use super::*;
pub(super) mod fields;
mod repair_guard;

fn line_for(bytes: &[u8], needle: &str) -> u32 {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return 1;
    };
    text.find(needle)
        .map(|position| {
            text[..position]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count() as u32
                + 1
        })
        .unwrap_or(1)
}

fn line_for_last(bytes: &[u8], needle: &str) -> u32 {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return 1;
    };
    text.rfind(needle)
        .map(|position| {
            text[..position]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count() as u32
                + 1
        })
        .unwrap_or(1)
}

impl ProjectTemplateDocument {
    fn error(&mut self, code: &'static str, file: &str, line: u32, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(
            code,
            file,
            Span::new(line, 1, 1),
            message,
        ));
    }
}
fn preserve_unknown_fields(old: &Value, new: &mut Value, context: &str) {
    fn index_fields<'a>(value: &'a Value, fields: &mut HashMap<&'a str, &'a Value>) {
        if let Some(children) = value.get("fields").and_then(Value::as_array) {
            for field in children {
                if let Some(id) = field.get("id").and_then(Value::as_str) {
                    fields.insert(id, field);
                }
                index_fields(field, fields);
            }
        }
    }
    fn merge(old: &Value, new: &mut Value, context: &str, fields: &HashMap<&str, &Value>) {
        let (Value::Object(old), Value::Object(new)) = (old, new) else {
            return;
        };
        let known: &[&str] = match context {
            "root" => &[
                "schema_version",
                "id",
                "title",
                "applies_to",
                "fields",
                "required_features",
            ],
            "applies_to" | "target" => &["kind", "entity_type"],
            "field" => &[
                "id", "key", "label", "type", "required", "choices", "target", "fields", "default",
            ],
            _ => &[],
        };
        for (key, value) in old {
            if !known.contains(&key.as_str()) && !new.contains_key(key) {
                new.insert(key.clone(), value.clone());
            }
        }
        for key in ["applies_to", "target"] {
            if let (Some(old), Some(new)) = (old.get(key), new.get_mut(key)) {
                merge(old, new, key, fields);
            }
        }
        if let Some(Value::Array(children)) = new.get_mut("fields") {
            merge_children(children, fields);
        }
    }
    fn merge_children(children: &mut [Value], fields: &HashMap<&str, &Value>) {
        for field in children {
            let old = field
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| fields.get(id))
                .copied();
            if let Some(old) = old {
                merge(old, field, "field", fields);
            } else if let Some(Value::Array(children)) = field.get_mut("fields") {
                // 新父组中的旧字段仍按全模板稳定身份匹配；删除项不遍历也不复活。
                merge_children(children, fields);
            }
        }
    }
    let mut fields = HashMap::new();
    index_fields(old, &mut fields);
    merge(old, new, context, &fields);
}
fn valid_default(
    field_type: &str,
    default: Option<&Value>,
    choices: &[String],
    target: Option<&TargetRef>,
) -> bool {
    let Some(default) = default else { return true };
    match field_type {
        "text" => default.is_string(),
        "number" => default.as_f64().is_some_and(f64::is_finite),
        "boolean" => default.is_boolean(),
        "enum" => default
            .as_str()
            .is_some_and(|value| choices.iter().any(|choice| choice == value)),
        "object_ref" => default.as_object().is_some_and(|value| {
            value.len() == 2
                && value
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| target.is_some_and(|target| kind == target.kind))
                && value
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !id.trim().is_empty())
        }),
        _ => false,
    }
}

fn validate_field_targets(
    field: &ProjectTemplateField,
    catalog: &crate::catalog::Catalog,
    bytes: &[u8],
    file: &str,
    entry: &mut ProjectTemplateDocument,
) {
    if field.field_type == "object_ref" {
        if let Some(target) = field.target.as_ref() {
            let id = field
                .default
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|object| object.get("id"))
                .and_then(Value::as_str);
            if let Some(id) = id {
                let target_ref = TargetRef::new(&target.kind, id);
                let object_exists = catalog.object(&target_ref).is_some();
                let entity_type_matches =
                    field.target_entity_type.as_ref().is_none_or(|expected| {
                        catalog
                            .entities
                            .get(id)
                            .is_some_and(|entity| &entity.entity_type == expected)
                    });
                if !object_exists || !entity_type_matches {
                    entry.error(
                        "TPL006",
                        file,
                        line_for(bytes, id),
                        format!("默认对象引用 {} `{id}` 不存在", target.kind),
                    );
                }
            }
        }
    }
    for child in &field.fields {
        validate_field_targets(child, catalog, bytes, file, entry);
    }
}

fn field_contains_object_ref(field: &ProjectTemplateField) -> bool {
    field.field_type == "object_ref" || field.fields.iter().any(field_contains_object_ref)
}

fn supported_template_feature(feature: &str) -> bool {
    matches!(
        feature,
        PROJECT_TEMPLATE_REQUIRED_FEATURE
            | OBJECT_REFS_REQUIRED_FEATURE
            | CHARACTER_REFS_REQUIRED_FEATURE
    )
}

fn field_contains_character_ref(field: &ProjectTemplateField) -> bool {
    (field.field_type == "object_ref"
        && field
            .target
            .as_ref()
            .is_some_and(|target| target.kind == "character"))
        || field.fields.iter().any(field_contains_character_ref)
}

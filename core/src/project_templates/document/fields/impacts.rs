use super::*;
mod commands;

fn field_changes(
    old: Option<&ProjectTemplate>,
    new: Option<&ProjectTemplate>,
) -> Vec<ProjectTemplateFieldChange> {
    let old_fields = old.map(flatten_fields).unwrap_or_default();
    let new_fields = new.map(flatten_fields).unwrap_or_default();
    let ids: BTreeSet<_> = old_fields
        .keys()
        .chain(new_fields.keys())
        .cloned()
        .collect();
    ids.into_iter()
        .filter_map(|id| {
            let old = old_fields.get(&id);
            let new = new_fields.get(&id);
            let change = match (old, new) {
                (None, Some(_)) => "added",
                (Some(_), None) => "removed",
                (Some(old), Some(new)) if old.key != new.key => "renamed",
                (Some(old), Some(new)) if old.field_type != new.field_type => "type_changed",
                (Some(old), Some(new)) if old.label != new.label => "label_changed",
                (Some(old), Some(new))
                    if old.required != new.required
                        || old.default != new.default
                        || old.choices != new.choices =>
                {
                    "constraints_changed"
                }
                _ => return None,
            };
            Some(ProjectTemplateFieldChange {
                field_id: id,
                change: change.into(),
                old_key: old.and_then(|field| field.key.clone()),
                new_key: new.and_then(|field| field.key.clone()),
                old_type: old.map(|field| field.field_type.clone()),
                new_type: new.map(|field| field.field_type.clone()),
            })
        })
        .collect()
}

fn flatten_fields(template: &ProjectTemplate) -> BTreeMap<String, ProjectTemplateField> {
    fn add(field: &ProjectTemplateField, fields: &mut BTreeMap<String, ProjectTemplateField>) {
        fields.insert(field.id.clone(), field.clone());
        for child in &field.fields {
            add(child, fields);
        }
    }
    let mut fields = BTreeMap::new();
    for field in &template.fields {
        add(field, &mut fields);
    }
    fields
}

fn instance_impacts(
    old: Option<&ProjectTemplate>,
    new: Option<&ProjectTemplate>,
    content: &CompileResult,
    check_integrity: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<ProjectTemplateInstanceImpact> {
    let mut templates = Vec::new();
    if let Some(old) = old {
        templates.push((old, "current"));
    }
    if let Some(new) = new {
        if old != Some(new) {
            templates.push((new, "proposed"));
        }
    }
    let mut objects = BTreeMap::<TargetRef, BTreeMap<String, PropertyValue>>::new();
    for (template, _) in &templates {
        for object in content
            .analysis
            .catalog
            .objects
            .iter()
            .filter(|object| object.target.kind == template.applies_to.kind)
        {
            let properties = match object.target.kind.as_str() {
                "entity" => content
                    .analysis
                    .catalog
                    .entities
                    .get(&object.target.id)
                    .filter(|entity| {
                        template
                            .applies_to_entity_type
                            .as_ref()
                            .is_none_or(|kind| kind == &entity.entity_type)
                    })
                    .map(|entity| &entity.properties),
                "character" => content
                    .analysis
                    .symbols
                    .characters
                    .get(&object.target.id)
                    .map(|character| &character.properties),
                "world" => content
                    .analysis
                    .world
                    .as_ref()
                    .map(|world| &world.properties),
                _ => None,
            };
            if let Some(properties) = properties {
                objects.insert(object.target.clone(), properties.clone());
            }
        }
    }
    objects
        .into_iter()
        .map(|(target, properties)| {
            let mut fields = Vec::new();
            for (template, template_state) in &templates {
                for field in flatten_fields(template).into_values() {
                    let Some(key) = field.key.as_ref() else {
                        continue;
                    };
                    let value = properties.get(key).cloned();
                    let type_matches = value.as_ref().map(|value| {
                        property_matches_type(value, &field, &content.analysis.catalog)
                    });
                    let state = match value.as_ref() {
                        None => ProjectTemplateValueState::Missing,
                        Some(PropertyValue::Str(text)) if text.trim().is_empty() => {
                            ProjectTemplateValueState::Empty
                        }
                        Some(value)
                            if field
                                .default
                                .as_ref()
                                .is_some_and(|default| property_matches_json(value, default)) =>
                        {
                            ProjectTemplateValueState::Default
                        }
                        Some(_) if type_matches == Some(true) => ProjectTemplateValueState::Set,
                        Some(_) => ProjectTemplateValueState::TypeMismatch,
                    };
                    if check_integrity && state == ProjectTemplateValueState::TypeMismatch {
                        let object = content.analysis.catalog.object(&target);
                        diagnostics.push(Diagnostic::warning(
                            "TPL006",
                            object.map_or("", |object| object.file.as_str()),
                            Span::new(object.map_or(1, |object| object.line), 1, 1),
                            format!(
                                "{} `{}` 的字段 `{key}` 与{}模板类型不匹配；预览不会转换实例值",
                                target.kind,
                                target.id,
                                if *template_state == "current" {
                                    "当前"
                                } else {
                                    "新"
                                }
                            ),
                        ));
                    }
                    fields.push(ProjectTemplateFieldImpact {
                        field_id: field.id.clone(),
                        template_state: (*template_state).into(),
                        key: key.clone(),
                        state,
                        type_matches,
                        value,
                    });
                }
            }
            ProjectTemplateInstanceImpact { target, fields }
        })
        .collect()
}

fn property_matches_type(
    value: &PropertyValue,
    field: &ProjectTemplateField,
    catalog: &crate::catalog::Catalog,
) -> bool {
    match field.field_type.as_str() {
        "text" => matches!(value, PropertyValue::Str(_)),
        "number" => matches!(value, PropertyValue::Num(number) if number.is_finite()),
        "boolean" => matches!(value, PropertyValue::Bool(_)),
        "enum" => matches!(value, PropertyValue::Str(text) if field.choices.contains(text)),
        "object_ref" => {
            matches!(value, PropertyValue::Ref(reference) if field.target.as_ref().is_some_and(|target| target.kind == reference.kind)
            && field.target_entity_type.as_ref().is_none_or(|expected| catalog.entities.get(&reference.id).is_some_and(|entity| &entity.entity_type == expected)))
        }
        _ => true,
    }
}

fn property_matches_json(value: &PropertyValue, expected: &Value) -> bool {
    match (value, expected) {
        (PropertyValue::Str(actual), Value::String(expected)) => actual == expected,
        (PropertyValue::Num(actual), Value::Number(expected)) => expected.as_f64() == Some(*actual),
        (PropertyValue::Bool(actual), Value::Bool(expected)) => actual == expected,
        (PropertyValue::Ref(actual), Value::Object(expected)) => {
            expected.get("kind").and_then(Value::as_str) == Some(actual.kind.as_str())
                && expected.get("id").and_then(Value::as_str) == Some(actual.id.as_str())
        }
        _ => false,
    }
}

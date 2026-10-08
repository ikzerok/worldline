use super::*;
mod commands;
mod instances;
mod limits;
mod properties;
use properties::BorrowedProperties;

struct IndexedField<'a> {
    field: &'a ProjectTemplateField,
    parent_id: Option<&'a str>,
    index: usize,
}
type FieldIndex<'a> = BTreeMap<&'a str, IndexedField<'a>>;

fn flatten_fields(template: &ProjectTemplate) -> FieldIndex<'_> {
    fn add<'a>(
        fields: &'a [ProjectTemplateField],
        parent: Option<&'a str>,
        index: &mut FieldIndex<'a>,
    ) {
        for (position, field) in fields.iter().enumerate() {
            index.insert(
                &field.id,
                IndexedField {
                    field,
                    parent_id: parent,
                    index: position,
                },
            );
            add(&field.fields, Some(&field.id), index);
        }
    }
    let mut index = BTreeMap::new();
    add(&template.fields, None, &mut index);
    index
}

fn field_changes(
    old_fields: &FieldIndex<'_>,
    new_fields: &FieldIndex<'_>,
    budget: &mut limits::ImpactBudget,
) -> Result<Vec<ProjectTemplateFieldChange>, String> {
    let ids: BTreeSet<_> = old_fields
        .keys()
        .chain(new_fields.keys())
        .copied()
        .collect();
    let mut changes = Vec::new();
    for id in ids {
        let old_position = old_fields.get(id);
        let new_position = new_fields.get(id);
        let old = old_position.map(|position| position.field);
        let new = new_position.map(|position| position.field);
        let old_properties = old.map(BorrowedProperties::new).transpose()?;
        let new_properties = new.map(BorrowedProperties::new).transpose()?;
        let record = |change| BorrowedChange {
            field_id: id,
            change,
            old_key: old.and_then(|field| field.key.as_deref()),
            new_key: new.and_then(|field| field.key.as_deref()),
            old_type: old.map(|field| field.field_type.as_str()),
            new_type: new.map(|field| field.field_type.as_str()),
            old_parent_id: old_position.and_then(|position| position.parent_id),
            new_parent_id: new_position.and_then(|position| position.parent_id),
            old_index: old_position.map(|position| position.index),
            new_index: new_position.map(|position| position.index),
            old_properties,
            new_properties,
        };
        let attribute_change = match (old, new) {
            (None, Some(_)) => Some("added"),
            (Some(_), None) => Some("removed"),
            (Some(old), Some(new)) if old.key != new.key => Some("renamed"),
            (Some(old), Some(new)) if old.field_type != new.field_type => Some("type_changed"),
            (Some(old), Some(new)) if old.label != new.label => Some("label_changed"),
            (Some(old), Some(new))
                if old.required != new.required
                    || old.default != new.default
                    || old.choices != new.choices
                    || old.target != new.target
                    || old.target_entity_type != new.target_entity_type =>
            {
                Some("constraints_changed")
            }
            _ => None,
        };
        let position_change = matches!((old_position, new_position), (Some(old), Some(new))
            if old.parent_id != new.parent_id || old.index != new.index)
        .then_some("position_changed");
        for change in [attribute_change, position_change].into_iter().flatten() {
            let borrowed = record(change);
            budget.comma(!changes.is_empty())?;
            budget.json(&borrowed)?;
            changes.push(borrowed.into_owned());
        }
    }
    Ok(changes)
}

#[derive(Serialize)]
struct BorrowedChange<'a> {
    field_id: &'a str,
    change: &'static str,
    old_key: Option<&'a str>,
    new_key: Option<&'a str>,
    old_type: Option<&'a str>,
    new_type: Option<&'a str>,
    old_parent_id: Option<&'a str>,
    new_parent_id: Option<&'a str>,
    old_index: Option<usize>,
    new_index: Option<usize>,
    old_properties: Option<BorrowedProperties<'a>>,
    new_properties: Option<BorrowedProperties<'a>>,
}

impl BorrowedChange<'_> {
    fn into_owned(self) -> ProjectTemplateFieldChange {
        ProjectTemplateFieldChange {
            field_id: self.field_id.into(),
            change: self.change.into(),
            old_key: self.old_key.map(str::to_owned),
            new_key: self.new_key.map(str::to_owned),
            old_type: self.old_type.map(str::to_owned),
            new_type: self.new_type.map(str::to_owned),
            old_parent_id: self.old_parent_id.map(str::to_owned),
            new_parent_id: self.new_parent_id.map(str::to_owned),
            old_index: self.old_index,
            new_index: self.new_index,
            old_properties: self.old_properties.map(BorrowedProperties::into_owned),
            new_properties: self.new_properties.map(BorrowedProperties::into_owned),
        }
    }
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

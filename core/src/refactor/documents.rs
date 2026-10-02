use crate::catalog::TargetRef;
use serde_json::Value;

pub(crate) fn rewrite_registered(
    value: &mut Value,
    registry: &crate::workspace_documents::Registry,
    path: &std::path::Path,
    target: &TargetRef,
    new_id: &str,
) -> usize {
    let has = |paths: &std::collections::BTreeMap<String, std::path::PathBuf>| {
        paths.values().any(|p| p == path)
    };
    if has(&registry.manuscripts) {
        return rewrite_manuscript_json(value, target, new_id);
    }
    if has(&registry.templates) {
        return rewrite_template_json(value, target, new_id);
    }
    let mut count = 0;
    if has(&registry.maps) {
        if let Some(placements) = value.get_mut("placements").and_then(Value::as_object_mut) {
            for placement in placements.values_mut() {
                count += reference(placement.get_mut("target_ref"), target, new_id);
                count += references(placement.get_mut("scope_refs"), target, new_id);
            }
        }
        if let Some(nodes) = value
            .get_mut("scene")
            .and_then(|v| v.get_mut("nodes"))
            .and_then(Value::as_object_mut)
        {
            for node in nodes.values_mut() {
                count += reference(node.get_mut("target_ref"), target, new_id);
                count += references(node.get_mut("scope_refs"), target, new_id);
            }
        }
    } else if has(&registry.reader_profiles) {
        if let Some(selection) = value.get_mut("selection") {
            count += references(selection.get_mut("objects"), target, new_id);
            if let Some(fields) = selection.get_mut("fields").and_then(Value::as_array_mut) {
                for field in fields {
                    count += reference(field.get_mut("target"), target, new_id);
                }
            }
        }
        if let Some(routes) = value.get_mut("routes").and_then(Value::as_array_mut) {
            for route in routes {
                count += reference(route.get_mut("target"), target, new_id);
            }
        }
    } else if has(&registry.graph_views) {
        count += reference(value.get_mut("focus"), target, new_id);
        if let Some(positions) = value.get_mut("positions").and_then(Value::as_object_mut) {
            if let Some(position) = positions.remove(&format!("{}:{}", target.kind, target.id)) {
                positions.insert(format!("{}:{new_id}", target.kind), position);
                count += 1;
            }
        }
        if target.kind == "relation" {
            count += ids(value.get_mut("hidden_relation_ids"), &target.id, new_id);
        }
    } else if has(&registry.comments) {
        if let Some(anchor) = value
            .get_mut("anchor")
            .filter(|v| v.get("kind").and_then(Value::as_str) == Some("object"))
        {
            count += reference(anchor.get_mut("target"), target, new_id);
        }
    } else if has(&registry.presets) {
        count += references(value.get_mut("scope_refs"), target, new_id);
    } else if has(&registry.saved_queries) {
        if let Some(filters) = value
            .get_mut("query")
            .and_then(|v| v.get_mut("filters"))
            .and_then(Value::as_array_mut)
        {
            for filter in filters {
                match filter.get("dimension").and_then(Value::as_str) {
                    Some("tag") if target.kind == "tag" => {
                        count += ids(filter.get_mut("values"), &target.id, new_id)
                    }
                    Some("relation") => {
                        if let Some(values) = filter.get_mut("values").and_then(Value::as_array_mut)
                        {
                            for condition in values {
                                count += reference(condition.get_mut("related"), target, new_id);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    count
}
fn reference(value: Option<&mut Value>, target: &TargetRef, new_id: &str) -> usize {
    let Some(object) = value.and_then(Value::as_object_mut) else {
        return 0;
    };
    if object.get("kind").and_then(Value::as_str) == Some(target.kind.as_str())
        && object.get("id").and_then(Value::as_str) == Some(target.id.as_str())
    {
        object.insert("id".into(), Value::String(new_id.into()));
        1
    } else {
        0
    }
}
fn references(value: Option<&mut Value>, target: &TargetRef, new_id: &str) -> usize {
    value
        .and_then(Value::as_array_mut)
        .map(|items| {
            items
                .iter_mut()
                .map(|v| reference(Some(v), target, new_id))
                .sum()
        })
        .unwrap_or(0)
}
fn ids(value: Option<&mut Value>, old: &str, new: &str) -> usize {
    let Some(items) = value.and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut count = 0;
    for item in items {
        if item.as_str() == Some(old) {
            *item = Value::String(new.into());
            count += 1;
        }
    }
    count
}

pub(crate) fn rewrite_manuscript_json(
    value: &mut Value,
    target: &TargetRef,
    new_id: &str,
) -> usize {
    let Some(entries) = value
        .as_object_mut()
        .and_then(|object| object.get_mut("entries"))
        .and_then(Value::as_array_mut)
    else {
        return 0;
    };
    let mut count = 0;
    for entry in entries.iter_mut() {
        count += reference(entry.get_mut("target_ref"), target, new_id);
        count += reference(entry.get_mut("perspective"), target, new_id);
    }
    count
}

pub(crate) fn rewrite_template_json(value: &mut Value, target: &TargetRef, new_id: &str) -> usize {
    let Some(fields) = value
        .as_object_mut()
        .and_then(|object| object.get_mut("fields"))
        .and_then(Value::as_array_mut)
    else {
        return 0;
    };
    fields
        .iter_mut()
        .map(|field| rewrite_template_field(field, target, new_id))
        .sum()
}

fn rewrite_template_field(field: &mut Value, target: &TargetRef, new_id: &str) -> usize {
    let Some(object) = field.as_object_mut() else {
        return 0;
    };
    let mut count = 0;
    if object.get("type").and_then(Value::as_str) == Some("object_ref") {
        if let Some(default) = object.get_mut("default").and_then(Value::as_object_mut) {
            if default.get("kind").and_then(Value::as_str) == Some(target.kind.as_str())
                && default.get("id").and_then(Value::as_str) == Some(target.id.as_str())
            {
                default.insert("id".into(), Value::String(new_id.into()));
                count += 1;
            }
        }
    }
    if let Some(children) = object.get_mut("fields").and_then(Value::as_array_mut) {
        count += children
            .iter_mut()
            .map(|child| rewrite_template_field(child, target, new_id))
            .sum::<usize>();
    }
    count
}

use crate::ReplayObservation;

pub(super) fn observations_match(expected: &ReplayObservation, actual: &ReplayObservation) -> bool {
    outputs_match(&expected.outputs, &actual.outputs)
        && presentation_semantics(&expected.choice_presentation)
            == presentation_semantics(&actual.choice_presentation)
        && expected
            .choices
            .iter()
            .map(|choice| (&choice.id, &choice.label))
            .eq(actual
                .choices
                .iter()
                .map(|choice| (&choice.id, &choice.label)))
        && semantic_state(&expected.state) == semantic_state(&actual.state)
}

fn presentation_semantics(presentation: &[serde_json::Value]) -> Vec<serde_json::Value> {
    presentation
        .iter()
        .map(|item| {
            let mut item = item.clone();
            if let Some(object) = item.as_object_mut() {
                object.remove("line");
            }
            strip_localization_locations(&mut item);
            item
        })
        .collect()
}

pub(super) fn semantic_state(value: &serde_json::Value) -> serde_json::Value {
    let mut value = value.clone();
    if let Some(calls) = value
        .get_mut("calls")
        .and_then(serde_json::Value::as_array_mut)
    {
        for call in calls {
            if let Some(call) = call.as_object_mut() {
                // Only these frame-level fields are source metadata. Keep all
                // semantic/unknown fields, including locals named file or line.
                call.remove("file");
                call.remove("line");
            }
        }
    }
    if let Some(choices) = value
        .get_mut("coverage")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|coverage| coverage.get_mut("selected_choices"))
        .and_then(serde_json::Value::as_array_mut)
    {
        for choice in choices {
            if let Some(choice) = choice.as_object_mut() {
                choice.remove("line");
            }
        }
    }
    value
}

fn outputs_match(expected: &[serde_json::Value], actual: &[serde_json::Value]) -> bool {
    expected.len() == actual.len()
        && expected.iter().zip(actual).all(|(left, right)| {
            if left == right {
                return true;
            }
            if left.get("localization").is_none() && right.get("localization").is_none() {
                return false;
            }
            let mut left = left.clone();
            let mut right = right.clone();
            strip_localization_locations(&mut left);
            strip_localization_locations(&mut right);
            left == right
        })
}

fn strip_localization_locations(value: &mut serde_json::Value) {
    if let Some(metadata) = value
        .get_mut("localization")
        .and_then(serde_json::Value::as_object_mut)
    {
        if let Some(source) = metadata
            .get_mut("source")
            .and_then(serde_json::Value::as_object_mut)
        {
            source.remove("file");
            source.remove("line");
        }
        for field in ["source_baseline", "sidecar_path", "translation_pointer"] {
            metadata.remove(field);
        }
    }
}

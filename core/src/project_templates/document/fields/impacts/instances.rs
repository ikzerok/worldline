use super::*;
use limits::{check_counts, limit_error, ImpactBudget};

type Properties = BTreeMap<String, PropertyValue>;

struct FlatTemplate<'a> {
    fields: Vec<&'a ProjectTemplateField>,
}
struct ObjectImpact<'a> {
    properties: &'a Properties,
    current_applicable: bool,
    proposed_applicable: bool,
}
pub(super) struct PreparedImpacts<'a> {
    current: Option<FlatTemplate<'a>>,
    proposed: Option<FlatTemplate<'a>>,
    objects: BTreeMap<&'a TargetRef, ObjectImpact<'a>>,
}

pub(super) fn prepare<'a>(
    old: Option<&'a ProjectTemplate>,
    new: Option<&'a ProjectTemplate>,
    old_fields: &FieldIndex<'a>,
    new_fields: &FieldIndex<'a>,
    content: &'a CompileResult,
    impact_limits: Option<&TemplateImpactLimits>,
) -> Result<PreparedImpacts<'a>, String> {
    let flatten = |fields: &FieldIndex<'a>| FlatTemplate {
        fields: fields
            .values()
            .map(|indexed| indexed.field)
            .filter(|field| field.key.is_some())
            .collect(),
    };
    let current = old.map(|_| flatten(old_fields));
    let proposed = new.map(|_| flatten(new_fields));
    let current_fields = current.as_ref().map_or(0, |template| template.fields.len());
    let proposed_fields = proposed
        .as_ref()
        .map_or(0, |template| template.fields.len());
    let mut objects: BTreeMap<&'a TargetRef, ObjectImpact<'a>> = BTreeMap::new();
    let mut fields_count = 0usize;
    for object in &content.analysis.catalog.objects {
        let target = &object.target;
        if objects.contains_key(target) {
            continue;
        }
        let current_applicable =
            old.is_some_and(|template| template_applies_to(template, target, content));
        let proposed_applicable =
            new.is_some_and(|template| template_applies_to(template, target, content));
        if !current_applicable && !proposed_applicable {
            continue;
        }
        let Some(properties) = object_properties(target, content) else {
            continue;
        };
        for count in [
            if current_applicable {
                current_fields
            } else {
                0
            },
            if proposed_applicable {
                proposed_fields
            } else {
                0
            },
        ] {
            fields_count = fields_count
                .checked_add(count)
                .ok_or_else(|| limit_error("字段值数量计数溢出"))?;
        }
        let instances_count = objects
            .len()
            .checked_add(1)
            .ok_or_else(|| limit_error("实例数量计数溢出"))?;
        check_counts(impact_limits, instances_count, fields_count)?;
        objects.insert(
            target,
            ObjectImpact {
                properties,
                current_applicable,
                proposed_applicable,
            },
        );
    }
    Ok(PreparedImpacts {
        current,
        proposed,
        objects,
    })
}

impl PreparedImpacts<'_> {
    pub(super) fn build(
        self,
        content: &CompileResult,
        check_integrity: bool,
        diagnostics: &mut Vec<Diagnostic>,
        budget: &mut ImpactBudget,
    ) -> Result<Vec<ProjectTemplateInstanceImpact>, String> {
        let mut result = Vec::new();
        for (target, object) in self.objects {
            #[derive(Serialize)]
            struct EmptyRow<'a> {
                target: &'a TargetRef,
                current_applicable: bool,
                proposed_applicable: bool,
                fields: &'a [ProjectTemplateFieldImpact],
            }
            budget.comma(!result.is_empty())?;
            budget.json(&EmptyRow {
                target,
                current_applicable: object.current_applicable,
                proposed_applicable: object.proposed_applicable,
                fields: &[],
            })?;
            let mut fields = Vec::new();
            for (template, applicable, state) in [
                (self.current.as_ref(), object.current_applicable, "current"),
                (
                    self.proposed.as_ref(),
                    object.proposed_applicable,
                    "proposed",
                ),
            ] {
                let Some(template) = template.filter(|_| applicable) else {
                    continue;
                };
                for &field in &template.fields {
                    let key = field.key.as_deref().expect("已筛选实例字段");
                    let value = object.properties.get(key);
                    let type_matches = value.map(|value| {
                        property_matches_type(value, field, &content.analysis.catalog)
                    });
                    let field_state = value_state(field, value, type_matches);
                    let borrowed = BorrowedField {
                        field_id: &field.id,
                        template_state: state,
                        key,
                        state: field_state,
                        value,
                        type_matches,
                    };
                    budget.comma(!fields.is_empty())?;
                    budget.json(&borrowed)?;
                    if check_integrity && field_state == ProjectTemplateValueState::TypeMismatch {
                        add_warning(target, key, state, content, diagnostics, budget)?;
                    }
                    fields.push(ProjectTemplateFieldImpact {
                        field_id: field.id.clone(),
                        template_state: state.into(),
                        key: key.into(),
                        state: field_state,
                        value: value.cloned(),
                        type_matches,
                    });
                }
            }
            result.push(ProjectTemplateInstanceImpact {
                target: target.clone(),
                current_applicable: object.current_applicable,
                proposed_applicable: object.proposed_applicable,
                fields,
            });
        }
        Ok(result)
    }
}

#[derive(Serialize)]
struct BorrowedField<'a> {
    field_id: &'a str,
    template_state: &'static str,
    key: &'a str,
    state: ProjectTemplateValueState,
    value: Option<&'a PropertyValue>,
    type_matches: Option<bool>,
}

fn value_state(
    field: &ProjectTemplateField,
    value: Option<&PropertyValue>,
    type_matches: Option<bool>,
) -> ProjectTemplateValueState {
    match value {
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
    }
}

fn add_warning(
    target: &TargetRef,
    key: &str,
    state: &str,
    content: &CompileResult,
    diagnostics: &mut Vec<Diagnostic>,
    budget: &mut ImpactBudget,
) -> Result<(), String> {
    let object = content.analysis.catalog.object(target);
    let file = object.map_or("", |object| object.file.as_str());
    // 在格式化包含任意长 key/ID 的诊断之前先检查不可避免的原始字节下界。
    let minimum = [target.kind.len(), target.id.len(), key.len(), file.len()]
        .into_iter()
        .try_fold(0usize, |sum, size| sum.checked_add(size))
        .ok_or_else(|| limit_error("诊断字节计数溢出"))?;
    budget.ensure_additional(minimum)?;
    let diagnostic = Diagnostic::warning(
        "TPL006",
        file,
        Span::new(object.map_or(1, |object| object.line), 1, 1),
        format!(
            "{} `{}` 的字段 `{key}` 与{}模板类型不匹配；预览不会转换实例值",
            target.kind,
            target.id,
            if state == "current" { "当前" } else { "新" }
        ),
    );
    budget.comma(!diagnostics.is_empty())?;
    budget.json(&diagnostic)?;
    diagnostics.push(diagnostic);
    Ok(())
}

fn template_applies_to(
    template: &ProjectTemplate,
    target: &TargetRef,
    content: &CompileResult,
) -> bool {
    template.applies_to.kind == target.kind
        && template
            .applies_to_entity_type
            .as_ref()
            .is_none_or(|expected| {
                content
                    .analysis
                    .catalog
                    .entities
                    .get(&target.id)
                    .is_some_and(|entity| &entity.entity_type == expected)
            })
}

fn object_properties<'a>(target: &TargetRef, content: &'a CompileResult) -> Option<&'a Properties> {
    match target.kind.as_str() {
        "entity" => content
            .analysis
            .catalog
            .entities
            .get(&target.id)
            .map(|entity| &entity.properties),
        "character" => content
            .analysis
            .symbols
            .characters
            .get(&target.id)
            .map(|character| &character.properties),
        "world" => content
            .analysis
            .world
            .as_ref()
            .map(|world| &world.properties),
        _ => None,
    }
}

//! 内置作者模板目录；规范单源为 spec/examples/templates.catalog.json。
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::OnceLock;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TemplateCatalog {
    pub schema_version: u32,
    pub templates: Vec<ContentTemplate>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContentTemplate {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub applies_to: TemplateTarget,
    pub fields: Vec<TemplateField>,
    pub prompts: Vec<String>,
    pub suggested_relations: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TemplateTarget {
    pub kind: String,
    #[serde(default)]
    pub entity_type: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TemplateField {
    pub key: String,
    pub label: String,
    pub widget: String,
    pub required: bool,
}

const BUILTIN_JSON: &str = include_str!("../../spec/examples/templates.catalog.json");
static BUILTIN: OnceLock<TemplateCatalog> = OnceLock::new();

pub fn builtin_templates() -> &'static TemplateCatalog {
    BUILTIN.get_or_init(|| {
        let catalog: TemplateCatalog =
            serde_json::from_str(BUILTIN_JSON).expect("内置模板目录必须符合规范样例");
        validate_catalog(&catalog).expect("内置模板目录必须通过核心校验");
        catalog
    })
}

pub fn matching_template(
    kind: &str,
    entity_type: Option<&str>,
) -> Option<&'static ContentTemplate> {
    builtin_templates().templates.iter().find(|template| {
        template.applies_to.kind == kind
            && template
                .applies_to
                .entity_type
                .as_deref()
                .is_none_or(|expected| Some(expected) == entity_type)
    })
}
pub fn validate_catalog(catalog: &TemplateCatalog) -> Result<(), String> {
    if catalog.schema_version != 1 {
        return Err("模板目录 schema_version 必须为 1".into());
    }
    if catalog.templates.is_empty() {
        return Err("模板目录不能为空".into());
    }
    let mut ids = BTreeSet::new();
    for template in &catalog.templates {
        if template.schema_version != 1
            || !valid_id(&template.id)
            || template.title.trim().is_empty()
            || !matches!(
                template.applies_to.kind.as_str(),
                "world" | "character" | "entity"
            )
        {
            return Err(format!("模板 {} 的身份或适用类型无效", template.id));
        }
        if template.applies_to.kind != "entity" && template.applies_to.entity_type.is_some() {
            return Err(format!(
                "模板 {} 只有 entity 可声明 entity_type",
                template.id
            ));
        }
        if !ids.insert(&template.id) {
            return Err(format!("模板 ID {} 重复", template.id));
        }
        let mut fields = BTreeSet::new();
        for field in &template.fields {
            if !valid_id(&field.key)
                || field.label.trim().is_empty()
                || !matches!(
                    field.widget.as_str(),
                    "multiline" | "text" | "number" | "boolean"
                )
                || !fields.insert(&field.key)
            {
                return Err(format!("模板 {} 包含无效或重复字段", template.id));
            }
        }
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_has_all_sixteen_valid_templates() {
        let catalog = builtin_templates();
        assert_eq!(catalog.templates.len(), 16);
        assert!(validate_catalog(catalog).is_ok());
        assert_eq!(
            matching_template("entity", Some("place")).map(|item| item.id.as_str()),
            Some("template_place")
        );
        assert_eq!(
            matching_template("character", None).map(|item| item.id.as_str()),
            Some("template_character")
        );
        assert_eq!(
            matching_template("world", None).map(|item| item.id.as_str()),
            Some("template_world_outline")
        );
        assert!(catalog.templates.iter().all(|template| {
            !template.prompts.is_empty()
                && !template.suggested_relations.is_empty()
                && template.fields.iter().all(|field| !field.required)
        }));
        let entity_templates = catalog
            .templates
            .iter()
            .filter(|template| template.applies_to.kind == "entity")
            .count();
        assert_eq!(entity_templates, 14);
    }
}

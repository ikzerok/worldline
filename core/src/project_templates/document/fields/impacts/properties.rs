use super::*;

/// 与公开 Properties 相同的浅快照线形状，计量前不复制 choices/default/长字符串。
#[derive(Clone, Copy, Serialize)]
pub(super) struct BorrowedProperties<'a> {
    label: &'a str,
    key: Option<&'a str>,
    field_type: ProjectTemplateFieldType,
    required: bool,
    choices: &'a [String],
    target: Option<BorrowedTarget<'a>>,
    default: Option<&'a Value>,
}

#[derive(Clone, Copy, Serialize)]
struct BorrowedTarget<'a> {
    kind: &'a str,
    entity_type: Option<&'a str>,
}

impl<'a> BorrowedProperties<'a> {
    pub(super) fn new(field: &'a ProjectTemplateField) -> Result<Self, String> {
        let field_type = match field.field_type.as_str() {
            "text" => ProjectTemplateFieldType::Text,
            "number" => ProjectTemplateFieldType::Number,
            "boolean" => ProjectTemplateFieldType::Boolean,
            "enum" => ProjectTemplateFieldType::Enum,
            "object_ref" => ProjectTemplateFieldType::ObjectRef,
            "group" => ProjectTemplateFieldType::Group,
            _ => return Err("TPL004：无法生成不受支持的字段类型属性快照".into()),
        };
        Ok(Self {
            label: &field.label,
            key: field.key.as_deref(),
            field_type,
            required: field.required,
            choices: &field.choices,
            target: field.target.as_ref().map(|target| BorrowedTarget {
                kind: &target.kind,
                entity_type: field.target_entity_type.as_deref(),
            }),
            default: field.default.as_ref(),
        })
    }

    pub(super) fn into_owned(self) -> ProjectTemplateFieldProperties {
        ProjectTemplateFieldProperties {
            label: self.label.into(),
            key: self.key.map(str::to_owned),
            field_type: self.field_type,
            required: self.required,
            choices: self.choices.to_vec(),
            target: self.target.map(|target| ProjectTemplateDraftTarget {
                kind: target.kind.into(),
                entity_type: target.entity_type.map(str::to_owned),
            }),
            default: self.default.cloned(),
        }
    }
}

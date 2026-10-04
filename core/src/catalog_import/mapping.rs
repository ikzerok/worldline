use super::*;
use std::collections::BTreeSet;

pub(super) struct Context {
    options: crate::CompileOptions,
    entities: bool,
}
impl Context {
    pub fn new(project: &crate::project::Project) -> Self {
        let entities = project.authoring_document(&crate::workspace_documents::manifest_path(&project.root)).ok().is_some_and(|doc| {
            crate::workspace_documents::parse_registry(&project.root, doc.bytes()).required_features.contains("content.entities.v1")
        });
        Self { options: project.compile_options(), entities }
    }
}
pub(super) struct MappedRow {
    pub target: TargetRef,
    pub values: Vec<(String, String, PropertyValue)>,
}
pub(super) fn validate(columns: &[CatalogColumnMapping], count: usize) -> Result<(), String> {
    let mut indices = BTreeSet::new();
    let mut fields = BTreeSet::new();
    for map in columns {
        if map.column >= count || !indices.insert(map.column) {
            return Err("映射列越界或重复；每列必须恰好映射一次".into());
        }
        if map.field == CatalogImportField::Ignore { continue; }
        let (key, text) = match &map.field {
            CatalogImportField::Kind => ("kind".into(), false),
            CatalogImportField::Id => ("id".into(), false),
            CatalogImportField::Display => ("display".into(), true),
            CatalogImportField::EntityType => ("entity_type".into(), false),
            CatalogImportField::Description => ("description".into(), true),
            CatalogImportField::Property { key, value_type } => {
                crate::authoring::identifier(key)?;
                if let CatalogImportType::Ref { target_kind } = value_type {
                    if !matches!(target_kind.as_str(), "entity" | "relation" | "character") {
                        return Err("ref映射只支持entity/relation/character目标kind".into());
                    }
                }
                (format!("property.{key}"), *value_type == CatalogImportType::Text)
            }
            CatalogImportField::Ignore => unreachable!(),
        };
        if !fields.insert(key) { return Err("多个列映射到同一字段".into()); }
        if map.blank == CatalogBlankPolicy::EmptyText && !text {
            return Err("empty_text仅支持显示名、描述或text属性".into());
        }
        if matches!(map.field, CatalogImportField::Kind | CatalogImportField::Id)
            && map.blank != CatalogBlankPolicy::Error {
            return Err("kind/id身份列必须使用error空值策略".into());
        }
    }
    if indices.len() != count { return Err("每一列都须显式映射或忽略，存在遗漏列".into()); }
    if !fields.contains("kind") || !fields.contains("id") {
        return Err("必须各映射一列kind和id".into());
    }
    Ok(())
}
/// 即使其它列有错误，已验证的身份仍可用于完整逐行预览与重复键检查。
pub(super) fn identity(record: &CatalogCsvRow, columns: &[CatalogColumnMapping]) -> Option<TargetRef> {
    let cell = |field| columns.iter().find(|map| map.field == field).and_then(|map| record.cells.get(map.column));
    let kind = cell(CatalogImportField::Kind)?;
    let id = cell(CatalogImportField::Id)?;
    if !matches!(kind.as_str(), "character" | "entity") || crate::authoring::identifier(id).is_err() { return None; }
    Some(TargetRef::new(kind,id))
}
pub(super) fn row(
    record: &CatalogCsvRow,
    row: usize,
    columns: &[CatalogColumnMapping],
    context: &Context,
) -> Result<MappedRow, Vec<CatalogImportDiagnostic>> {
    let get = |field: CatalogImportField| {
        let map = columns.iter().find(|map| map.field == field).expect("validated identity mapping");
        (&record.cells[map.column], map.column)
    };
    let (kind, kind_col) = get(CatalogImportField::Kind);
    let (id, id_col) = get(CatalogImportField::Id);
    let error = |code: &str, col: usize, message: String| CatalogImportDiagnostic::new(code, message).at(row, col+1, record.line);
    if !matches!(kind.as_str(), "character" | "entity") {
        return Err(vec![error("IMPORT_KIND", kind_col, "kind只能是character或entity".into())]);
    }
    crate::authoring::identifier(id).map_err(|message| vec![error("IMPORT_ID", id_col, message)])?;
    let target = TargetRef::new(kind, id);
    if kind == "entity" {
        if !context.options.language_version.supports_entities() || !context.entities {
            return Err(vec![error("IMPORT_CAPABILITY", kind_col, "entity需要显式语言1.10+与content.entities.v1；请先启用能力".into())]);
        }
    }
    let mut values = Vec::new();
    let mut errors = Vec::new();
    for map in columns {
        if matches!(map.field, CatalogImportField::Kind | CatalogImportField::Id | CatalogImportField::Ignore) { continue; }
        let parsed = (|| -> Result<Option<(String, String, PropertyValue)>, CatalogImportDiagnostic> {
        let cell = &record.cells[map.column];
        if cell.is_empty() {
            match map.blank {
                CatalogBlankPolicy::Keep => return Ok(None),
                CatalogBlankPolicy::Error => return Err(error("IMPORT_BLANK", map.column, "空单元格需要明确选择保留或空文本策略".into())),
                CatalogBlankPolicy::EmptyText => {}
            }
        }
        if cell.chars().any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t')) {
            return Err(error("IMPORT_TEXT", map.column, "资料含不支持的控制字符".into()));
        }
        let (field, value_type) = match &map.field {
            CatalogImportField::Display => ("display".into(), CatalogImportType::Text),
            CatalogImportField::EntityType | CatalogImportField::Description => {
                if kind != "entity" {
                    return Err(error("IMPORT_FIELD", map.column, "character不支持entity_type/description；空单元格可明确保留跳过".into()));
                }
                if map.field == CatalogImportField::EntityType {
                    crate::authoring::identifier(cell).map_err(|message| error("IMPORT_TYPE", map.column, message))?;
                    ("entity_type".into(), CatalogImportType::Text)
                } else { ("description".into(), CatalogImportType::Text) }
            }
            CatalogImportField::Property { key, value_type } => (format!("property.{key}"), value_type.clone()),
            _ => unreachable!(),
        };
        let value = parse_value(cell, &value_type).map_err(|message| error("IMPORT_VALUE", map.column, message))?;
        if let CatalogImportType::Ref { target_kind } = &value_type {
            let options = context.options;
            if !options.object_refs || !context.options.language_version.supports_entities()
                || ((kind == "character" || target_kind == "character")
                    && (!options.character_refs || !context.options.language_version.supports_language_113())) {
                return Err(error("IMPORT_CAPABILITY", map.column, "ref缺少对应语言或content.object_refs.v1/content.character_refs.v1能力；不会自动升级".into()));
            }
        }
        let label = match value_type {
            CatalogImportType::Text => "text".into(),
            CatalogImportType::Number => "number".into(),
            CatalogImportType::Bool => "bool".into(),
            CatalogImportType::Ref { target_kind } => format!("ref:{target_kind}"),
        };
        Ok(Some((field, label, value)))
        })();
        match parsed { Ok(Some(value)) => values.push(value), Ok(None) => {}, Err(error) => errors.push(error) }
    }
    if errors.is_empty() { Ok(MappedRow { target, values }) } else { Err(errors) }
}
fn parse_value(cell: &str, kind: &CatalogImportType) -> Result<PropertyValue, String> {
    Ok(match kind {
        CatalogImportType::Text => PropertyValue::Str(cell.into()),
        CatalogImportType::Number => {
            if cell.trim() != cell { return Err("数值不能含首尾空白".into()); }
            let value: serde_json::Number = serde_json::from_str(cell).map_err(|_| "数值须为有限JSON十进制数字")?;
            let value = value.as_f64().filter(|value| value.is_finite()).ok_or("数值必须有限")?;
            if value.fract() == 0.0 && value.abs() > 9_007_199_254_740_991.0 {
                return Err("整数超过可精确表示范围±9007199254740991".into());
            }
            PropertyValue::Num(value)
        }
        CatalogImportType::Bool => match cell {
            "true" => PropertyValue::Bool(true),
            "false" => PropertyValue::Bool(false),
            _ => return Err("bool只接受true或false".into()),
        },
        CatalogImportType::Ref { target_kind } => {
            crate::authoring::identifier(cell)?;
            PropertyValue::Ref(TargetRef::new(target_kind, cell))
        }
    })
}

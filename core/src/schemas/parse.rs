use super::{SchemaBinding, SchemaDecl, SchemaField, SchemaType};
use crate::{ast::Loc, catalog::TargetRef, Diagnostic, Span};

pub(crate) const INSTANCE_KINDS: &[&str] = &["world", "character", "entity", "relation"];
type Token = (String, bool);

fn word(tokens: &[Token], index: usize) -> &str {
    tokens
        .get(index)
        .filter(|t| !t.1)
        .map_or("", |t| t.0.as_str())
}
fn id(tokens: &[Token], index: usize) -> bool {
    let token = word(tokens, index);
    token != "END" && crate::lexer::valid_identifier(token)
}
pub(crate) fn error(file: &str, loc: Loc, message: &str, diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.push(Diagnostic::error(
        "SCH001",
        file,
        Span::new(loc.line, loc.column, 1),
        message,
    ));
}

pub(crate) fn declaration(
    source: &str,
    file: &str,
    loc: Loc,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<SchemaDecl> {
    let tokens = crate::catalog_syntax::tokenize(source, file, loc.line, diagnostics);
    let mut at = 3;
    let kind = word(&tokens, 2);
    let mut entity_type = None;
    if word(&tokens, at) == "entity_type" {
        if kind != "entity" || !id(&tokens, at + 1) {
            error(
                file,
                loc,
                "schema entity_type 仅适用于 entity，且必须跟子类标识符",
                diagnostics,
            );
            return None;
        }
        entity_type = Some(word(&tokens, at + 1).into());
        at += 2;
    }
    let closed = word(&tokens, at) == "closed";
    at += usize::from(closed);
    if !id(&tokens, 0)
        || word(&tokens, 1) != "for"
        || !INSTANCE_KINDS.contains(&kind)
        || at != tokens.len()
    {
        error(
            file,
            loc,
            "格式为 schema ID for KIND [entity_type SUBTYPE] [closed]",
            diagnostics,
        );
        return None;
    }
    Some(SchemaDecl {
        id: word(&tokens, 0).into(),
        kind: kind.into(),
        entity_type,
        closed,
        fields: Vec::new(),
        file: file.into(),
        loc,
    })
}

pub(crate) fn binding(
    source: &str,
    file: &str,
    loc: Loc,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<SchemaBinding> {
    let tokens = crate::catalog_syntax::tokenize(source, file, loc.line, diagnostics);
    if tokens.len() != 4
        || !INSTANCE_KINDS.contains(&word(&tokens, 0))
        || !id(&tokens, 1)
        || word(&tokens, 2) != "to"
        || !id(&tokens, 3)
    {
        error(
            file,
            loc,
            "格式为 bind KIND ID to SCHEMA_ID；KIND 仅支持 world/character/entity/relation",
            diagnostics,
        );
        return None;
    }
    Some(SchemaBinding {
        target: TargetRef::new(word(&tokens, 0), word(&tokens, 1)),
        schema_id: word(&tokens, 3).into(),
        file: file.into(),
        loc,
    })
}

pub(crate) fn field(
    source: &str,
    file: &str,
    loc: Loc,
    options: crate::CompileOptions,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<SchemaField> {
    let mut tokens = crate::catalog_syntax::tokenize(source, file, loc.line, diagnostics);
    let required = tokens.last().is_some_and(|t| !t.1 && t.0 == "required");
    if required {
        tokens.pop();
    }
    if !id(&tokens, 0) || !id(&tokens, 1) {
        error(
            file,
            loc,
            "field 必须含稳定字段 ID、property 键和类型",
            diagnostics,
        );
        return None;
    }
    let value_type = match word(&tokens, 2) {
        "text" if tokens.len() == 3 => SchemaType::Text,
        "number" if tokens.len() == 3 => SchemaType::Number,
        "boolean" if tokens.len() == 3 => SchemaType::Boolean,
        "enum" if tokens.len() >= 4 && tokens[3..].iter().all(|t| t.1) => {
            let values: Vec<_> = tokens[3..].iter().map(|t| t.0.clone()).collect();
            if values
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != values.len()
            {
                error(file, loc, "enum 不允许重复值", diagnostics);
                return None;
            }
            SchemaType::Enum { values }
        }
        "ref" if crate::catalog::OBJECT_REFERENCE_TARGET_KINDS.contains(&word(&tokens, 3)) => {
            let target_kind = word(&tokens, 3).to_owned();
            if target_kind == "character"
                && !crate::catalog::is_object_reference_kind(&target_kind, options)
            {
                error(file, loc, "ref character 需要显式语言 1.13 与 content.object_refs.v1、content.character_refs.v1 双能力", diagnostics);
                return None;
            }
            let entity_type = if tokens.len() == 6
                && target_kind == "entity"
                && word(&tokens, 4) == "entity_type"
                && id(&tokens, 5)
            {
                Some(word(&tokens, 5).into())
            } else if tokens.len() == 4 {
                None
            } else {
                error(
                    file,
                    loc,
                    "ref 类型为 ref entity [entity_type SUBTYPE] 或 ref relation/character（人物需显式能力）",
                    diagnostics,
                );
                return None;
            };
            SchemaType::Ref {
                target_kind,
                entity_type,
            }
        }
        _ => {
            error(file, loc, "字段类型应为 text/number/boolean/enum \"值\"…/ref entity|relation|character，尾部可写 required", diagnostics);
            return None;
        }
    };
    Some(SchemaField {
        id: word(&tokens, 0).into(),
        key: word(&tokens, 1).into(),
        value_type,
        required,
        loc,
    })
}

//! 正式lexer分类/token范围驱动的字段补丁，未触及字节完整复制。
use super::*;
use crate::lexer::{Line, LineKind};
use crate::refactor::preview::Edit;
use std::collections::BTreeMap;
use std::ops::Range;

pub(super) struct PreparedRow {
    pub path: PathBuf,
    pub edits: Vec<Edit>,
    pub append: String,
    pub fields: Vec<CatalogImportFieldChange>,
    pub operation: String,
}
pub(super) struct SourceIndex<'a> {
    raw_lines: Vec<(usize, &'a str)>,
    clean_lines: Vec<String>,
    lines: Vec<Line>,
}

struct Existing {
    path: PathBuf,
    line: u32,
    values: BTreeMap<String, PropertyValue>,
}
fn existing(content: &crate::CompileResult, target: &TargetRef) -> Option<Existing> {
    let mut values = BTreeMap::new();
    let (path, line, display, properties) = if target.kind == "character" {
        let decl = content
            .program
            .characters
            .iter()
            .find(|decl| decl.name == target.id)?;
        (&decl.file, decl.loc.line, &decl.display, &decl.properties)
    } else {
        let decl = content
            .program
            .entities
            .iter()
            .find(|decl| decl.name == target.id)?;
        values.insert(
            "entity_type".into(),
            PropertyValue::Str(decl.entity_type.clone()),
        );
        values.insert(
            "description".into(),
            PropertyValue::Str(decl.description.clone()),
        );
        (&decl.file, decl.loc.line, &decl.display, &decl.properties)
    };
    values.insert(
        "display".into(),
        PropertyValue::Str(display.clone().unwrap_or_else(|| target.id.clone())),
    );
    for property in properties {
        values.insert(
            format!("property.{}", property.name),
            property.value.clone(),
        );
    }
    Some(Existing {
        path: path.into(),
        line,
        values,
    })
}
pub(super) fn prepare<'a>(
    project: &'a crate::project::Project,
    content: &crate::CompileResult,
    destination: &std::path::Path,
    row: &mapping::MappedRow,
    cache: &mut BTreeMap<PathBuf, SourceIndex<'a>>,
) -> Result<PreparedRow, String> {
    let old = existing(content, &row.target);
    let fields: Vec<_> = row
        .values
        .iter()
        .map(|(field, value_type, value)| CatalogImportFieldChange {
            field: field.clone(),
            value_type: value_type.clone(),
            before: old.as_ref().and_then(|old| old.values.get(field).cloned()),
            after: Some(value.clone()),
        })
        .collect();
    let Some(old) = old else {
        let required = |name: &str| {
            row.values
                .iter()
                .find_map(|(field, _, value)| {
                    if field == name {
                        if let PropertyValue::Str(text) = value {
                            return Some(text.as_str());
                        }
                    }
                    None
                })
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("新对象缺少非空{name}；须明确映射并填写"))
        };
        let display = required("display")?;
        let source = project.document(destination)?;
        let eol = newline(source);
        let mut append = if row.target.kind == "entity" {
            format!(
                "entity {} kind {} as {}{eol}",
                row.target.id,
                required("entity_type")?,
                crate::authoring::quote(display)
            )
        } else {
            format!(
                "character {} as {}{eol}",
                row.target.id,
                crate::authoring::quote(display)
            )
        };
        for (field, _, value) in &row.values {
            if let Some(key) = field.strip_prefix("property.") {
                append.push_str(&format!(
                    "  property {key} = {}{eol}",
                    crate::authoring::property_source(value)
                ));
            } else if field == "description" {
                append.push_str(&format!(
                    "  description {}{eol}",
                    crate::authoring::property_source(value)
                ));
            }
        }
        return Ok(PreparedRow {
            path: destination.into(),
            edits: Vec::new(),
            append,
            fields,
            operation: "create".into(),
        });
    };
    let source = project.document(&old.path)?;
    let syntax = cache
        .entry(old.path.clone())
        .or_insert_with(|| SourceIndex {
            raw_lines: physical_lines(source),
            clean_lines: crate::lexer::strip_comments(source)
                .split_inclusive('\n')
                .map(str::to_owned)
                .collect(),
            lines: crate::lexer::lex_source_with_options(
                &old.path.to_string_lossy(),
                source,
                &mut Vec::new(),
                project.compile_options(),
            ),
        });
    let raw_lines = &syntax.raw_lines;
    let clean_lines = &syntax.clean_lines;
    let lines = &syntax.lines;
    let index = lines
        .iter()
        .position(|line| line.no == old.line)
        .ok_or("声明位置无法证明")?;
    let header = &lines[index];
    let children: Vec<_> = lines
        .iter()
        .skip(index + 1)
        .take_while(|line| line.indent > header.indent)
        .collect();
    let end = lines
        .get(index + 1 + children.len())
        .map_or(source.len(), |line| raw_lines[line.no as usize - 1].0);
    let indent = children
        .first()
        .map_or(header.indent + 2, |line| line.indent);
    let (_, header_raw) = raw_lines[old.line as usize - 1];
    let eol = if header_raw.ends_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut edits = Vec::new();
    let mut insert = String::new();
    for field in &fields {
        if field.before == field.after {
            continue;
        }
        let value = field.after.as_ref().ok_or("导入不允许删除字段")?;
        let replacement = crate::authoring::property_source(value);
        match field.field.as_str() {
            "display" | "entity_type" => {
                let tokens = tokens(header, clean_lines);
                let token = if field.field == "entity_type" {
                    tokens.get(3)
                } else {
                    tokens.get(if matches!(header.kind, LineKind::Character { .. }) {
                        3
                    } else {
                        5
                    })
                };
                let range = if let Some(token) = token {
                    range(header, token.range.clone(), raw_lines)?
                } else {
                    let last = tokens.last().ok_or("声明token缺失")?;
                    let at = range(header, last.range.clone(), raw_lines)?.end;
                    at..at
                };
                let replacement = if field.field == "entity_type" {
                    if let PropertyValue::Str(text) = value {
                        text.clone()
                    } else {
                        return Err("实体类型必须是文本身份".into());
                    }
                } else if token.is_none() {
                    format!(" as {replacement}")
                } else {
                    replacement
                };
                edits.push(Edit {
                    range,
                    replacement,
                    field: field.field.clone(),
                });
            }
            _ => {
                let child = children.iter().find(|line| match &line.kind {
                    LineKind::Property { name, .. } => field.field == format!("property.{name}"),
                    LineKind::Description { .. } => field.field == "description",
                    _ => false,
                });
                if let Some(line) = child {
                    let clean_line = &clean_lines[line.no as usize - 1];
                    let char_range = if matches!(line.kind, LineKind::Property { .. }) {
                        (line.source.base + line.source.value) as usize
                            ..clean_line.trim_end().chars().count()
                    } else {
                        tokens(line, clean_lines)
                            .get(1)
                            .ok_or("description值token缺失")?
                            .range
                            .clone()
                    };
                    let byte_range = range(line, char_range.clone(), raw_lines)?;
                    let raw_value = &source[byte_range.clone()];
                    let clean_value: String = clean_line
                        .chars()
                        .skip(char_range.start)
                        .take(char_range.end - char_range.start)
                        .collect();
                    if raw_value != clean_value {
                        return Err("字段值内部含注释，无法保证原文安全，整批未修改".into());
                    }
                    edits.push(Edit {
                        range: byte_range,
                        replacement,
                        field: field.field.clone(),
                    });
                } else {
                    let statement = if let Some(key) = field.field.strip_prefix("property.") {
                        format!("property {key} = {replacement}")
                    } else {
                        format!("description {replacement}")
                    };
                    insert.push_str(&format!("{}{statement}{eol}", " ".repeat(indent as usize)));
                }
            }
        }
    }
    if !insert.is_empty() {
        safe_insertion(source, end)?;
        if end > 0 && !source[..end].ends_with('\n') {
            insert.insert_str(0, eol);
        }
        edits.push(Edit {
            range: end..end,
            replacement: insert,
            field: "new_fields".into(),
        });
    }
    let operation = if edits.is_empty() {
        "unchanged"
    } else {
        "update"
    }
    .into();
    Ok(PreparedRow {
        path: old.path,
        edits,
        append: String::new(),
        fields,
        operation,
    })
}
fn physical_lines(source: &str) -> Vec<(usize, &str)> {
    let mut at = 0;
    source
        .split_inclusive('\n')
        .map(|line| {
            let result = (at, line);
            at += line.len();
            result
        })
        .collect()
}
fn tokens(line: &Line, clean: &[String]) -> Vec<crate::catalog_syntax::SourceToken> {
    crate::catalog_syntax::tokenize_spanned(
        &clean[line.no as usize - 1],
        &line.file,
        line.no,
        &mut Vec::new(),
    )
}
fn range(line: &Line, chars: Range<usize>, raw: &[(usize, &str)]) -> Result<Range<usize>, String> {
    let (offset, text) = raw[line.no as usize - 1];
    let byte = |at| {
        text.char_indices()
            .nth(at)
            .map(|(index, _)| index)
            .or_else(|| (at == text.chars().count()).then_some(text.len()))
            .ok_or("字段字符范围越界")
    };
    Ok(offset + byte(chars.start)?..offset + byte(chars.end)?)
}
pub(super) fn newline(source: &str) -> &str {
    if source
        .split_inclusive('\n')
        .find(|line| line.ends_with('\n'))
        .is_some_and(|line| line.ends_with("\r\n"))
    {
        "\r\n"
    } else {
        "\n"
    }
}
pub(super) fn safe_insertion(source: &str, at: usize) -> Result<(), String> {
    let probe = format!("{}\nCATALOG_IMPORT_PROBE", &source[..at]);
    if !crate::lexer::strip_comments(&probe).ends_with("CATALOG_IMPORT_PROBE") {
        return Err("插入点位于跨行块注释，无法安全追加字段".into());
    }
    Ok(())
}

/// 无需生成重构语境；每文件一次有序字节复制，拒绝重叠与非UTF-8边界。
pub(super) fn apply_fields(source: &str, mut edits: Vec<Edit>) -> Result<String, String> {
    edits.sort_by_key(|edit| edit.range.start);
    let mut after = String::with_capacity(source.len());
    let mut cursor = 0;
    for edit in edits {
        if edit.range.start < cursor || edit.range.end < edit.range.start {
            return Err("资料字段范围重叠，整批未提交".into());
        }
        after.push_str(
            source
                .get(cursor..edit.range.start)
                .ok_or("资料字段起点无效")?,
        );
        source.get(edit.range.clone()).ok_or("资料字段范围无效")?;
        after.push_str(&edit.replacement);
        cursor = edit.range.end;
    }
    after.push_str(source.get(cursor..).ok_or("资料字段终点无效")?);
    Ok(after)
}

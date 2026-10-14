use super::{
    model::*,
    projection::{object, relative_path},
};
use crate::{
    ast::PropertyValue,
    catalog::TargetRef,
    lexer::LineKind,
    project::Project,
    schemas::{SchemaBinding, SchemaDecl},
    CompileResult,
};
use std::{collections::BTreeMap, ops::Range, path::PathBuf};

struct Replacement {
    range: Range<usize>,
    text: String,
}
type Edits = BTreeMap<String, Vec<Replacement>>;

pub(super) fn changed_sources(
    project: &Project,
    compiled: &CompileResult,
    request: &SchemaAuthoringRequest,
    schema: Option<&SchemaDecl>,
    binding: Option<&SchemaBinding>,
) -> Result<BTreeMap<PathBuf, String>, String> {
    let target = object(&compiled.program, &request.target)?;
    let mut edits = Edits::new();
    let mut additions = String::new();
    let original_source = source(project, compiled, target.file)?;
    let newline = newline(original_source);
    let mut lexical = BTreeMap::new();
    let all_lines = crate::lexer::lex_source_with_options(
        target.file,
        original_source,
        &mut Vec::new(),
        compiled.options,
    );
    let indent = all_lines
        .iter()
        .find(|l| l.no > target.loc.line)
        .filter(|l| l.indent > 0)
        .map_or(2, |l| l.indent);
    lexical.insert(target.file.to_owned(), all_lines);
    for mutation in &request.fields {
        let (field_id, key, value) = match mutation {
            SchemaFieldMutation::Set {
                field_id,
                key,
                value,
            } => (field_id, key, Some(value)),
            SchemaFieldMutation::Remove { field_id, key } => (field_id, key, None),
        };
        let schema = schema.ok_or("没有操作后 schema，不能编辑约束字段")?;
        if !schema
            .fields
            .iter()
            .any(|f| &f.id == field_id && &f.key == key)
        {
            return Err("字段身份或属性键已改变；请重新打开表单".into());
        }
        if let Some(PropertyValue::Ref(reference)) = value {
            if !crate::catalog::is_object_reference_kind(&reference.kind, compiled.options) {
                return Err("强引用 kind 或能力未启用；不能隐式升级工作区".into());
            }
        }
        let property = target.properties.iter().find(|p| &p.name == key);
        if property.map(|p| &p.value) == value {
            continue;
        }
        let statement =
            value.map(|v| format!("property {key} = {}", crate::authoring::property_source(v)));
        if let Some(property) = property {
            let line = lexical_line(compiled, &mut lexical, target.file, property.loc.line)?;
            if !matches!(&line.kind, LineKind::Property { name, .. } if name == key) {
                return Err("属性编译来源与正式词法不一致；原稿未改变".into());
            }
            replace_statement(
                &mut edits,
                compiled,
                target.file,
                &line,
                statement.as_deref().unwrap_or(""),
            )?;
        } else if let Some(statement) = statement {
            additions.push_str(&format!(
                "{}{statement}{newline}",
                " ".repeat(indent as usize)
            ));
        }
    }
    if !additions.is_empty() {
        let header = lexical_line(compiled, &mut lexical, target.file, target.loc.line)?;
        if !is_header(&header.kind, &request.target) || header.indent != 0 {
            return Err("对象声明来源不一致，无法安全插入字段".into());
        }
        let offset = line_end(original_source, target.loc.line)?;
        ensure_insertion(original_source, offset)?;
        if !original_source[..offset].ends_with('\n') {
            additions.insert_str(0, newline);
        }
        edits
            .entry(target.file.into())
            .or_default()
            .push(Replacement {
                range: offset..offset,
                text: additions,
            });
    }
    match &request.binding {
        SchemaBindingMutation::Keep => {}
        SchemaBindingMutation::Bind { schema_id } => {
            ensure_insertion(original_source, original_source.len())?;
            let prefix = if original_source.is_empty() || original_source.ends_with('\n') {
                ""
            } else {
                newline
            };
            edits
                .entry(target.file.into())
                .or_default()
                .push(Replacement {
                    range: original_source.len()..original_source.len(),
                    text: format!(
                        "{prefix}bind {} {} to {schema_id}{newline}",
                        request.target.kind, request.target.id
                    ),
                });
        }
        SchemaBindingMutation::Change { schema_id } => {
            let binding = binding.ok_or("对象尚未绑定，不能变更绑定")?;
            if binding.schema_id != *schema_id {
                replace_binding(
                    &mut edits,
                    project,
                    compiled,
                    &mut lexical,
                    binding,
                    &format!(
                        "bind {} {} to {schema_id}",
                        request.target.kind, request.target.id
                    ),
                )?;
            }
        }
        SchemaBindingMutation::Unbind => {
            replace_binding(
                &mut edits,
                project,
                compiled,
                &mut lexical,
                binding.ok_or("对象尚未绑定，不能解绑")?,
                "",
            )?;
        }
    }
    let mut changed = BTreeMap::new();
    for (file, replacements) in edits {
        let original = source(project, compiled, &file)?;
        let mut grouped: BTreeMap<(usize, usize), String> = BTreeMap::new();
        for replacement in replacements {
            let key = (replacement.range.start, replacement.range.end);
            if grouped.contains_key(&key) && key.0 != key.1 {
                return Err("字段来源范围重复；原稿未改变".into());
            }
            grouped.entry(key).or_default().push_str(&replacement.text);
        }
        let mut previous = 0;
        let mut text = String::with_capacity(original.len());
        for ((start, end), replacement) in grouped {
            if start < previous || end > original.len() {
                return Err("字段来源范围交叠或已过期；原稿未改变".into());
            }
            text.push_str(&original[previous..start]);
            text.push_str(&replacement);
            previous = end;
            if text.len() > MAX_SCHEMA_AUTHORING_SOURCE_BYTES {
                return Err("schema 作者候选源码超过 4 MiB 预算".into());
            }
        }
        text.push_str(&original[previous..]);
        if text.len() > MAX_SCHEMA_AUTHORING_SOURCE_BYTES {
            return Err("schema 作者候选源码超过 4 MiB 预算".into());
        }
        if text != original {
            changed.insert(project.root.join(relative_path(project, &file)?), text);
        }
    }
    Ok(changed)
}

fn is_header(kind: &LineKind, target: &TargetRef) -> bool {
    match kind {
        LineKind::World { name, .. } => target.kind == "world" && name == &target.id,
        LineKind::Character { name, .. } => target.kind == "character" && name == &target.id,
        LineKind::Entity { name, .. } => target.kind == "entity" && name == &target.id,
        LineKind::RelationDef { id, .. } => target.kind == "relation" && id == &target.id,
        _ => false,
    }
}
fn source<'a>(
    project: &Project,
    compiled: &'a CompileResult,
    file: &str,
) -> Result<&'a str, String> {
    relative_path(project, file)?;
    let text = compiled
        .sources
        .get(&PathBuf::from(file))
        .ok_or("schema 来源未完整加载")?;
    if text.len() > MAX_SCHEMA_AUTHORING_SOURCE_BYTES {
        return Err("schema 作者被修改源码超过 4 MiB 预算".into());
    }
    Ok(text)
}
fn lexical_line(
    compiled: &CompileResult,
    cache: &mut BTreeMap<String, Vec<crate::lexer::Line>>,
    file: &str,
    line: u32,
) -> Result<crate::lexer::Line, String> {
    let source = compiled
        .sources
        .get(&PathBuf::from(file))
        .ok_or("schema 来源未完整加载")?;
    cache
        .entry(file.into())
        .or_insert_with(|| {
            crate::lexer::lex_source_with_options(file, source, &mut Vec::new(), compiled.options)
        })
        .iter()
        .find(|l| l.no == line)
        .cloned()
        .ok_or_else(|| "schema 语句来源已过期".into())
}
fn replace_binding(
    edits: &mut Edits,
    project: &Project,
    compiled: &CompileResult,
    cache: &mut BTreeMap<String, Vec<crate::lexer::Line>>,
    binding: &SchemaBinding,
    replacement: &str,
) -> Result<(), String> {
    source(project, compiled, &binding.file)?;
    let line = lexical_line(compiled, cache, &binding.file, binding.loc.line)?;
    let LineKind::Schema112 {
        keyword, source, ..
    } = &line.kind
    else {
        return Err("绑定来源已过期".into());
    };
    let parsed =
        crate::schemas::parse::binding(source, &binding.file, binding.loc, &mut Vec::new());
    if keyword != "bind" || parsed.as_ref() != Some(binding) || line.indent != 0 {
        return Err("绑定编译来源与正式词法不一致；原稿未改变".into());
    }
    replace_statement(edits, compiled, &binding.file, &line, replacement)
}
fn replace_statement(
    edits: &mut Edits,
    compiled: &CompileResult,
    file: &str,
    line: &crate::lexer::Line,
    replacement: &str,
) -> Result<(), String> {
    let source = compiled
        .sources
        .get(&PathBuf::from(file))
        .ok_or("schema 来源未完整加载")?;
    let span = line.statement_source().span;
    let start = line_start(source, span.line)?;
    let raw = source[start..].split('\n').next().unwrap_or("");
    let offset = |column: u32| -> Result<usize, String> {
        raw.char_indices()
            .map(|(at, _)| at)
            .chain(std::iter::once(raw.len()))
            .nth(column as usize)
            .map(|at| start + at)
            .ok_or_else(|| "schema 来源列超出原稿".into())
    };
    let begin = span
        .column
        .checked_sub(1)
        .ok_or("schema 来源列必须从 1 开始")?;
    let range = offset(begin)?..offset(begin + span.length)?;
    let mut text = replacement.to_owned();
    for comment in crate::lexer::comment_source_spans(source) {
        if comment.range.end <= range.start || comment.range.start >= range.end {
            continue;
        }
        if !comment.closed || comment.range.start < range.start || comment.range.end > range.end {
            return Err("块注释跨越选中语句，无法证明安全改写范围；请使用源码编辑".into());
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&source[comment.range]);
    }
    edits
        .entry(file.into())
        .or_default()
        .push(Replacement { range, text });
    Ok(())
}
fn line_start(source: &str, number: u32) -> Result<usize, String> {
    if number == 0 {
        return Err("schema 来源行必须从 1 开始".into());
    }
    let mut at = 0;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        if index + 1 == number as usize {
            return Ok(at);
        }
        at += line.len();
    }
    Err("schema 来源行已过期".into())
}
fn line_end(source: &str, number: u32) -> Result<usize, String> {
    let start = line_start(source, number)?;
    Ok(start
        + source[start..]
            .split_inclusive('\n')
            .next()
            .map_or(0, str::len))
}
fn ensure_insertion(source: &str, offset: usize) -> Result<(), String> {
    if crate::lexer::comment_source_spans(source)
        .iter()
        .any(|c| !c.closed || (c.range.start < offset && offset < c.range.end))
    {
        return Err("插入位置位于未闭合或跨行注释内；原稿未改变".into());
    }
    Ok(())
}
fn newline(source: &str) -> &'static str {
    if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

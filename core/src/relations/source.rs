use super::{RelationDirection, RelationDraft, RelationTypeDraft};
use crate::catalog::TargetRef;
use std::path::Path;

pub(super) fn relation_type_source(draft: &RelationTypeDraft) -> Result<String, String> {
    let mut out = format!(
        "relation_type {} as {}\n",
        draft.id,
        crate::authoring::quote(&draft.display)
    );
    if let Some(inverse) = &draft.inverse_display {
        out.push_str(&format!("  inverse {}\n", crate::authoring::quote(inverse)));
    }
    out.push_str(&format!(
        "  direction {}\n",
        match draft.direction {
            RelationDirection::Directed => "directed",
            RelationDirection::Undirected => "undirected",
        }
    ));
    if let Some(kind) = &draft.from_kind {
        out.push_str(&format!("  from {kind}\n"));
    }
    if let Some(kind) = &draft.to_kind {
        out.push_str(&format!("  to {kind}\n"));
    }
    Ok(out)
}

pub(super) fn relation_source(
    draft: &RelationDraft,
    declaration_file: &Path,
) -> Result<String, String> {
    let from = relation_target_source(&draft.from, declaration_file)?;
    let to = relation_target_source(&draft.to, declaration_file)?;
    let mut out = format!(
        "relation_def {} type {} from {} to {}\n",
        draft.id, draft.relation_type, from, to
    );
    if !draft.description.is_empty() {
        out.push_str(&format!(
            "  description {}\n",
            crate::authoring::quote(&draft.description)
        ));
    }
    if let Some(note) = &draft.source_note {
        out.push_str(&format!(
            "  source_note {}\n",
            crate::authoring::quote(note)
        ));
    }
    for scope in &draft.scope_refs {
        out.push_str(&format!(
            "  scope {}\n",
            relation_target_source(scope, declaration_file)?
        ));
    }
    for (name, value) in &draft.properties {
        crate::authoring::identifier(name)?;
        out.push_str(&format!(
            "  property {name} = {}\n",
            crate::authoring::property_source(value)
        ));
    }
    Ok(out)
}

fn relation_target_source(target: &TargetRef, declaration_file: &Path) -> Result<String, String> {
    if target.kind != "file" {
        return Ok(format!("{} {}", target.kind, target.id));
    }
    let parent = declaration_file.parent().unwrap_or(Path::new("."));
    let relative = crate::catalog_edit::relative_source_path(parent, Path::new(&target.id))?;
    Ok(format!("file {}", crate::authoring::quote(&relative)))
}

pub(super) fn append_relation_source(
    project: &mut crate::project::Project,
    path: &Path,
    source: &str,
) -> Result<(), String> {
    let mut text = project.document(path)?.to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(source);
    project.set_text(path, text)
}

pub(super) fn replace_relation_block(
    project: &mut crate::project::Project,
    path: &Path,
    line_no: u32,
    type_decl: bool,
    source: &str,
) -> Result<(), String> {
    let text = project.document(path)?.to_string();
    let parsed = crate::lexer::lex_source_with_options(
        &path.to_string_lossy(),
        &text,
        &mut Vec::new(),
        project.compile_options(),
    );
    let index = parsed
        .iter()
        .position(|line| {
            line.no == line_no
                && if type_decl {
                    matches!(line.kind, crate::lexer::LineKind::RelationType { .. })
                } else {
                    matches!(line.kind, crate::lexer::LineKind::RelationDef { .. })
                }
        })
        .ok_or("关系声明源位置不存在")?;
    let indent = parsed[index].indent;
    let next = parsed
        .iter()
        .skip(index + 1)
        .find(|line| line.indent <= indent)
        .map(|line| line.no)
        .unwrap_or_else(|| text.lines().count() as u32 + 1);
    let start = line_start(&text, line_no);
    let end = if next <= text.lines().count() as u32 {
        line_start(&text, next)
    } else {
        text.len()
    };
    let retained = crate::authoring::comments(&text[start..end]);
    let mut replacement = retained;
    replacement.push_str(source);
    if !replacement.is_empty() && !replacement.ends_with('\n') {
        replacement.push('\n');
    }
    let mut text = text;
    text.replace_range(start..end, &replacement);
    project.set_text(path, text)
}

pub(super) fn line_start(text: &str, line: u32) -> usize {
    text.split_inclusive('\n')
        .take(line.saturating_sub(1) as usize)
        .map(str::len)
        .sum()
}

pub(super) fn line_end(text: &str, line: u32) -> usize {
    let start = line_start(text, line);
    text[start..]
        .find('\n')
        .map(|offset| start + offset + 1)
        .unwrap_or(text.len())
}

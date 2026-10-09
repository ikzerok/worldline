use super::*;
use crate::ast::{Program, Stmt};
use crate::project::Project;
use crate::source_provenance::{SourceOwner, StatementKind};
use std::path::Path;

#[derive(Clone)]
pub(super) struct Record {
    pub id: Option<String>,
    pub unit: SourceUnit,
}

pub(super) fn current(project: &Project) -> Result<(crate::CompileResult, Vec<Record>), String> {
    limits::project(project)?;
    #[cfg(all(test, not(target_arch = "wasm32")))]
    record_compilation();
    let compiled = project.compile_problems_snapshot();
    if compiled.has_errors() {
        return Err("COMPILE_ERROR：工程存在编译错误，不能建立本地化目录".into());
    }
    let records = collect(&compiled.program, &project.root)?;
    Ok((compiled, records))
}

pub(super) fn collect(program: &Program, root: &Path) -> Result<Vec<Record>, String> {
    collect_mode(program, root, false, true)
}

pub(super) fn collect_annotated(program: &Program, root: &Path) -> Result<Vec<Record>, String> {
    collect_mode(program, root, true, false)
}

fn collect_mode(
    program: &Program,
    root: &Path,
    identified_only: bool,
    bounded: bool,
) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    let mut state = WalkState {
        payload_bytes: 0,
        identified_only,
        bounded,
    };
    for (index, event) in program.events.iter().enumerate() {
        let file = program
            .event_files
            .get(index)
            .ok_or("事件缺少源码文件映射")?;
        let owner = SourceOwner::new(file, event.loc.line);
        walk(program, &event.body, &owner, root, &mut records, &mut state)?;
    }
    for fragment in &program.fragments {
        let owner = SourceOwner::new(&fragment.file, fragment.loc.line);
        walk(
            program,
            &fragment.body,
            &owner,
            root,
            &mut records,
            &mut state,
        )?;
    }
    records.sort_by(|a, b| {
        (
            &a.unit.source.file,
            a.unit.source.line,
            &a.unit.source.kind,
            &a.id,
        )
            .cmp(&(
                &b.unit.source.file,
                b.unit.source.line,
                &b.unit.source.kind,
                &b.id,
            ))
    });
    Ok(records)
}

struct WalkState {
    payload_bytes: usize,
    identified_only: bool,
    bounded: bool,
}

fn walk(
    program: &Program,
    statements: &[Stmt],
    owner: &SourceOwner,
    root: &Path,
    out: &mut Vec<Record>,
    state: &mut WalkState,
) -> Result<(), String> {
    let bounded = state.bounded;
    let identified_only = state.identified_only;
    for statement in statements {
        let unit = match statement {
            Stmt::Text(text) => Some((
                "text",
                &text.parts,
                text.glue,
                text.loc.line,
                &text.localization_id,
            )),
            Stmt::Say(say) => Some((
                "say",
                &say.text.parts,
                say.text.glue,
                say.loc.line,
                &say.text.localization_id,
            )),
            Stmt::Choice(choice) => Some((
                "choice",
                &choice.label,
                false,
                choice.loc.line,
                &choice.localization_id,
            )),
            _ => None,
        };
        if let Some((kind, parts, glue, line, id)) =
            unit.filter(|unit| !identified_only || unit.4.is_some())
        {
            let file = program
                .source_provenance
                .statement_file(
                    owner,
                    crate::language::statement_loc(statement),
                    StatementKind::of(statement),
                )
                .ok_or("INVALID_SOURCE：正文真实来源缺失或位置歧义，请修正来源并重新导出交换包")?;
            if bounded {
                limits::budget(out.len() < MAX_LOCALIZATION_UNITS, "目录 AST 单元数")?;
                if let Some(id) = id {
                    limits::id(id)?;
                }
                limits::budget(parts.len() <= MAX_LOCALIZATION_PARTS, "源 typed parts 数")?;
                let visible_bytes = parts.iter().fold(0usize, |size, part| {
                    size.saturating_add(match part {
                        crate::ast::TextPart::Str(text) => text.len(),
                        crate::ast::TextPart::Link(link) => link.label.len(),
                        crate::ast::TextPart::Expr(_) => 0,
                    })
                });
                limits::budget(
                    visible_bytes <= MAX_LOCALIZATION_UNIT_BYTES,
                    "源单元 UTF-8 字节",
                )?;
                let relative = Path::new(file)
                    .strip_prefix(root)
                    .map_err(|_| "来源不在工作区内")?
                    .to_str()
                    .ok_or("来源路径不是 UTF-8")?;
                // Check the incoming path before allocating another per-unit path copy.
                limits::serialized_len(
                    &relative,
                    limits::MAX_SNAPSHOT_BYTES.saturating_sub(state.payload_bytes),
                    "完整来源元数据",
                )?;
            }
            let parts_view = export::source_parts(parts);
            if bounded {
                limits::parts(&parts_view)?;
            }
            let unit = SourceUnit {
                source: export::source_reference(root, Path::new(file), line, kind)?,
                parts: parts_view,
                source_revision: export::source_revision(kind, parts, glue),
            };
            if bounded {
                limits::reserve(&(&id, &unit), &mut state.payload_bytes, "完整来源元数据")?;
            }
            out.push(Record {
                id: id.clone(),
                unit,
            });
        }
        match statement {
            Stmt::Choice(choice) => walk(program, &choice.body, owner, root, out, state)?,
            Stmt::If(condition) => {
                for (_, body) in &condition.branches {
                    walk(program, body, owner, root, out, state)?;
                }
            }
            Stmt::Scene(scene) => walk(program, &scene.body, owner, root, out, state)?,
            _ => {}
        }
    }
    Ok(())
}

/// Resolve the actual statement/header range in an immutable compiled source snapshot.
/// Does not compile or execute. Callers must verify the live source/session and workspace
/// navigation guards; `draft` only carries the already-verified authoring identity.
pub fn localization_source_hit(
    compiled: &crate::CompileResult,
    root: &Path,
    source: &LocalizationSource,
    draft: bool,
) -> Result<crate::search_replace::SearchMatch, String> {
    let records = collect(&compiled.program, root)?;
    if records
        .iter()
        .filter(|record| &record.unit.source == source)
        .count()
        != 1
    {
        return Err("本地化来源不属于本次完整编译快照的唯一正文单元".into());
    }
    let path = root.join(&source.file);
    let text = compiled
        .sources
        .get(&path)
        .ok_or("本地化来源文件不在编译快照中")?;
    let mut start = 0usize;
    let mut selected = None;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if index as u32 + 1 == source.line {
            selected = Some((start, line));
            break;
        }
        start += line.len();
    }
    let (start, line) = selected.ok_or("本地化来源行不存在")?;
    let comments = crate::lexer::comment_source_spans(text);
    let visible =
        statement_code_range(line, start, &comments).ok_or("本地化来源没有可定位的语句")?;
    let first = visible.start;
    let range = (start + visible.start)..(start + visible.end);
    limits::budget(range.len() <= MAX_LOCALIZATION_UNIT_BYTES, "来源语句范围")?;
    let preview = text
        .get(range.clone())
        .ok_or("本地化来源范围不是有效 UTF-8 边界")?
        .into();
    Ok(crate::search_replace::SearchMatch {
        path,
        range,
        line: source.line,
        column: line[..first].chars().count() as u32 + 1,
        preview,
        replaceable: false,
        draft,
        context: None,
        identity: None,
    })
}

/// Monotonic scan after one binary search; does not rescan every comment for every character.
pub(super) fn statement_code_range(
    line: &str,
    start: usize,
    comments: &[crate::lexer::CommentSource],
) -> Option<std::ops::Range<usize>> {
    let mut comment = comments.partition_point(|comment| comment.range.end <= start);
    let mut first = None;
    let mut end = 0usize;
    for (offset, ch) in line.char_indices() {
        let absolute = start + offset;
        while comments
            .get(comment)
            .is_some_and(|current| current.range.end <= absolute)
        {
            comment += 1;
        }
        if ch.is_whitespace()
            || comments
                .get(comment)
                .is_some_and(|current| current.range.contains(&absolute))
        {
            continue;
        }
        first.get_or_insert(offset);
        end = offset + ch.len_utf8();
    }
    first.map(|first| first..end)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
thread_local! {
    static COMPILATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn record_compilation() {
    COMPILATIONS.with(|count| count.set(count.get() + 1));
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn take_compilation_count() -> usize {
    COMPILATIONS.with(|count| count.replace(0))
}

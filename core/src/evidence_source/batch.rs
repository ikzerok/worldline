//! 一次调用内按文件复用正式词法结果；身份验证与单项入口完全共用。
use super::state_actions::{self, StateActionSource};
use super::variable_writes::{self, VariableWriteSource};
use super::{EvidenceSource, EvidenceSourceOwner, EvidenceSourcePrecision, EvidenceSourceTarget};
use crate::lexer::{Line, LineKind};
use crate::{CompileOptions, CompileResult, Diagnostic};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::PathBuf;

pub const MAX_EVIDENCE_SOURCE_BATCH: usize = 514;
pub const MAX_EVIDENCE_SOURCE_BATCH_BYTES: usize = 1024 * 1024;
type LexSource = fn(&str, &str, &mut Vec<Diagnostic>, CompileOptions) -> Vec<Line>;

/// 单项接口不引入新的输入限制；批量优化复用同一权威校验过程。
pub fn resolve_evidence_source(
    snapshot: &CompileResult,
    source: &EvidenceSource,
) -> Result<EvidenceSourceTarget, String> {
    resolve_inner(snapshot, &[source], crate::lexer::lex_source_with_options)
        .pop()
        .expect("单项定位恰好返回一项")
}

/// 顺序与重复项原样保留；超出批量额度时整体拒绝，不返回截断成功。
pub fn resolve_evidence_sources(
    snapshot: &CompileResult,
    sources: &[&EvidenceSource],
) -> Result<Vec<Result<EvidenceSourceTarget, String>>, String> {
    check_limits(sources)?;
    Ok(resolve_inner(
        snapshot,
        sources,
        crate::lexer::lex_source_with_options,
    ))
}
fn check_limits(sources: &[&EvidenceSource]) -> Result<(), String> {
    if sources.len() > MAX_EVIDENCE_SOURCE_BATCH {
        return Err("批量证据来源条数超过514项".into());
    }
    let mut used = 0usize;
    for source in sources {
        let (identity, detail) = match &source.owner {
            EvidenceSourceOwner::Choice { node } => (node.as_str(), ""),
            EvidenceSourceOwner::Rule { name } => (name.as_str(), ""),
            EvidenceSourceOwner::StateAction { node, timing, .. } => {
                (node.as_str(), timing.as_str())
            }
            EvidenceSourceOwner::VariableWrite { node, variable, .. } => {
                (node.as_str(), variable.as_str())
            }
        };
        for field in [source.file.as_str(), identity, detail] {
            used = used
                .checked_add(field.len())
                .filter(|used| *used <= MAX_EVIDENCE_SOURCE_BATCH_BYTES)
                .ok_or("批量证据来源字符串超过1 MiB")?;
        }
    }
    Ok(())
}
struct Prepared<'a> {
    source: &'a EvidenceSource,
    path: PathBuf,
    text: &'a str,
    action: Option<StateActionSource<'a>>,
    variable_write: Option<VariableWriteSource<'a>>,
}
fn prepare<'a>(
    snapshot: &'a CompileResult,
    source: &'a EvidenceSource,
) -> Result<Prepared<'a>, String> {
    if source.line == 0 || source.file.is_empty() {
        return Err("证据没有有效的声明来源".into());
    }
    let path = PathBuf::from(&source.file);
    let text = snapshot
        .sources
        .get(&path)
        .ok_or("证据来源不属于此编译快照")?;
    let action = state_actions::find(&snapshot.program, source.line, &source.owner);
    let variable_write = variable_writes::find(&snapshot.program, source.line, &source.owner);
    let valid = match &source.owner {
        EvidenceSourceOwner::Choice { node } => super::choice_body(snapshot, &source.file, node)
            .is_some_and(|body| super::count_choices(body, source.line) == 1),
        EvidenceSourceOwner::Rule { name } => {
            snapshot
                .program
                .rules
                .iter()
                .filter(|rule| {
                    rule.name == *name && rule.file == source.file && rule.loc.line == source.line
                })
                .count()
                == 1
        }
        EvidenceSourceOwner::StateAction { .. } => action
            .as_ref()
            .is_some_and(|action| action.file == source.file),
        EvidenceSourceOwner::VariableWrite { .. } => variable_write
            .as_ref()
            .is_some_and(|write| write.file == source.file),
    };
    if !valid {
        return Err("证据所属声明已变化或无法唯一确认".into());
    }
    Ok(Prepared {
        source,
        path,
        text,
        action,
        variable_write,
    })
}
struct HeaderRange {
    range: Range<usize>,
    column: u32,
}
fn header_ranges(text: &str, wanted: &BTreeSet<u32>) -> BTreeMap<u32, Result<HeaderRange, String>> {
    let mut result = BTreeMap::new();
    let mut offset = 0;
    for (index, raw) in text.split_inclusive('\n').enumerate() {
        if let Ok(line) = u32::try_from(index + 1) {
            if wanted.contains(&line) {
                let header = raw.trim_end_matches(['\r', '\n']);
                let start = header.len() - header.trim_start().len();
                let range = if start == header.len() {
                    Err("证据声明头为空".into())
                } else {
                    Ok(HeaderRange {
                        range: offset + start..offset + header.len(),
                        column: header[..start].chars().count() as u32 + 1,
                    })
                };
                result.insert(line, range);
            }
        }
        offset += raw.len();
    }
    result
}
fn finish(
    prepared: &Prepared<'_>,
    kind: Option<&LineKind>,
    range: Option<&Result<HeaderRange, String>>,
) -> Result<EvidenceSourceTarget, String> {
    let kind = kind.ok_or("证据声明头已不存在")?;
    let classified = match (&prepared.source.owner, kind) {
        (EvidenceSourceOwner::Choice { .. }, LineKind::Choice { .. }) => true,
        (EvidenceSourceOwner::Rule { .. }, LineKind::Language111 { keyword, .. }) => {
            keyword == "rule"
        }
        (EvidenceSourceOwner::StateAction { action, .. }, kind) => prepared
            .action
            .as_ref()
            .is_some_and(|source| source.matches_header(*action, kind)),
        (EvidenceSourceOwner::VariableWrite { .. }, kind) => prepared
            .variable_write
            .as_ref()
            .is_some_and(|source| source.matches_header(kind)),
        _ => false,
    };
    if !classified {
        return Err("证据来源不再是对应的声明头".into());
    }
    let range = range
        .ok_or("证据声明头范围已不存在")?
        .as_ref()
        .map_err(Clone::clone)?;
    Ok(EvidenceSourceTarget {
        path: prepared.path.clone(),
        range: range.range.clone(),
        line: prepared.source.line,
        column: range.column,
        precision: EvidenceSourcePrecision::StatementHeader,
    })
}
fn resolve_inner(
    snapshot: &CompileResult,
    sources: &[&EvidenceSource],
    lex: LexSource,
) -> Vec<Result<EvidenceSourceTarget, String>> {
    let mut prepared = Vec::with_capacity(sources.len());
    let mut results: Vec<Option<Result<EvidenceSourceTarget, String>>> = vec![None; sources.len()];
    let mut files: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, source) in sources.iter().enumerate() {
        match prepare(snapshot, source) {
            Ok(value) => {
                files.entry(source.file.as_str()).or_default().push(index);
                prepared.push(Some(value));
            }
            Err(error) => {
                prepared.push(None);
                results[index] = Some(Err(error));
            }
        }
    }
    for (file, indices) in files {
        let first = prepared[indices[0]]
            .as_ref()
            .expect("文件组只包含已验证声明");
        let wanted = indices
            .iter()
            .map(|index| sources[*index].line)
            .collect::<BTreeSet<_>>();
        let mut headers = BTreeMap::new();
        for line in lex(file, first.text, &mut Vec::new(), snapshot.options) {
            if wanted.contains(&line.no) {
                headers
                    .entry(line.no)
                    .or_insert_with(|| line.physical().kind);
            }
        }
        let ranges = header_ranges(first.text, &wanted);
        for index in indices {
            let source = prepared[index].as_ref().expect("文件组只包含已验证声明");
            results[index] = Some(finish(
                source,
                headers.get(&source.source.line),
                ranges.get(&source.source.line),
            ));
        }
        // 当前文件的词法头和范围在这里释放，不跨文件/调用/快照保存。
    }
    results
        .into_iter()
        .map(|result| result.expect("每个来源均已生成结果"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ChangeKind;
    use crate::evidence_source::{variable_write_source, VariableWriteOperation};
    use std::cell::Cell;
    thread_local! { static LEX_CALLS: Cell<usize> = const { Cell::new(0) }; }
    fn counted_lex(
        file: &str,
        text: &str,
        diagnostics: &mut Vec<Diagnostic>,
        options: CompileOptions,
    ) -> Vec<Line> {
        LEX_CALLS.with(|calls| calls.set(calls.get() + 1));
        crate::lexer::lex_source_with_options(file, text, diagnostics, options)
    }
    #[test]
    fn repeated_actions_variables_and_duplicate_inputs_lex_a_file_once() {
        let compiled = crate::compile_source("batch.wl", "tag one\nworld setting\nstate fate on world setting with []\nevent start\n  become fate with one\n  become fate add one\n  let total = 0\n  set total = 1\n  -> END\n");
        assert!(!compiled.has_errors());
        let mut sources = [5, 6, 5]
            .map(|line| EvidenceSource {
                file: "batch.wl".into(),
                line,
                owner: EvidenceSourceOwner::StateAction {
                    node: "start".into(),
                    action: if line == 5 {
                        ChangeKind::Become
                    } else {
                        ChangeKind::AddTags
                    },
                    timing: "during".into(),
                    effect_index: None,
                    action_index: None,
                },
            })
            .to_vec();
        sources.extend([7, 8].map(|line| {
            variable_write_source(
                &compiled.program,
                line,
                &EvidenceSourceOwner::VariableWrite {
                    node: "start".into(),
                    variable: "total".into(),
                    operation: if line == 7 {
                        VariableWriteOperation::Let
                    } else {
                        VariableWriteOperation::Set
                    },
                },
            )
            .unwrap()
        }));
        let refs = sources.iter().collect::<Vec<_>>();
        LEX_CALLS.with(|calls| calls.set(0));
        let results = resolve_inner(&compiled, &refs, counted_lex);
        assert!(results.iter().all(Result::is_ok));
        assert_eq!(results[0], results[2]);
        LEX_CALLS.with(|calls| assert_eq!(calls.get(), 1));
    }
}

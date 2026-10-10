//! 每次投影一次来源索引/注释扫描；不改变正式 owner 或深度优先顺序。
use super::*;
use crate::source_provenance::{SourceOwner, StatementKind};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::Path,
};

pub(super) type Records<'a> = BTreeMap<(String, u32), &'a Stmt>;

pub(super) fn records<'a>(
    result: &'a CompileResult,
    nodes: &[ReviewNode],
    path: &Path,
) -> Records<'a> {
    let mut wanted = BTreeSet::new();
    let mut pending = vec![nodes];
    while let Some(nodes) = pending.pop() {
        for node in nodes {
            if let Some(source) = &node.source {
                if Path::new(&source.file) == path {
                    wanted.insert((source.file.clone(), source.line));
                }
            }
            pending.push(&node.children);
        }
    }
    let mut out = Records::new();
    for (event, file) in result
        .program
        .events
        .iter()
        .zip(&result.program.event_files)
    {
        collect(
            result,
            &event.body,
            &SourceOwner::new(file, event.loc.line),
            &wanted,
            &mut out,
        );
    }
    for fragment in &result.program.fragments {
        collect(
            result,
            &fragment.body,
            &SourceOwner::new(&fragment.file, fragment.loc.line),
            &wanted,
            &mut out,
        );
    }
    out
}

fn collect<'a>(
    result: &CompileResult,
    body: &'a [Stmt],
    owner: &SourceOwner,
    wanted: &BTreeSet<(String, u32)>,
    out: &mut Records<'a>,
) {
    let mut pending = vec![body.iter()];
    while let Some(iter) = pending.last_mut() {
        let Some(statement) = iter.next() else {
            pending.pop();
            continue;
        };
        let loc = crate::language::statement_loc(statement);
        if let Some(file) = result.program.source_provenance.statement_file(
            owner,
            loc,
            StatementKind::of(statement),
        ) {
            let key = (file.into(), loc.line);
            if wanted.contains(&key) {
                out.insert(key, statement);
            }
        }
        match statement {
            Stmt::If(branches) => {
                for (_, body) in branches.branches.iter().rev() {
                    pending.push(body.iter());
                }
            }
            Stmt::Choice(choice) => pending.push(choice.body.iter()),
            Stmt::Scene(scene) => pending.push(scene.body.iter()),
            _ => {}
        }
    }
}

pub(super) struct Anchors {
    comments: Vec<Range<usize>>,
    unclosed: bool,
    seen: BTreeSet<String>,
}
impl Anchors {
    pub(super) fn new(source: &str) -> Self {
        let comments = crate::lexer::comment_source_spans(source);
        Self {
            unclosed: comments.iter().any(|comment| !comment.closed),
            comments: comments.into_iter().map(|comment| comment.range).collect(),
            seen: BTreeSet::new(),
        }
    }
    pub(super) fn safe_at(&self, at: usize) -> bool {
        if self.unclosed {
            return false;
        }
        let before = self.comments.partition_point(|span| span.start < at);
        before == 0 || self.comments[before - 1].end <= at
    }
    pub(super) fn insert(&mut self, id: &str) -> bool {
        self.seen.insert(id.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_comment_bounds_keep_strict_interior_and_unclosed_rules() {
        let source = "say a \"//字面\" //尾注\n/*跨\n行*/ next\n";
        let cached = Anchors::new(source);
        let original = crate::lexer::comment_source_spans(source);
        for at in 0..=source.len() {
            assert_eq!(
                cached.safe_at(at),
                !original.iter().any(|comment| !comment.closed
                    || (comment.range.start < at && comment.range.end > at))
            );
        }
        let unclosed = Anchors::new("//普通\n/*未闭合");
        assert!(!unclosed.safe_at(0));
        assert!(!unclosed.safe_at(100));
    }
}

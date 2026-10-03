use super::*;

impl Parser<'_> {
    /// 解析效果块体:仅允许 grant / revoke / meet / part / to 动作行。
    pub(super) fn parse_effect_actions(&mut self, parent_indent: u32, file: &str) -> Vec<Change> {
        let Some(first) = self.peek() else {
            self.diags.push(Diagnostic::error(
                "P005",
                file,
                self.missing_body_span(),
                "effect 之后缺少缩进的动作块",
            ));
            return Vec::new();
        };
        if first.indent <= parent_indent {
            self.diags.push(Diagnostic::error(
                "P005",
                file,
                self.missing_body_span(),
                "effect 之后缺少缩进的动作块",
            ));
            return Vec::new();
        }
        let block_indent = first.indent;
        let mut actions = Vec::new();
        while let Some(line) = self.peek() {
            if line.indent <= parent_indent {
                break;
            }
            if line.indent != block_indent {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &self.file_of(&line),
                    Span::new(line.no, line.indent + 1, 1),
                    format!("缩进不一致:同一效果块内应保持 {block_indent} 个空格"),
                ));
                self.next();
                continue;
            }
            if let Some(owner) = &self.source_owner {
                let loc = match &line.kind {
                    LineKind::Become(change) => Some(change.loc),
                    LineKind::ChangeLine { loc, .. } | LineKind::ToLine { loc, .. } => Some(*loc),
                    _ => None,
                };
                if let Some(loc) = loc {
                    self.sources.record_statement(
                        owner,
                        loc,
                        crate::source_provenance::StatementKind::Change,
                        &line.file,
                    );
                }
            }
            match &line.kind {
                LineKind::Become(change) => {
                    actions.push(change.clone());
                    self.next();
                }
                LineKind::ChangeLine {
                    kind,
                    id,
                    note,
                    loc,
                } => {
                    actions.push(Change {
                        tags: Vec::new(),
                        kind: *kind,
                        id: id.clone(),
                        note: note.clone(),
                        to_storyline: None,
                        loc: *loc,
                    });
                    self.next();
                }
                LineKind::ToLine {
                    storyline,
                    note,
                    loc,
                } => {
                    actions.push(Change {
                        tags: Vec::new(),
                        kind: ChangeKind::To,
                        id: String::new(),
                        note: note.clone(),
                        to_storyline: Some(storyline.clone()),
                        loc: *loc,
                    });
                    self.next();
                }
                _ => {
                    let (no, col) = match &line.kind {
                        LineKind::Text { loc, .. } => (loc.line, loc.column),
                        _ => (line.no, line.indent + 1),
                    };
                    self.diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, col, 6),
                        "效果块内只能是 become / grant / revoke / meet / part / to 动作",
                    ));
                    self.next();
                }
            }
        }
        actions
    }
}

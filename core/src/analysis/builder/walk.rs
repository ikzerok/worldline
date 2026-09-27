use super::{shorten_label, Ctx, NodeCtx};
use crate::analysis_helpers::first_divert;
use crate::ast::*;
use crate::diagnostic::{Diagnostic, Span};
use crate::graph::{AnchorDecl, EdgeKind};
use std::collections::HashSet;
impl<'a> Ctx<'a> {
    /// 第三遍:遍历所有事件体,做引用/类型检查并产出图边。
    pub(super) fn walk_all(&mut self) {
        for idx in 0..self.program.events.len() {
            let name = self.program.events[idx].name.clone();
            if self.symbols.events.get(&name).map(|p| p.event) != Some(idx) {
                continue; // 重复定义的后者跳过
            }
            self.cur_file = self
                .program
                .event_files
                .get(idx)
                .cloned()
                .unwrap_or_default();
            let node = NodeCtx {
                event: idx,
                node_name: name.clone(),
            };
            // after 前置条件类型检查
            if let Some(after) = &self.program.events[idx].after {
                self.check_expr(after, Some(ValueKind::Bool));
            }
            // 效果块:条件与动作校验
            let effects = self.program.events[idx].effects.clone();
            for fx in &effects {
                if let Some(cond) = &fx.cond {
                    self.check_expr(cond, Some(ValueKind::Bool));
                }
                for a in &fx.actions {
                    self.check_change(a);
                }
            }
            let body = self.program.events[idx].body.clone();
            self.walk_block(&body, &node, 0);
        }
    }

    /// 变动动作校验:角色引用(A208)与故事线引用(A210)。
    fn check_change(&mut self, a: &Change) {
        match a.kind {
            ChangeKind::Meet | ChangeKind::Part => {
                if !a.id.is_empty() && !self.symbols.characters.contains_key(&a.id) {
                    self.diags.push(Diagnostic::error(
                        "A208",
                        &self.cur_file,
                        Span::new(a.loc.line, a.loc.column, a.id.chars().count() as u32),
                        format!("{}引用了未定义角色 `{}`", a.kind.label(), a.id),
                    ));
                }
            }
            ChangeKind::To => {
                let Some(sl) = &a.to_storyline else { return };
                if !sl.is_empty() && !self.symbols.storylines.contains_key(sl) {
                    self.diags.push(Diagnostic::error(
                        "A210",
                        &self.cur_file,
                        Span::new(a.loc.line, a.loc.column, sl.chars().count() as u32),
                        format!("主线变动的目标故事线 `{sl}` 不存在"),
                    ));
                }
            }
            ChangeKind::Grant
            | ChangeKind::Revoke
            | ChangeKind::Become
            | ChangeKind::AddTags
            | ChangeKind::RemoveTags => {}
        }
    }

    fn walk_block(&mut self, stmts: &[Stmt], node: &NodeCtx, depth: u32) {
        let mut i = 0;
        while i < stmts.len() {
            match &stmts[i] {
                Stmt::Text(t) => {
                    for p in &t.parts {
                        if let TextPart::Expr(e) = p {
                            self.check_expr(e, None);
                        }
                    }
                }
                Stmt::Divert(d) => {
                    self.check_divert(d, node, depth);
                }
                Stmt::Choice(first) => {
                    // 选择组:连续 Choice 语句构成一组
                    let mut j = i;
                    let mut labels: HashSet<String> = HashSet::new();
                    let mut all_cond = true;
                    while let Some(Stmt::Choice(c)) = stmts.get(j) {
                        let label = c.label_raw.trim().to_string();
                        if !labels.insert(label.clone()) {
                            self.diags.push(Diagnostic::hint(
                                "A207",
                                &self.cur_file,
                                Span::new(c.loc.line, c.loc.column, 6),
                                format!("同一选择组内标签重复:`{label}`"),
                            ));
                        }
                        if let Some(cond) = &c.cond {
                            self.check_expr(cond, Some(ValueKind::Bool));
                        } else {
                            all_cond = false;
                        }
                        for p in &c.label {
                            if let TextPart::Expr(e) = p {
                                self.check_expr(e, None);
                            }
                        }
                        // Choice 边:选择体内预序首个跃迁
                        if let Some(DivertTarget::Node(t)) = first_divert(&c.body) {
                            let from = node.node_name.clone();
                            let file = self.cur_file.clone();
                            let line = c.loc.line;
                            let label = shorten_label(&c.label_raw);
                            self.add_edge(&from, t, EdgeKind::Choice, Some(label), &file, line);
                        }
                        self.walk_block(&c.body, node, depth + 1);
                        j += 1;
                    }
                    if all_cond && j > i {
                        self.diags.push(Diagnostic::warning(
                            "A203",
                            &self.cur_file,
                            Span::new(first.loc.line, first.loc.column, 6),
                            "选择组内所有分支都带条件:全部不满足时将直接穿过本组(fallback 落穿)",
                        ));
                    }
                    i = j;
                    continue;
                }
                Stmt::If(s) => {
                    for (cond, body) in &s.branches {
                        if let Some(c) = cond {
                            self.check_expr(c, Some(ValueKind::Bool));
                        }
                        self.walk_block(body, node, depth + 1);
                    }
                }
                Stmt::Let(l) => {
                    self.check_expr(&l.expr, None);
                }
                Stmt::Set(s) => {
                    let expected = match self.symbols.vars.get(&s.name) {
                        Some(v) => {
                            if v.is_const {
                                self.diags.push(Diagnostic::error(
                                    "A106",
                                    &self.cur_file,
                                    Span::new(
                                        s.loc.line,
                                        s.loc.column,
                                        s.name.chars().count() as u32,
                                    ),
                                    format!("不能对常量 `{}` 赋值", s.name),
                                ));
                            }
                            v.kind
                        }
                        None => {
                            self.diags.push(Diagnostic::error(
                                "A102",
                                &self.cur_file,
                                Span::new(s.loc.line, s.loc.column, s.name.chars().count() as u32),
                                format!("`set` 的目标 `{}` 未声明(需要先 let)", s.name),
                            ));
                            None
                        }
                    };
                    self.check_expr(&s.expr, expected);
                }
                Stmt::Scene(s) => {
                    let inner = NodeCtx {
                        event: node.event,
                        node_name: format!("{}.{}", node.node_name, s.name),
                    };
                    let file = self.cur_file.clone();
                    let line = s.loc.line;
                    let from = node.node_name.clone();
                    let to = inner.node_name.clone();
                    self.add_edge(&from, &to, EdgeKind::Enter, None, &file, line);
                    self.walk_block(&s.body, &inner, depth);
                }
                Stmt::Change(c) => {
                    self.check_change(&c.change);
                }
                Stmt::Anchor(a) => {
                    self.anchors.push(AnchorDecl {
                        node: node.node_name.clone(),
                        name: a.name.clone(),
                        note: a.note.clone(),
                        file: self.cur_file.clone(),
                        line: a.loc.line,
                    });
                }
                Stmt::Effect(_) => {} // 已在事件顶层提取;残留由解析器报错
            }
            i += 1;
        }
    }

    fn check_divert(&mut self, d: &DivertStmt, node: &NodeCtx, depth: u32) {
        let DivertTarget::Node(target) = &d.target else {
            return;
        };
        let Some(path) = self.symbols.resolve_target(
            target,
            self.symbols.event_order.get(node.event).map(String::as_str),
        ) else {
            self.diags.push(Diagnostic::error(
                "A101",
                &self.cur_file,
                Span::new(d.loc.line, d.loc.column, target.chars().count() as u32),
                format!("跃迁目标 `{target}` 不存在"),
            ));
            return;
        };
        let full = path.full_name(&self.program.events[path.event].name);
        if full == node.node_name && depth == 0 {
            self.diags.push(Diagnostic::warning(
                "A206",
                &self.cur_file,
                Span::new(d.loc.line, d.loc.column, target.chars().count() as u32),
                format!("无条件跃迁回 `{full}` 自身,会构成死循环"),
            ));
        }
        // 漂流语义:A209 同线漂流提示
        if d.drift {
            let cur_sl = self.program.events[node.event].storyline.clone();
            let tgt_sl = self.program.events[path.event].storyline.clone();
            if cur_sl == tgt_sl {
                self.diags.push(Diagnostic::warning(
                    "A209",
                    &self.cur_file,
                    Span::new(d.loc.line, d.loc.column, target.chars().count() as u32 + 3),
                    format!(
                        "漂流 `->>` 的目标 `{full}` 与当前节点在同一故事线 `{cur_sl}`;跨线移动才需要漂流,此处应使用 `->`"
                    ),
                ));
            }
        }
        let edge_kind = if d.drift {
            EdgeKind::Drift
        } else {
            EdgeKind::Divert
        };
        let label = if d.drift {
            Some("漂流".to_string())
        } else {
            None
        };
        let file = self.cur_file.clone();
        let line = d.loc.line;
        let from = node.node_name.clone();
        self.add_edge(&from, &full, edge_kind, label, &file, line);
    }
}

use super::text::{extract_localization_id, split_text_decorations};
use super::*;

impl<'a> Parser<'a> {
    /// 解析一个缩进块:所有 indent > parent_indent 的语句。
    /// 块内各行缩进必须一致;`require_body` 为真时空块报 P005。
    pub(super) fn parse_block(
        &mut self,
        parent_indent: u32,
        file: &str,
        require_body: bool,
    ) -> Vec<Stmt> {
        let Some(first) = self.peek() else {
            if require_body {
                self.diags.push(Diagnostic::error(
                    "P005",
                    file,
                    self.missing_body_span(),
                    "声明之后缺少缩进的块体",
                ));
            }
            return Vec::new();
        };
        if first.indent <= parent_indent {
            if require_body {
                self.diags.push(Diagnostic::error(
                    "P005",
                    file,
                    self.missing_body_span(),
                    "声明之后缺少缩进的块体",
                ));
            }
            return Vec::new();
        }
        let block_indent = first.indent;
        let mut stmts = Vec::new();
        while let Some(line) = self.peek() {
            if line.indent <= parent_indent {
                break;
            }
            if line.indent != block_indent {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &self.file_of(&line),
                    Span::new(line.no, line.indent + 1, 1),
                    format!("缩进不一致:同一块内应保持 {block_indent} 个空格"),
                ));
                self.next();
                continue;
            }
            let indent = line.indent;
            let stmt = self.parse_stmt(indent);
            stmts.push(stmt);
        }
        stmts
    }

    fn parse_stmt(&mut self, indent: u32) -> Stmt {
        let line = self.next().expect("parse_stmt 调用前已确认存在");
        let file = self.file_of(&line);
        let actual_file = file.clone();
        let statement = match line.kind.clone() {
            LineKind::Schema112 { loc, .. } => {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    Span::new(loc.line, loc.column, 6),
                    "schema / bind 只能出现在顶层，不能写入执行块",
                ));
                Stmt::Text(TextStmt {
                    parts: Vec::new(),
                    glue: false,
                    tags: Vec::new(),
                    localization_id: None,
                    loc,
                })
            }
            LineKind::Language111 {
                keyword,
                source,
                loc,
            } => self.parse_language_statement(&keyword, &source, loc, &file),
            LineKind::Text { content, loc } => {
                let (text_part, glue, tags) = split_text_decorations(&content);
                let options = self.options();
                let (tags, localization_id) =
                    extract_localization_id(tags, &file, loc, options.localization_ids, self.diags);
                let parts = self.sourced_text(
                    &text_part,
                    &file,
                    line.no,
                    line.source.base + line.source.text,
                    false,
                );
                Stmt::Text(TextStmt {
                    parts,
                    glue,
                    tags,
                    localization_id,
                    loc,
                })
            }
            LineKind::Divert {
                target,
                drift,
                span,
            } => {
                let t = if target == "END" {
                    DivertTarget::End
                } else {
                    DivertTarget::Node(target)
                };
                Stmt::Divert(DivertStmt {
                    target: t,
                    drift,
                    loc: Loc::new(span.line, span.column),
                })
            }
            LineKind::Choice {
                once,
                label_raw,
                cond_src,
                enable_src,
                disabled_reason,
                disabled_span: _,
                localization_id,
                loc,
                label_span: _,
            } => {
                let label = self.sourced_label(&label_raw, &line);
                let cond = cond_src.map(|src| {
                    self.sourced_expr(
                        &src,
                        &file,
                        line.no,
                        line.no,
                        line.source.base + line.source.condition,
                        ExpressionSlot::Condition(0),
                    )
                });
                let enable = enable_src.map(|src| {
                    self.sourced_expr(
                        &src,
                        &file,
                        line.no,
                        line.no,
                        line.source.base + line.source.enable,
                        ExpressionSlot::Enable,
                    )
                });
                let body = self.parse_block(indent, &file, false);
                Stmt::Choice(ChoiceStmt {
                    label,
                    label_raw,
                    once,
                    cond,
                    enable,
                    disabled_reason,
                    body,
                    loc,
                    localization_id,
                })
            }
            LineKind::If { cond_src, loc } => {
                let cond = self.sourced_expr(
                    &cond_src,
                    &file,
                    line.no,
                    line.no,
                    line.source.base + line.source.condition,
                    ExpressionSlot::Condition(0),
                );
                let body = self.parse_block(indent, &file, true);
                let mut branches = vec![(Some(cond), body)];
                // 链式 else if / else:必须与 if 同缩进
                while let Some(next_line) = self.peek() {
                    if next_line.indent != indent {
                        break;
                    }
                    match &next_line.kind {
                        LineKind::ElseIf { cond_src, .. } => {
                            let cond_src = cond_src.clone();
                            self.next();
                            let c = self.sourced_expr_from(
                                &cond_src,
                                (&file, line.no),
                                (&next_line.file, next_line.no),
                                next_line.source.base + next_line.source.condition,
                                ExpressionSlot::Condition(branches.len() as u32),
                            );
                            let b = self.parse_block(indent, &file, true);
                            branches.push((Some(c), b));
                        }
                        LineKind::Else { .. } => {
                            self.next();
                            let b = self.parse_block(indent, &file, true);
                            branches.push((None, b));
                            break; // else 必须是最后一条
                        }
                        _ => break,
                    }
                }
                Stmt::If(IfStmt { branches, loc })
            }
            LineKind::ElseIf { loc, .. } => {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    Span::new(loc.line, loc.column, 7),
                    "else if 没有对应的 if",
                ));
                Stmt::Text(TextStmt {
                    parts: vec![],
                    glue: false,
                    tags: vec![],
                    localization_id: None,
                    loc,
                })
            }
            LineKind::Else { loc } => {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    Span::new(loc.line, loc.column, 4),
                    "else 没有对应的 if",
                ));
                Stmt::Text(TextStmt {
                    parts: vec![],
                    glue: false,
                    tags: vec![],
                    localization_id: None,
                    loc,
                })
            }
            LineKind::Let {
                name,
                expr_src,
                name_span,
                ..
            } => {
                let expr = self.sourced_expr(
                    &expr_src,
                    &file,
                    line.no,
                    line.no,
                    line.source.base + line.source.value,
                    ExpressionSlot::Value,
                );
                Stmt::Let(LetStmt {
                    name,
                    expr,
                    is_const: false,
                    loc: Loc::new(line.no, name_span.column),
                    file,
                })
            }
            LineKind::Const {
                name,
                expr_src,
                name_span,
                ..
            } => {
                let expr = self.sourced_expr(
                    &expr_src,
                    &file,
                    line.no,
                    line.no,
                    line.source.base + line.source.value,
                    ExpressionSlot::Value,
                );
                Stmt::Let(LetStmt {
                    name,
                    expr,
                    is_const: true,
                    loc: Loc::new(line.no, name_span.column),
                    file,
                })
            }
            LineKind::Set {
                name,
                expr_src,
                name_span,
                ..
            } => {
                let expr = self.sourced_expr(
                    &expr_src,
                    &file,
                    line.no,
                    line.no,
                    line.source.base + line.source.value,
                    ExpressionSlot::Value,
                );
                Stmt::Set(SetStmt {
                    name,
                    expr,
                    loc: Loc::new(line.no, name_span.column),
                })
            }
            LineKind::Scene { name, loc } => {
                let body = self.parse_block(indent, &file, true);
                Stmt::Scene(SceneStmt {
                    name,
                    body,
                    loc: Loc::new(loc.line, loc.column),
                })
            }
            LineKind::Event { loc: espan, .. } => {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    espan,
                    "event 只能出现在顶层(或用 include 合并文件)",
                ));
                Stmt::Text(TextStmt {
                    parts: vec![],
                    glue: false,
                    tags: vec![],
                    localization_id: None,
                    loc: Loc::new(espan.line, espan.column),
                })
            }
            LineKind::Include { span, .. } => {
                self.diags.push(Diagnostic::error(
                    "P007",
                    &file,
                    span,
                    "include 只能出现在顶层",
                ));
                Stmt::Text(TextStmt {
                    parts: vec![],
                    glue: false,
                    tags: vec![],
                    localization_id: None,
                    loc: Loc::new(span.line, span.column),
                })
            }
            LineKind::Become(change) => Stmt::Change(ChangeStmt { change }),
            LineKind::ChangeLine {
                kind,
                id,
                note,
                loc,
            } => Stmt::Change(ChangeStmt {
                change: Change {
                    tags: Vec::new(),
                    kind,
                    id,
                    note,
                    to_storyline: None,
                    loc,
                },
            }),
            LineKind::ToLine { loc, .. } => {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    Span::new(loc.line, loc.column, 2),
                    "`to`(主线变动)只能写在效果块内;改变执行流请用 `->>`",
                ));
                Stmt::Text(TextStmt {
                    parts: vec![],
                    glue: false,
                    tags: vec![],
                    localization_id: None,
                    loc,
                })
            }
            LineKind::Anchor { name, note, loc } => Stmt::Anchor(AnchorStmt { name, note, loc }),
            LineKind::Storyline { loc, .. } => {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    loc,
                    "storyline 只能出现在顶层(嵌套 storyline 不允许)",
                ));
                Stmt::Text(TextStmt {
                    parts: vec![],
                    glue: false,
                    tags: vec![],
                    localization_id: None,
                    loc: Loc::new(loc.line, loc.column),
                })
            }
            LineKind::Character { loc, .. }
            | LineKind::Entity { loc, .. }
            | LineKind::RelationType { loc, .. }
            | LineKind::RelationDef { loc, .. }
            | LineKind::World { loc, .. }
            | LineKind::Period { loc, .. } => {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    loc,
                    "character / entity / relation_type / relation_def 只能出现在顶层,不能写入事件或故事线块内",
                ));
                Stmt::Text(TextStmt {
                    parts: vec![],
                    glue: false,
                    tags: vec![],
                    localization_id: None,
                    loc: Loc::new(loc.line, loc.column),
                })
            }
            LineKind::RelationField { loc, .. } => {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    Span::new(loc.line, loc.column, 1),
                    "关系字段只能出现在 relation_type 或 relation_def 块内",
                ));
                Stmt::Text(TextStmt {
                    parts: vec![],
                    glue: false,
                    tags: vec![],
                    localization_id: None,
                    loc,
                })
            }
            LineKind::Property { loc, .. }
            | LineKind::Description { loc, .. }
            | LineKind::Relation { loc, .. } => {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    Span::new(loc.line, loc.column, 1),
                    "人物属性、关系与世界描述必须写在对应声明的块内",
                ));
                Stmt::Text(TextStmt {
                    parts: vec![],
                    glue: false,
                    tags: vec![],
                    localization_id: None,
                    loc,
                })
            }
            LineKind::Catalog(_) => {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    Span::new(line.no, 1, 4),
                    "tag / asset / mark / attach 只能出现在顶层",
                ));
                Stmt::Text(TextStmt {
                    parts: Vec::new(),
                    glue: false,
                    tags: Vec::new(),
                    localization_id: None,
                    loc: Loc::new(line.no, 1),
                })
            }
            LineKind::Effect {
                when_src,
                cond_src,
                loc,
            } => {
                let when = match when_src.as_str() {
                    "done" => EffectWhen::Done,
                    "exit" => EffectWhen::Exit,
                    _ => EffectWhen::Enter,
                };
                let cond = cond_src.map(|src| {
                    self.sourced_expr(
                        &src,
                        &file,
                        line.no,
                        line.no,
                        line.source.base + line.source.condition,
                        ExpressionSlot::Condition(0),
                    )
                });
                let actions = self.parse_effect_actions(indent, &file);
                Stmt::Effect(EffectBlock {
                    when,
                    cond,
                    actions,
                    loc,
                })
            }
        };
        if let Some(owner) = &self.source_owner {
            self.sources.record_statement(
                owner,
                crate::language::statement_loc(&statement),
                crate::source_provenance::StatementKind::of(&statement),
                &actual_file,
            );
        }
        statement
    }
}

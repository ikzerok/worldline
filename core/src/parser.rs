//! 缩进驱动的递归下降语法分析 —— 规范见 `worldline/spec/syntax.md`。

use crate::ast::*;
use crate::diagnostic::{Diagnostic, Span};
use crate::expression::{parse_expr_src, parse_interpolations};
use crate::lexer::{Line, LineKind};

/// 从事件体顶层提取效果块;其余位置出现的 effect 报 P002。
fn extract_effects(
    body: Vec<Stmt>,
    file: &str,
    diags: &mut Vec<Diagnostic>,
) -> (Vec<EffectBlock>, Vec<Stmt>) {
    let mut effects = Vec::new();
    let mut rest = Vec::new();
    for s in body {
        match s {
            Stmt::Effect(e) => effects.push(e),
            other => {
                check_no_effect_nested(&other, file, diags);
                rest.push(other);
            }
        }
    }
    (effects, rest)
}

fn check_no_effect_nested(s: &Stmt, file: &str, diags: &mut Vec<Diagnostic>) {
    match s {
        Stmt::Effect(e) => diags.push(Diagnostic::error(
            "P002",
            file,
            Span::new(e.loc.line, e.loc.column, 6),
            "effect 只能写在事件体的顶层(选择 / 条件 / 场景块内不允许)",
        )),
        Stmt::Choice(c) => {
            for x in &c.body {
                check_no_effect_nested(x, file, diags);
            }
        }
        Stmt::If(i) => {
            for (_, b) in &i.branches {
                for x in b {
                    check_no_effect_nested(x, file, diags);
                }
            }
        }
        Stmt::Scene(sc) => {
            for x in &sc.body {
                check_no_effect_nested(x, file, diags);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// 语句与块
// ---------------------------------------------------------------------------

pub struct Parser<'a> {
    lines: &'a [Line],
    pos: usize,
    diags: &'a mut Vec<Diagnostic>,
    /// 当前 storyline 块归属(块外为 None → main)。
    cur_storyline: Option<String>,
}

impl<'a> Parser<'a> {
    pub fn new(lines: &'a [Line], diags: &'a mut Vec<Diagnostic>) -> Self {
        Parser {
            lines,
            pos: 0,
            diags,
            cur_storyline: None,
        }
    }

    fn peek(&self) -> Option<&'a Line> {
        self.lines.get(self.pos)
    }

    fn next(&mut self) -> Option<&'a Line> {
        let l = self.lines.get(self.pos);
        if l.is_some() {
            self.pos += 1;
        }
        l
    }

    fn file_of(&self, line: &Line) -> String {
        line.file.clone()
    }

    /// 顶层解析。include 已在驱动层展开;入口 = 主文件第一个事件。
    pub fn parse_program(&mut self) -> Program {
        let mut program = Program::default();
        let main_file = self
            .lines
            .first()
            .map(|l| l.file.clone())
            .unwrap_or_default();
        while let Some(line) = self.peek() {
            let file = self.file_of(line);
            if line.indent != 0 {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    Span::new(line.no, 1, 1),
                    "顶层声明不能缩进",
                ));
            }
            match &line.kind {
                LineKind::Catalog(item) => {
                    let mut item = item.clone();
                    self.next();
                    if let crate::catalog::CatalogDecl::Tag(tag) = &mut item {
                        let (properties, _, description) =
                            self.parse_metadata(line.indent, &file, true);
                        tag.properties = properties;
                        tag.description = description;
                    }
                    if let crate::catalog::CatalogDecl::Anchor(anchor) = &mut item {
                        let (properties, _, description) =
                            self.parse_metadata(line.indent, &file, true);
                        for property in properties {
                            self.diags.push(Diagnostic::error(
                                "P002",
                                &file,
                                Span::new(property.loc.line, 1, 8),
                                "独立锚点块内只允许一个 description",
                            ));
                        }
                        anchor.description = description;
                    }
                    program.catalog.push(item);
                }
                LineKind::Let {
                    name,
                    expr_src,
                    loc,
                    name_span,
                } => {
                    self.next();
                    let expr = parse_expr_src(
                        expr_src,
                        &file,
                        line.no,
                        name_span.column + name.chars().count() as u32 + 1,
                        self.diags,
                    );
                    program.lets.push(LetStmt {
                        name: name.clone(),
                        expr,
                        is_const: false,
                        loc: *loc,
                        file: file.clone(),
                    });
                }
                LineKind::Const {
                    name,
                    expr_src,
                    loc,
                    name_span,
                } => {
                    self.next();
                    let expr = parse_expr_src(
                        expr_src,
                        &file,
                        line.no,
                        name_span.column + name.chars().count() as u32 + 1,
                        self.diags,
                    );
                    program.lets.push(LetStmt {
                        name: name.clone(),
                        expr,
                        is_const: true,
                        loc: *loc,
                        file: file.clone(),
                    });
                }
                LineKind::Storyline { name, display, loc } => {
                    let (name, display) = (name.clone(), display.clone());
                    let espan = *loc;
                    let s_indent = line.indent;
                    self.next();
                    let mut nested = false;
                    if !name.is_empty() {
                        program.storylines.push(StorylineDecl {
                            name: name.clone(),
                            display,
                            loc: Loc::new(espan.line, espan.column),
                        });
                    }
                    let prev = self.cur_storyline.replace(if name.is_empty() {
                        "main".into()
                    } else {
                        name
                    });
                    if prev.is_some() {
                        nested = true;
                    }
                    if nested {
                        self.diags.push(Diagnostic::error(
                            "P002",
                            &file,
                            espan,
                            "storyline 不能嵌套在另一个 storyline 块内",
                        ));
                    }
                    while let Some(nl) = self.peek().cloned() {
                        if nl.indent <= s_indent {
                            break;
                        }
                        match &nl.kind {
                            LineKind::Event { .. } => {
                                self.parse_event_decl(&mut program, &main_file)
                            }
                            LineKind::Let { .. } | LineKind::Const { .. } => {
                                self.parse_global_decl(&mut program);
                            }
                            LineKind::Character { loc: cloc, .. } => {
                                self.diags.push(Diagnostic::error(
                                    "P002",
                                    &file,
                                    *cloc,
                                    "character 只能出现在顶层,不能写入 storyline 块",
                                ));
                                self.next();
                            }
                            LineKind::Storyline { loc: sloc, .. } => {
                                self.diags.push(Diagnostic::error(
                                    "P002",
                                    &file,
                                    *sloc,
                                    "storyline 不能嵌套在另一个 storyline 块内",
                                ));
                                self.next();
                            }
                            _ => {
                                self.diags.push(Diagnostic::error(
                                    "P002",
                                    &file,
                                    Span::new(nl.no, nl.indent + 1, 4),
                                    "storyline 块内只能出现 event(或 let / const)",
                                ));
                                self.next();
                            }
                        }
                    }
                    self.cur_storyline = prev;
                }
                LineKind::Period {
                    name,
                    display,
                    parent,
                    loc,
                } => {
                    program.periods.push(PeriodDecl {
                        parent: parent.clone(),
                        name: name.clone(),
                        display: display.clone(),
                        file,
                        loc: Loc::new(loc.line, loc.column),
                    });
                    self.next();
                }
                LineKind::World { name, display, loc } => {
                    let (name, display, loc, indent) =
                        (name.clone(), display.clone(), *loc, line.indent);
                    self.next();
                    let (properties, _, description) = self.parse_metadata(indent, &file, true);
                    program.worlds.push(WorldDecl {
                        name,
                        display,
                        description,
                        properties,
                        file,
                        loc: Loc::new(loc.line, loc.column),
                    });
                }
                LineKind::Character { name, display, loc } => {
                    let (name, display) = (name.clone(), display.clone());
                    let espan = *loc;
                    self.next();
                    let (properties, relations, _) = self.parse_metadata(line.indent, &file, false);
                    program.characters.push(CharacterDecl {
                        name,
                        display,
                        loc: Loc::new(espan.line, espan.column),
                        file: file.clone(),
                        properties,
                        relations,
                    });
                }
                LineKind::Event { .. } => self.parse_event_decl(&mut program, &main_file),
                LineKind::Scene { loc, .. } => {
                    self.diags.push(Diagnostic::error(
                        "P005",
                        &file,
                        *loc,
                        "scene 必须声明在事件(或场景)块内部",
                    ));
                    self.next();
                }
                LineKind::Else { loc } | LineKind::ElseIf { loc, .. } => {
                    self.diags.push(Diagnostic::error(
                        "P002",
                        &file,
                        Span::new(loc.line, loc.column, 4),
                        "else 不能出现在事件块外",
                    ));
                    self.next();
                }
                _ => {
                    self.diags.push(Diagnostic::error(
                        "P002",
                        &file,
                        Span::new(line.no, line.indent + 1, 5),
                        "顶层只能是 event / storyline / character / let / const / include;文本与选择必须写在事件块内",
                    ));
                    self.next();
                }
            }
        }
        program
    }

    fn parse_metadata(
        &mut self,
        indent: u32,
        file: &str,
        world: bool,
    ) -> (Vec<Property>, Vec<CharacterRelation>, String) {
        let mut properties = Vec::new();
        let mut relations = Vec::new();
        let mut description = String::new();
        let mut has_description = false;
        let mut block_indent = None;
        while let Some(line) = self.peek().cloned() {
            if line.indent <= indent || line.file != file {
                break;
            }
            self.next();
            if *block_indent.get_or_insert(line.indent) != line.indent {
                self.diags.push(Diagnostic::error(
                    "P002",
                    file,
                    Span::new(line.no, 1, 1),
                    "属性块内缩进必须一致",
                ));
            }
            match line.kind {
                LineKind::Property {
                    name,
                    value_src,
                    loc,
                } => {
                    let expr = parse_expr_src(&value_src, file, line.no, 1, self.diags);
                    let value = match expr {
                        Expr::Str(s) => Some(PropertyValue::Str(s)),
                        Expr::Bool(b) => Some(PropertyValue::Bool(b)),
                        Expr::Num(n) if n.is_finite() => Some(PropertyValue::Num(n)),
                        Expr::Unary {
                            op: UnOp::Neg,
                            expr,
                        } => match *expr {
                            Expr::Num(n) if n.is_finite() => Some(PropertyValue::Num(-n)),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(value) = value {
                        properties.push(Property { name, value, loc });
                    } else {
                        self.diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(line.no, 1, 8),
                            "属性值只能是字符串、有限数值或布尔字面量",
                        ));
                    }
                }
                LineKind::Relation { target, label, loc } if !world => {
                    relations.push(CharacterRelation { target, label, loc })
                }
                LineKind::Description { text, loc } if world && !has_description => {
                    let _ = loc;
                    description = text;
                    has_description = true;
                }
                _ => self.diags.push(Diagnostic::error(
                    "P002",
                    file,
                    Span::new(line.no, 1, 1),
                    if world {
                        "世界观块内只允许一个 description 和 property"
                    } else {
                        "角色块内只允许 property 和 relation"
                    },
                )),
            }
        }
        (properties, relations, description)
    }

    /// 解析顶层/故事线块内的 let/const(下一行)。
    fn parse_global_decl(&mut self, program: &mut Program) {
        let Some(line) = self.next() else { return };
        let file = line.file.clone();
        match &line.kind {
            LineKind::Let {
                name,
                expr_src,
                loc,
                name_span,
            } => {
                let expr = parse_expr_src(
                    expr_src,
                    &file,
                    line.no,
                    name_span.column + name.chars().count() as u32 + 1,
                    self.diags,
                );
                program.lets.push(LetStmt {
                    name: name.clone(),
                    expr,
                    is_const: false,
                    loc: *loc,
                    file,
                });
            }
            LineKind::Const {
                name,
                expr_src,
                loc,
                name_span,
            } => {
                let expr = parse_expr_src(
                    expr_src,
                    &file,
                    line.no,
                    name_span.column + name.chars().count() as u32 + 1,
                    self.diags,
                );
                program.lets.push(LetStmt {
                    name: name.clone(),
                    expr,
                    is_const: true,
                    loc: *loc,
                    file,
                });
            }
            _ => {}
        }
    }

    /// 解析一个事件声明(下一行为 event 头)。
    fn parse_event_decl(&mut self, program: &mut Program, main_file: &str) {
        let Some(line) = self.next() else { return };
        let file = line.file.clone();
        let (
            name,
            summary,
            order,
            period,
            predecessors,
            characters,
            perm,
            after_src,
            espan,
            indent,
        ) = match &line.kind {
            LineKind::Event {
                name,
                summary,
                order,
                period,
                predecessors,
                characters,
                perm,
                after_src,
                loc,
            } => (
                name.clone(),
                summary.clone(),
                *order,
                period.clone(),
                predecessors.clone(),
                characters.clone(),
                perm.clone(),
                after_src.clone(),
                *loc,
                line.indent,
            ),
            _ => return,
        };
        let after = after_src.map(|src| {
            parse_expr_src(
                &src,
                &file,
                line.no,
                espan.column + name.chars().count() as u32 + 1,
                self.diags,
            )
        });
        let body = self.parse_block(indent, &file, true);
        let (effects, body) = extract_effects(body, &file, self.diags);
        if program.entry.is_empty() && file == main_file {
            program.entry = name.clone();
        }
        let storyline = self
            .cur_storyline
            .clone()
            .unwrap_or_else(|| "main".to_string());
        program.events.push(Event {
            name,
            body,
            loc: Loc::new(espan.line, espan.column),
            summary,
            order,
            period,
            predecessors,
            characters,
            perm,
            after,
            storyline,
            effects,
        });
        program.event_files.push(file);
    }

    /// 解析一个缩进块:所有 indent > parent_indent 的语句。
    /// 块内各行缩进必须一致;`require_body` 为真时空块报 P005。
    fn parse_block(&mut self, parent_indent: u32, file: &str, require_body: bool) -> Vec<Stmt> {
        let Some(first) = self.peek() else {
            if require_body {
                self.diags.push(Diagnostic::error(
                    "P005",
                    file,
                    Span::new(0, 1, 1),
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
                    Span::new(first.no, first.indent + 1, 1),
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
                    &self.file_of(line),
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
        let file = self.file_of(line);
        match line.kind.clone() {
            LineKind::Text { content, loc } => {
                let (text_part, glue, tags) = split_text_decorations(&content);
                let parts =
                    parse_interpolations(&text_part, &file, line.no, indent + 1, self.diags);
                Stmt::Text(TextStmt {
                    parts,
                    glue,
                    tags,
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
                loc,
                label_span,
            } => {
                let label = parse_interpolations(
                    &label_raw,
                    &file,
                    line.no,
                    indent + label_span.column,
                    self.diags,
                );
                let cond = cond_src.map(|src| {
                    parse_expr_src(
                        &src,
                        &file,
                        line.no,
                        label_span.column + label_raw.chars().count() as u32 + 4,
                        self.diags,
                    )
                });
                let body = self.parse_block(indent, &file, false);
                Stmt::Choice(ChoiceStmt {
                    label,
                    label_raw,
                    once,
                    cond,
                    body,
                    loc,
                })
            }
            LineKind::If { cond_src, loc } => {
                let cond = parse_expr_src(&cond_src, &file, line.no, loc.column + 2, self.diags);
                let body = self.parse_block(indent, &file, true);
                let mut branches = vec![(Some(cond), body)];
                // 链式 else if / else:必须与 if 同缩进
                while let Some(next_line) = self.peek().cloned() {
                    if next_line.indent != indent {
                        break;
                    }
                    match &next_line.kind {
                        LineKind::ElseIf { cond_src, .. } => {
                            let cond_src = cond_src.clone();
                            self.next();
                            let c = parse_expr_src(
                                &cond_src,
                                &file,
                                next_line.no,
                                loc.column + 6,
                                self.diags,
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
                    loc,
                })
            }
            LineKind::Let {
                name,
                expr_src,
                name_span,
                ..
            } => {
                let expr = parse_expr_src(
                    &expr_src,
                    &file,
                    line.no,
                    name_span.column + name.chars().count() as u32 + 1,
                    self.diags,
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
                let expr = parse_expr_src(
                    &expr_src,
                    &file,
                    line.no,
                    name_span.column + name.chars().count() as u32 + 1,
                    self.diags,
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
                let expr = parse_expr_src(
                    &expr_src,
                    &file,
                    line.no,
                    name_span.column + name.chars().count() as u32 + 1,
                    self.diags,
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
                    loc: Loc::new(loc.line, loc.column),
                })
            }
            LineKind::Character { loc, .. }
            | LineKind::World { loc, .. }
            | LineKind::Period { loc, .. } => {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    loc,
                    "character 只能出现在顶层,不能写入事件或故事线块内",
                ));
                Stmt::Text(TextStmt {
                    parts: vec![],
                    glue: false,
                    tags: vec![],
                    loc: Loc::new(loc.line, loc.column),
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
                let cond = cond_src
                    .map(|src| parse_expr_src(&src, &file, line.no, loc.column + 6, self.diags));
                let actions = self.parse_effect_actions(indent, &file);
                Stmt::Effect(EffectBlock {
                    when,
                    cond,
                    actions,
                    loc,
                })
            }
        }
    }

    /// 解析效果块体:仅允许 grant / revoke / meet / part / to 动作行。
    fn parse_effect_actions(&mut self, parent_indent: u32, file: &str) -> Vec<Change> {
        let Some(first) = self.peek() else {
            self.diags.push(Diagnostic::error(
                "P005",
                file,
                Span::new(0, 1, 1),
                "effect 之后缺少缩进的动作块",
            ));
            return Vec::new();
        };
        if first.indent <= parent_indent {
            self.diags.push(Diagnostic::error(
                "P005",
                file,
                Span::new(first.no, first.indent + 1, 1),
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
                    &self.file_of(line),
                    Span::new(line.no, line.indent + 1, 1),
                    format!("缩进不一致:同一效果块内应保持 {block_indent} 个空格"),
                ));
                self.next();
                continue;
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

/// 从原始文本行提取 (正文, 是否粘接~, 标签列表)。
/// 顺序:先剥行尾标签,再判粘接;正文中的转义交给 parse_interpolations。
fn split_text_decorations(raw: &str) -> (String, bool, Vec<String>) {
    let chars: Vec<char> = raw.chars().collect();
    // 1. 标签:未转义的 `#` 且前一字符是空白或行首
    let mut text_end = chars.len();
    let mut tags = Vec::new();
    let mut i = 0;
    let mut in_brace = 0u8; // 插值内的 # 不算标签
    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' if i == 0 || chars[i - 1] != '\\' => in_brace = in_brace.saturating_add(1),
            '}' if i == 0 || chars[i - 1] != '\\' => in_brace = in_brace.saturating_sub(1),
            '#' if in_brace == 0
                && (i == 0 || chars[i - 1].is_whitespace())
                && i + 1 < chars.len()
                && !chars[i + 1].is_whitespace() =>
            {
                if i < text_end {
                    text_end = i;
                }
                // 收集标签到空白或行尾
                let mut j = i + 1;
                let mut tag = String::new();
                while j < chars.len() && !chars[j].is_whitespace() {
                    tag.push(chars[j]);
                    j += 1;
                }
                tags.push(tag);
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    let mut text: String = chars[..text_end].iter().collect();
    // 2. 粘接:行尾(去尾随空格后)未转义的 `~`
    let trimmed = text.trim_end().to_string();
    let tchars: Vec<char> = trimmed.chars().collect();
    let mut glue = false;
    if let Some(&last) = tchars.last() {
        if last == '~' && (tchars.len() < 2 || tchars[tchars.len() - 2] != '\\') {
            glue = true;
            text = tchars[..tchars.len() - 1].iter().collect();
            text = text.trim_end().to_string();
        } else {
            text = trimmed;
        }
    }
    (text, glue, tags)
}

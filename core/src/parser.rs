//! 缩进驱动的递归下降语法分析 —— 规范见 `worldline/spec/syntax.md`。

use crate::ast::*;
use crate::diagnostic::{Diagnostic, Span};
use crate::expression::{parse_expr_src, parse_interpolations_with_options};
use crate::lexer::{Line, LineKind};
mod metadata;
mod program;
mod statements;
mod text;

fn relation_target(file: &str, kind: &str, id: &str) -> crate::catalog::TargetRef {
    if kind == "file" {
        let resolved = crate::catalog::resolved_asset(file, id);
        crate::catalog::TargetRef::new(kind, &resolved.to_string_lossy())
    } else {
        crate::catalog::TargetRef::new(kind, id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetadataKind {
    World,
    Entity,
    Character,
}

fn parse_property_value(
    expression: Expr,
    file: &str,
    options: crate::compiler::CompileOptions,
) -> Result<PropertyValue, &'static str> {
    match expression {
        Expr::Str(value) => Ok(PropertyValue::Str(value)),
        Expr::Bool(value) => Ok(PropertyValue::Bool(value)),
        Expr::Num(value) if value.is_finite() => Ok(PropertyValue::Num(value)),
        Expr::Unary {
            op: UnOp::Neg,
            expr,
        } => match *expr {
            Expr::Num(value) if value.is_finite() => Ok(PropertyValue::Num(-value)),
            _ => Err("属性数值必须是有限数字字面量"),
        },
        Expr::Call { name, args, .. } if name == "ref" => {
            if !options.language_version.supports_entities() || !options.object_refs {
                return Err("ref 属性值需要语言 1.10 与清单能力 content.object_refs.v1");
            }
            let [Expr::Str(kind), Expr::Str(id)] = args.as_slice() else {
                return Err("ref 属性值格式为 ref(\"kind\", \"id\")，只接受两个字符串字面量");
            };
            if id.trim().is_empty()
                || !crate::catalog::OBJECT_REFERENCE_TARGET_KINDS.contains(&kind.as_str())
                || !crate::catalog::is_target_kind(kind, options)
            {
                return Err("ref 属性值的目标类型或 ID 无效");
            }
            Ok(PropertyValue::Ref(relation_target(file, kind, id)))
        }
        _ => Err("属性值只能是字符串、有限数值、布尔字面量或显式 ref(\"kind\", \"id\")"),
    }
}

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
    allow_entities: bool,
    allow_object_refs: bool,
    allow_localization_ids: bool,
}

impl<'a> Parser<'a> {
    pub fn new(lines: &'a [Line], diags: &'a mut Vec<Diagnostic>) -> Self {
        Self::new_with_options(lines, diags, crate::compiler::CompileOptions::default())
    }

    pub fn new_with_options(
        lines: &'a [Line],
        diags: &'a mut Vec<Diagnostic>,
        options: crate::compiler::CompileOptions,
    ) -> Self {
        Parser {
            lines,
            pos: 0,
            diags,
            cur_storyline: None,
            allow_entities: options.language_version.supports_entities(),
            allow_object_refs: options.object_refs,
            allow_localization_ids: options.localization_ids,
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

    fn options(&self) -> crate::compiler::CompileOptions {
        crate::compiler::CompileOptions::new(if self.allow_entities {
            crate::compiler::LanguageVersion::V1_10
        } else {
            crate::compiler::LanguageVersion::V1_9
        })
        .with_object_refs(self.allow_object_refs)
        .with_localization_ids(self.allow_localization_ids)
    }
}

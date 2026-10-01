//! 显式1.11共享语言模型；语法真源为spec/language-1.11.md。
use crate::ast::{ChangeKind, Expr, Loc, Program, Stmt, TextStmt, ValueKind};

#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub kind: ValueKind,
    pub loc: Loc,
}
#[derive(Debug, Clone)]
pub struct RuleDecl {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub result: ValueKind,
    pub expr: Expr,
    pub file: String,
    pub loc: Loc,
}
#[derive(Debug, Clone)]
pub struct FragmentDecl {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub body: Vec<Stmt>,
    pub file: String,
    pub loc: Loc,
}
#[derive(Debug, Clone)]
pub struct LocalStmt {
    pub name: String,
    pub kind: ValueKind,
    pub expr: Expr,
    pub loc: Loc,
}
#[derive(Debug, Clone)]
pub struct CallStmt {
    pub name: String,
    pub args: Vec<Expr>,
    pub loc: Loc,
}
#[derive(Debug, Clone)]
pub struct SayStmt {
    pub speaker: String,
    pub text: TextStmt,
    pub direction: Option<String>,
    pub loc: Loc,
}
#[derive(Debug, Clone)]
pub struct DynamicChangeStmt {
    pub state: Expr,
    pub tags: Expr,
    pub kind: ChangeKind,
    pub loc: Loc,
}

pub fn value_kind(name: &str) -> Option<ValueKind> {
    Some(match name.trim() {
        "num" => ValueKind::Num,
        "str" => ValueKind::Str,
        "bool" => ValueKind::Bool,
        "tag" => ValueKind::Tag,
        "tagset" => ValueKind::TagSet,
        "state" => ValueKind::StateRef,
        _ => return None,
    })
}
pub fn is_new_builtin(name: &str) -> bool {
    matches!(
        name,
        "when"
            | "tag"
            | "state"
            | "tags"
            | "members"
            | "count"
            | "contains"
            | "union"
            | "intersect"
            | "difference"
    )
}
pub fn static_id(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Var { name, .. } | Expr::Str(name) => Some(name),
        _ => None,
    }
}
pub fn locals(stmts: &[Stmt]) -> Vec<&LocalStmt> {
    let mut out = Vec::new();
    for stmt in stmts {
        match stmt {
            Stmt::Local(l) => out.push(l),
            Stmt::Choice(c) => out.extend(locals(&c.body)),
            Stmt::If(i) => {
                for (_, b) in &i.branches {
                    out.extend(locals(b));
                }
            }
            Stmt::Scene(s) => out.extend(locals(&s.body)),
            _ => {}
        }
    }
    out
}
pub fn uses_new_features(program: &Program) -> bool {
    if !program.rules.is_empty() || !program.fragments.is_empty() {
        return true;
    }
    fn expr(e: &Expr) -> bool {
        match e {
            Expr::Call { name, args, .. } => is_new_builtin(name) || args.iter().any(expr),
            Expr::Unary { expr: e, .. } => expr(e),
            Expr::Binary { lhs, rhs, .. } => expr(lhs) || expr(rhs),
            _ => false,
        }
    }
    fn parts(ps: &[crate::ast::TextPart]) -> bool {
        ps.iter()
            .any(|p| matches!(p, crate::ast::TextPart::Expr(e) if expr(e)))
    }
    fn body(ss: &[Stmt]) -> bool {
        ss.iter().any(|s| match s {
            Stmt::Call(_)
            | Stmt::Local(_)
            | Stmt::Return(_)
            | Stmt::Say(_)
            | Stmt::DynamicChange(_) => true,
            Stmt::Text(t) => parts(&t.parts),
            Stmt::Let(l) => expr(&l.expr),
            Stmt::Set(s) => expr(&s.expr),
            Stmt::Choice(c) => {
                parts(&c.label)
                    || c.cond.as_ref().is_some_and(expr)
                    || c.enable.as_ref().is_some_and(expr)
                    || body(&c.body)
            }
            Stmt::If(i) => i
                .branches
                .iter()
                .any(|(c, b)| c.as_ref().is_some_and(expr) || body(b)),
            Stmt::Scene(s) => body(&s.body),
            _ => false,
        })
    }
    program.lets.iter().any(|l| expr(&l.expr))
        || program.events.iter().any(|e| {
            body(&e.body)
                || e.after.as_ref().is_some_and(expr)
                || e.effects.iter().any(|f| f.cond.as_ref().is_some_and(expr))
        })
}
/// 动态变更分界只识别字符串及括号外的显式操作，不误读旧动作备注。
pub fn dynamic_change_parts(source: &str) -> Option<(&str, &str, ChangeKind)> {
    for (needle, kind) in [
        (" with from ", ChangeKind::Become),
        (" add from ", ChangeKind::AddTags),
        (" remove from ", ChangeKind::RemoveTags),
    ] {
        let mut quoted = false;
        let mut escaped = false;
        let mut depth = 0;
        for (i, c) in source.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if c == '\\' {
                escaped = true;
                continue;
            }
            if c == '"' {
                quoted = !quoted;
            }
            if !quoted {
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                }
                if depth == 0 && source[i..].starts_with(needle) {
                    return Some((&source[..i], &source[i + needle.len()..], kind));
                }
            }
        }
    }
    None
}
/// 不依赖运行时的源码定位，作者工具与诊断共享。
pub fn statement_loc(stmt: &Stmt) -> Loc {
    match stmt {
        Stmt::Text(s) => s.loc,
        Stmt::Say(s) => s.loc,
        Stmt::Choice(s) => s.loc,
        Stmt::If(s) => s.loc,
        Stmt::Divert(s) => s.loc,
        Stmt::Let(s) => s.loc,
        Stmt::Set(s) => s.loc,
        Stmt::Scene(s) => s.loc,
        Stmt::Change(s) => s.change.loc,
        Stmt::Anchor(s) => s.loc,
        Stmt::Effect(s) => s.loc,
        Stmt::Local(s) => s.loc,
        Stmt::Call(s) => s.loc,
        Stmt::Return(loc) => *loc,
        Stmt::DynamicChange(s) => s.loc,
    }
}

//! 旧权限只作为输入：统一成世界的叙事身份状态，并可无损改写工程缓冲。
use crate::ast::*;
use crate::catalog::CatalogDecl;
use crate::lexer::{self, LineKind};
use crate::{fingerprint_program, Program};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
mod rewrite;
pub(crate) use rewrite::rewrite_sources;

const HEADER: &str = "// worldline-permissions-v1 ";
const TAG: &str = "// worldline-permission-tag ";
const GATE: &str = "// worldline-permission-gate ";

/// 只含编译兼容信息；权限的唯一运行真相在对应状态中。
#[derive(Debug, Clone)]
pub struct PermissionMigration {
    pub state: String,
    pub tags: BTreeMap<String, String>,
    /// 旧事件闸门先于 after 求值，值为身份标签 ID。
    pub gates: BTreeMap<String, String>,
    pub legacy_fingerprint: u64,
    pub normalized_fingerprint: u64,
    /// 本轮编译为旧输入补建的声明，工程迁移时写入入口缓冲。
    pub(crate) declarations: String,
}
impl PermissionMigration {
    pub fn accepts_legacy(&self, fingerprint: u64, current: u64) -> bool {
        fingerprint == self.legacy_fingerprint && current == self.normalized_fingerprint
    }
    pub fn permission(&self, tag: &str) -> Option<&str> {
        self.tags
            .iter()
            .find_map(|(p, t)| (t == tag).then_some(p.as_str()))
    }
    fn predicate(&self, permission: &str, loc: Loc) -> Expr {
        Expr::Call {
            name: "has".into(),
            args: vec![
                Expr::Var {
                    name: self.state.clone(),
                    loc,
                },
                Expr::Var {
                    name: self.tags[permission].clone(),
                    loc,
                },
            ],
            loc,
        }
    }
    fn metadata(&self) -> String {
        let mut out = format!(
            "{HEADER}{} {} {}\n",
            self.state, self.legacy_fingerprint, self.normalized_fingerprint
        );
        for (permission, tag) in &self.tags {
            out.push_str(&format!("{TAG}{tag} {}\n", hex(permission)));
        }
        for (event, tag) in &self.gates {
            out.push_str(&format!("{GATE}{event} {tag}\n"));
        }
        out
    }
}

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> Option<String> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    String::from_utf8(
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
            .collect::<Result<Vec<_>, _>>()
            .ok()?,
    )
    .ok()
}

fn read_metadata(sources: &BTreeMap<PathBuf, String>) -> Option<PermissionMigration> {
    for text in sources.values() {
        let Some(header) = text.lines().find_map(|l| l.strip_prefix(HEADER)) else {
            continue;
        };
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 3 {
            continue;
        }
        let mut tags = BTreeMap::new();
        for line in text.lines().filter_map(|l| l.strip_prefix(TAG)) {
            let (tag, encoded) = line.split_once(' ')?;
            crate::authoring::identifier(tag).ok()?;
            tags.insert(unhex(encoded)?, tag.into());
        }
        let mut gates = BTreeMap::new();
        for line in text.lines().filter_map(|l| l.strip_prefix(GATE)) {
            let (event, tag) = line.split_once(' ')?;
            crate::authoring::identifier(event).ok()?;
            if !tags.values().any(|t| t == tag) {
                return None;
            }
            gates.insert(event.into(), tag.into());
        }
        crate::authoring::identifier(fields[0]).ok()?;
        return Some(PermissionMigration {
            state: fields[0].into(),
            tags,
            gates,
            legacy_fingerprint: fields[1].parse().ok()?,
            normalized_fingerprint: fields[2].parse().ok()?,
            declarations: String::new(),
        });
    }
    None
}

fn permission(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Call { name, args, .. } if name == "perm" && args.len() == 1 => match &args[0] {
            Expr::Str(p) | Expr::Var { name: p, .. } => Some(p),
            _ => None,
        },
        _ => None,
    }
}

fn visit_expr(expr: &mut Expr, f: &mut impl FnMut(&mut Expr)) {
    match expr {
        Expr::Unary { expr, .. } => visit_expr(expr, f),
        Expr::Binary { lhs, rhs, .. } => {
            visit_expr(lhs, f);
            visit_expr(rhs, f);
        }
        Expr::Call { args, .. } => {
            for a in args {
                visit_expr(a, f);
            }
        }
        _ => {}
    }
    f(expr);
}
fn visit_effect(
    effect: &mut EffectBlock,
    expr: &mut impl FnMut(&mut Expr),
    change: &mut impl FnMut(&mut Change),
) {
    if let Some(cond) = &mut effect.cond {
        visit_expr(cond, expr);
    }
    for a in &mut effect.actions {
        change(a);
    }
}
fn visit_parts(parts: &mut [TextPart], f: &mut impl FnMut(&mut Expr)) {
    for part in parts {
        if let TextPart::Expr(e) = part {
            visit_expr(e, f);
        }
    }
}
fn visit_body(
    body: &mut [Stmt],
    expr: &mut impl FnMut(&mut Expr),
    change: &mut impl FnMut(&mut Change),
) {
    for stmt in body {
        match stmt {
            Stmt::Text(t) => visit_parts(&mut t.parts, expr),
            Stmt::Choice(c) => {
                visit_parts(&mut c.label, expr);
                if let Some(e) = &mut c.cond {
                    visit_expr(e, expr);
                }
                visit_body(&mut c.body, expr, change);
            }
            Stmt::If(i) => {
                for (cond, body) in &mut i.branches {
                    if let Some(e) = cond {
                        visit_expr(e, expr);
                    }
                    visit_body(body, expr, change);
                }
            }
            Stmt::Let(l) => visit_expr(&mut l.expr, expr),
            Stmt::Set(s) => visit_expr(&mut s.expr, expr),
            Stmt::Scene(s) => visit_body(&mut s.body, expr, change),
            Stmt::Change(c) => change(&mut c.change),
            Stmt::Effect(e) => visit_effect(e, expr, change),
            _ => {}
        }
    }
}
fn visit(
    program: &mut Program,
    expr: &mut impl FnMut(&mut Expr),
    change: &mut impl FnMut(&mut Change),
) {
    for l in &mut program.lets {
        visit_expr(&mut l.expr, expr);
    }
    for event in &mut program.events {
        if let Some(e) = &mut event.after {
            visit_expr(e, expr);
        }
        for e in &mut event.effects {
            visit_effect(e, expr, change);
        }
        visit_body(&mut event.body, expr, change);
    }
}

fn allocate(base: &str, used: &mut BTreeSet<String>) -> String {
    let mut id = base.to_string();
    let mut suffix = 2;
    while !used.insert(id.clone()) {
        id = format!("{base}_{suffix}");
        suffix += 1;
    }
    id
}

pub(crate) fn normalize(program: &mut Program, sources: &BTreeMap<PathBuf, String>) {
    let mut permissions = BTreeSet::new();
    let mut actions = BTreeSet::new();
    visit(
        program,
        &mut |e| {
            if let Some(p) = permission(e) {
                permissions.insert(p.to_string());
            }
        },
        &mut |c| {
            if matches!(c.kind, ChangeKind::Grant | ChangeKind::Revoke) {
                actions.insert(c.id.clone());
            }
        },
    );
    permissions.extend(actions);
    permissions.extend(program.events.iter().filter_map(|e| e.perm.clone()));
    let existing = read_metadata(sources).filter(|m| {
        program.catalog.iter().any(
            |d| matches!(d, CatalogDecl::State(s) if s.id == m.state && s.target.kind == "world"),
        ) && m.tags.values().all(|id| {
            program
                .catalog
                .iter()
                .any(|d| matches!(d, CatalogDecl::Tag(t) if &t.name == id))
        })
    });
    if permissions.is_empty() {
        program.permission_migration = existing;
        return;
    }
    let legacy_fingerprint = fingerprint_program(program);
    // 收集全部文件中的词法标识符，也覆盖不同对象种类和未来目录扩展。
    let mut used: BTreeSet<String> = sources
        .values()
        .flat_map(|s| {
            let cleaned = lexer::strip_comments(s);
            tokens(&cleaned.chars().collect::<Vec<_>>())
                .into_iter()
                .filter(|t| !t.quoted && crate::authoring::identifier(&t.value).is_ok())
                .map(|t| t.value)
                .collect::<Vec<_>>()
        })
        .collect();
    let mut migration = existing.unwrap_or_else(|| PermissionMigration {
        state: allocate("__narrative_identity", &mut used),
        tags: BTreeMap::new(),
        gates: BTreeMap::new(),
        legacy_fingerprint,
        normalized_fingerprint: 0,
        declarations: String::new(),
    });
    migration.legacy_fingerprint = legacy_fingerprint;
    let new_state = !program
        .catalog
        .iter()
        .any(|d| matches!(d, CatalogDecl::State(s) if s.id == migration.state));
    let world = if new_state {
        if let Some(world) = program.worlds.first() {
            world.name.clone()
        } else {
            let id = allocate("__permission_world", &mut used);
            migration
                .declarations
                .push_str(&format!("world {id} as \"世界\"\n"));
            id
        }
    } else {
        String::new()
    };
    for p in permissions {
        if !migration.tags.contains_key(&p) {
            let tag = allocate(&format!("__permission_{}", hex(&p)), &mut used);
            migration
                .declarations
                .push_str(&format!("tag {tag} as {}\n", crate::authoring::quote(&p)));
            migration.tags.insert(p, tag);
        }
    }
    if new_state {
        migration.declarations.push_str(&format!(
            "state {} on world {world} with [] as \"叙事身份\"\n",
            migration.state
        ));
    }
    // 同一声明文本既用于 AST 也用于源码迁移，保证 ID、顺序和指纹一致。
    let file = program.files.first().cloned().unwrap_or_default();
    let lines = lexer::lex_source(&file, &migration.declarations, &mut Vec::new());
    let generated = crate::parser::Parser::new(&lines, &mut Vec::new()).parse_program();
    program.worlds.extend(generated.worlds);
    program.catalog.extend(generated.catalog);
    visit(
        program,
        &mut |e| {
            if let Some(p) = permission(e).map(str::to_string) {
                let loc = match e {
                    Expr::Call { loc, .. } => *loc,
                    _ => unreachable!(),
                };
                *e = migration.predicate(&p, loc);
            }
        },
        &mut |c| {
            if matches!(c.kind, ChangeKind::Grant | ChangeKind::Revoke) {
                c.tags = vec![migration.tags[&c.id].clone()];
                c.id = migration.state.clone();
                c.kind = if c.kind == ChangeKind::Grant {
                    ChangeKind::AddTags
                } else {
                    ChangeKind::RemoveTags
                };
            }
        },
    );
    for event in &mut program.events {
        if let Some(p) = event.perm.take() {
            migration
                .gates
                .insert(event.name.clone(), migration.tags[&p].clone());
            let gate = migration.predicate(&p, event.loc);
            event.after = Some(match event.after.take() {
                Some(after) => Expr::Binary {
                    op: BinOp::And,
                    lhs: Box::new(gate),
                    rhs: Box::new(after),
                },
                None => gate,
            });
        }
    }
    program.permission_migration = Some(migration);
    let normalized = fingerprint_program(program);
    program
        .permission_migration
        .as_mut()
        .unwrap()
        .normalized_fingerprint = normalized;
}

// 只用于定位编辑区间：语句种类与表达式含义由正式 lexer / expression 提供。
struct Token {
    value: String,
    start: usize,
    end: usize,
    quoted: bool,
}
fn tokens(chars: &[char]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let quoted = chars[i] == '"';
        if quoted {
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i = (i + 2).min(chars.len());
                } else if chars[i] == '"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else if chars[i].is_ascii_alphanumeric() || chars[i] == '_' {
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
        } else {
            i += 1;
        }
        out.push(Token {
            value: chars[start..i].iter().collect(),
            start,
            end: i,
            quoted,
        });
    }
    out
}

//! 声明身份来自 AST；重复声明保留，不经过目录单键映射。
use crate::{
    ast::{Program, Stmt},
    catalog::CatalogDecl,
    lexer::{Line, LineKind},
};

pub(super) struct Decl {
    pub kind: &'static str,
    pub id: String,
    pub display: String,
    pub entity_type: Option<String>,
    pub line: u32,
}
impl Decl {
    fn new(kind: &'static str, id: &str, display: Option<&str>, line: u32) -> Self {
        Self {
            kind,
            id: id.into(),
            display: display.unwrap_or(id).into(),
            entity_type: None,
            line,
        }
    }
}

pub(super) fn declarations(program: &Program, lines: &[Line]) -> Vec<Decl> {
    let mut out = Vec::new();
    macro_rules! named {
        ($values:expr, $kind:literal) => {
            for value in $values {
                out.push(Decl::new(
                    $kind,
                    &value.name,
                    value.display.as_deref(),
                    value.loc.line,
                ));
            }
        };
    }
    named!(&program.worlds, "world");
    named!(&program.characters, "character");
    named!(&program.periods, "period");
    named!(&program.relation_types, "relation_type");
    for value in &program.entities {
        let mut entry = Decl::new(
            "entity",
            &value.name,
            value.display.as_deref(),
            value.loc.line,
        );
        entry.entity_type = Some(value.entity_type.clone());
        out.push(entry);
    }
    for line in lines {
        // Explicit unnamed storyline selects main, but no implicit main entry is invented.
        if let LineKind::Storyline { name, display, .. } = &line.kind {
            out.push(Decl::new(
                "storyline",
                if name.is_empty() { "main" } else { name },
                display.as_deref(),
                line.no,
            ));
        }
    }
    for value in &program.relations {
        out.push(Decl::new("relation", &value.id, None, value.loc.line));
    }
    for value in &program.lets {
        out.push(Decl::new(
            if value.is_const { "const" } else { "let" },
            &value.name,
            None,
            value.loc.line,
        ));
    }
    for value in &program.rules {
        out.push(Decl::new("rule", &value.name, None, value.loc.line));
    }
    for value in &program.schemas {
        out.push(Decl::new("schema", &value.id, None, value.loc.line));
    }
    for value in &program.catalog {
        let entry = match value {
            CatalogDecl::Tag(v) => Decl::new("tag", &v.name, v.display.as_deref(), v.loc.line),
            CatalogDecl::Anchor(v) => {
                Decl::new("anchor", &v.name, v.display.as_deref(), v.loc.line)
            }
            CatalogDecl::Asset(v) => Decl::new("asset", &v.id, Some(&v.display), v.loc.line),
            CatalogDecl::State(v) => Decl::new("state", &v.id, Some(&v.display), v.line),
            _ => continue,
        };
        out.push(entry);
    }
    for value in &program.events {
        out.push(Decl::new(
            "event",
            &value.name,
            value.summary.as_deref(),
            value.loc.line,
        ));
        scenes(&value.body, &value.name, &mut out);
    }
    for value in &program.fragments {
        out.push(Decl::new("fragment", &value.name, None, value.loc.line));
    }
    out.sort_by_key(|value| value.line);
    out
}

fn scenes(body: &[Stmt], prefix: &str, out: &mut Vec<Decl>) {
    for stmt in body {
        match stmt {
            Stmt::Scene(value) => {
                let id = format!("{prefix}.{}", value.name);
                out.push(Decl::new("scene", &id, Some(&value.name), value.loc.line));
                scenes(&value.body, &id, out);
            }
            Stmt::Choice(value) => scenes(&value.body, prefix, out),
            Stmt::If(value) => {
                for (_, branch) in &value.branches {
                    scenes(branch, prefix, out);
                }
            }
            _ => {}
        }
    }
}

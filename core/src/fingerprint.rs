//! 与源码位置无关的程序内容指纹。

use crate::ast::{DivertTarget, Expr, Program, Stmt, TextPart, UnOp};

/// FNV-1a 内容哈希,用于存档兼容性校验。不含行号,因此增删注释不破坏存档。
pub fn fingerprint_program(program: &Program) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    for declaration in &program.lets {
        mix(
            &mut hash,
            &format!("G{}{}", declaration.is_const, declaration.name),
        );
        walk_expr(&mut hash, &declaration.expr);
    }
    for storyline in &program.storylines {
        mix(&mut hash, &format!("Y{}", storyline.name));
        if let Some(display) = &storyline.display {
            mix(&mut hash, &format!("Yd{display}"));
        }
    }
    for character in &program.characters {
        mix(&mut hash, &format!("P{}", character.name));
        if let Some(display) = &character.display {
            mix(&mut hash, &format!("Pd{display}"));
        }
        for property in &character.properties {
            mix(
                &mut hash,
                &format!("Pa{}{:?}", property.name, property.value),
            );
        }
        for relation in &character.relations {
            mix(
                &mut hash,
                &format!("Pr{}:{}", relation.target.len(), relation.target),
            );
            mix(&mut hash, &relation.label);
        }
    }
    for world in &program.worlds {
        mix(&mut hash, &format!("W{}", world.name));
        mix(&mut hash, world.display.as_deref().unwrap_or(""));
        mix(&mut hash, &world.description);
        for property in &world.properties {
            mix(
                &mut hash,
                &format!("Wa{}{:?}", property.name, property.value),
            );
        }
    }
    for decl in &program.catalog {
        if let crate::catalog::CatalogDecl::State(state) = decl {
            mix(
                &mut hash,
                &format!("State{}:{}", state.id, state.target.kind),
            );
            mix(&mut hash, &state.target.id);
            for tag in &state.tags {
                mix(&mut hash, tag);
            }
        }
    }
    for event in &program.events {
        mix(&mut hash, &format!("E{}", event.name));
        mix(&mut hash, &format!("Ys{}", event.storyline));
        if let Some(summary) = &event.summary {
            mix(&mut hash, &format!("Es{summary}"));
        }
        for character in &event.characters {
            mix(&mut hash, &format!("Ec{character}"));
        }
        if let Some(permission) = &event.perm {
            mix(&mut hash, &format!("Ep{permission}"));
        }
        if let Some(after) = &event.after {
            mix(&mut hash, "Ea");
            walk_expr(&mut hash, after);
        }
        for effect in &event.effects {
            mix(&mut hash, &format!("F{:?}", effect.when));
            if let Some(condition) = &effect.cond {
                walk_expr(&mut hash, condition);
            }
            for action in &effect.actions {
                mix(&mut hash, &format!("X{:?}{}", action.kind, action.id));
                for tag in &action.tags {
                    mix(&mut hash, tag);
                }
                if let Some(storyline) = &action.to_storyline {
                    mix(&mut hash, &format!("L{storyline}"));
                }
                if let Some(note) = &action.note {
                    mix(&mut hash, &format!("A{note}"));
                }
            }
        }
        walk_stmts(&mut hash, &event.body);
    }
    // 兼容映射影响旧档注入和准入求值顺序，也必须绑定内容；不混入指纹自身。
    if let Some(m) = &program.permission_migration {
        mix(&mut hash, &format!("PermissionState{}", m.state));
        for (permission, tag) in &m.tags {
            mix(&mut hash, permission);
            mix(&mut hash, tag);
        }
        for (event, tag) in &m.gates {
            mix(&mut hash, &format!("PermissionGate{event}"));
            mix(&mut hash, tag);
        }
    }
    mix(&mut hash, &format!("X{}", program.entry));
    hash
}

fn mix(hash: &mut u64, text: &str) {
    for byte in text.as_bytes() {
        *hash ^= *byte as u64;
        *hash = hash.wrapping_mul(0x100000001b3);
    }
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(0x100000001b3);
}

fn walk_expr(hash: &mut u64, expr: &Expr) {
    match expr {
        Expr::Num(number) => mix(hash, &format!("n{number}")),
        Expr::Str(text) => mix(hash, &format!("s{text}")),
        Expr::Bool(value) => mix(hash, &format!("b{value}")),
        Expr::Var { name, .. } => mix(hash, &format!("v{name}")),
        Expr::Unary { op, expr } => {
            mix(hash, "u");
            mix(
                hash,
                match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "!",
                },
            );
            walk_expr(hash, expr);
        }
        Expr::Binary { op, lhs, rhs } => {
            mix(hash, "b");
            mix(hash, op.symbol());
            walk_expr(hash, lhs);
            walk_expr(hash, rhs);
        }
        Expr::Call { name, args, .. } => {
            mix(hash, &format!("c{name}"));
            for argument in args {
                walk_expr(hash, argument);
            }
        }
    }
}

fn walk_parts(hash: &mut u64, parts: &[TextPart]) {
    let mut literal = String::new();
    for part in parts {
        match part {
            TextPart::Str(text) => literal.push_str(text),
            TextPart::Link(link) => literal.push_str(&link.label),
            TextPart::Expr(expr) => {
                if !literal.is_empty() {
                    mix(hash, &format!("t{literal}"));
                    literal.clear();
                }
                mix(hash, "e");
                walk_expr(hash, expr);
            }
        }
    }
    if !literal.is_empty() {
        mix(hash, &format!("t{literal}"));
    }
}

fn walk_stmts(hash: &mut u64, statements: &[Stmt]) {
    for statement in statements {
        match statement {
            Stmt::Text(text) => {
                mix(hash, "T");
                walk_parts(hash, &text.parts);
                if text.glue {
                    mix(hash, "~");
                }
            }
            Stmt::Choice(choice) => {
                mix(hash, &format!("C{}", choice.once));
                walk_parts(hash, &choice.label);
                if let Some(condition) = &choice.cond {
                    walk_expr(hash, condition);
                }
                walk_stmts(hash, &choice.body);
            }
            Stmt::If(statement) => {
                mix(hash, "I");
                for (condition, body) in &statement.branches {
                    match condition {
                        Some(expr) => walk_expr(hash, expr),
                        None => mix(hash, "else"),
                    }
                    walk_stmts(hash, body);
                }
            }
            Stmt::Divert(divert) => {
                if divert.drift {
                    mix(hash, ">>");
                }
                match &divert.target {
                    DivertTarget::Node(name) => mix(hash, &format!("D{name}")),
                    DivertTarget::End => mix(hash, "DE"),
                }
            }
            Stmt::Let(declaration) => {
                mix(
                    hash,
                    &format!("L{}{}", declaration.is_const, declaration.name),
                );
                walk_expr(hash, &declaration.expr);
            }
            Stmt::Set(assignment) => {
                mix(hash, &format!("S{}", assignment.name));
                walk_expr(hash, &assignment.expr);
            }
            Stmt::Scene(scene) => {
                mix(hash, &format!("N{}", scene.name));
                walk_stmts(hash, &scene.body);
            }
            Stmt::Change(change) => {
                mix(
                    hash,
                    &format!("X{:?}{}", change.change.kind, change.change.id),
                );
                for tag in &change.change.tags {
                    mix(hash, tag);
                }
                if let Some(storyline) = &change.change.to_storyline {
                    mix(hash, &format!("L{storyline}"));
                }
                if let Some(note) = &change.change.note {
                    mix(hash, &format!("A{note}"));
                }
            }
            Stmt::Anchor(anchor) => {
                mix(hash, &format!("M{}", anchor.name));
                if let Some(note) = &anchor.note {
                    mix(hash, &format!("A{note}"));
                }
            }
            Stmt::Effect(effect) => {
                mix(hash, &format!("F{:?}", effect.when));
                if let Some(condition) = &effect.cond {
                    walk_expr(hash, condition);
                }
                for action in &effect.actions {
                    mix(hash, &format!("X{:?}{}", action.kind, action.id));
                    for tag in &action.tags {
                        mix(hash, tag);
                    }
                    if let Some(storyline) = &action.to_storyline {
                        mix(hash, &format!("L{storyline}"));
                    }
                    if let Some(note) = &action.note {
                        mix(hash, &format!("A{note}"));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::compile_source;

    #[test]
    fn comments_do_not_change_the_program_fingerprint() {
        let plain = compile_source("story.wl", "event start\n  正文。\n  -> END\n");
        let commented = compile_source(
            "story.wl",
            "// 注释\nevent start\n  正文。 // 行尾注释\n  -> END\n",
        );
        assert_eq!(plain.analysis.fingerprint, commented.analysis.fingerprint);
    }

    #[test]
    fn semantic_content_changes_the_program_fingerprint() {
        let first = compile_source("story.wl", "event start\n  甲。\n  -> END\n");
        let second = compile_source("story.wl", "event start\n  乙。\n  -> END\n");
        assert_ne!(first.analysis.fingerprint, second.analysis.fingerprint);
    }
}

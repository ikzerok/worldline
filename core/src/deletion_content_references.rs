//! 内容删除前的 1.9 语言引用反查。
//!
//! 该查询只返回删除目标后仍会留在源码中的引用。事件自身正文、场景和
//! 效果会随事件块一起删除，因此其中的引用不构成外部阻断；顶层目录声明、
//! 别名以及其他事件中的条件调用则会保留，必须列入影响计划。

use crate::ast::{Expr, Stmt, TextPart};
use crate::catalog::{Catalog, CatalogLink, ReferenceInfo, TargetRef};
use crate::CompileResult;

/// 返回当前编译快照中会阻止删除 `target` 的语言引用。
///
/// 结果包含既有目录反向引用，并补充别名、顶层 `mark`/`attach` 所有者引用，
/// 以及 `seen`/`visits` 条件调用。事件目标的场景子节点也视为该事件的一部分；
/// 目标事件自身正文、场景与效果中的引用会被排除。
pub fn content_deletion_references(
    content: &CompileResult,
    target: &TargetRef,
) -> Vec<ReferenceInfo> {
    let catalog = &content.analysis.catalog;
    let mut references = catalog
        .references
        .iter()
        .filter(|reference| {
            affected_by_deletion(&reference.target, target)
                && !(reference.kind == "对象属性引用" && reference.source == *target)
                && !belongs_to_deleted_event(&reference.source, target)
                && !is_internal_catalog_reference(catalog, reference, target)
        })
        .cloned()
        .collect::<Vec<_>>();

    for alias in &catalog.aliases {
        if affected_by_deletion(&alias.target, target) {
            references.push(ReferenceInfo {
                source: file_target(&alias.file),
                target: alias.target.clone(),
                kind: "别名引用".into(),
                file: alias.file.clone(),
                line: alias.line,
            });
        }
    }
    collect_link_references(
        &mut references,
        &catalog.marks,
        target,
        "tag",
        "标签标记",
        "标签引用",
    );
    collect_link_references(
        &mut references,
        &catalog.attachments,
        target,
        "asset",
        "素材附件",
        "素材引用",
    );

    for (index, event) in content.program.events.iter().enumerate() {
        let Some(file) = content.program.event_files.get(index) else {
            continue;
        };
        let owner = TargetRef::new("event", &event.name);
        if let Some(after) = &event.after {
            collect_expr_references(content, after, &owner, file, target, &mut references);
        }
        for effect in &event.effects {
            if let Some(condition) = &effect.cond {
                collect_expr_references(content, condition, &owner, file, target, &mut references);
            }
        }
        collect_stmt_references(content, &event.body, &owner, file, target, &mut references);
    }

    for declaration in &content.program.lets {
        let owner = file_target(&declaration.file);
        collect_expr_references(
            content,
            &declaration.expr,
            &owner,
            &declaration.file,
            target,
            &mut references,
        );
    }

    deduplicate_references(&mut references);
    references
}

fn collect_link_references(
    references: &mut Vec<ReferenceInfo>,
    links: &[CatalogLink],
    target: &TargetRef,
    value_kind: &str,
    owner_kind: &str,
    value_reference_kind: &str,
) {
    for link in links {
        if !link.inline && affected_by_deletion(&link.target, target) {
            references.push(ReferenceInfo {
                source: file_target(&link.file),
                target: link.target.clone(),
                kind: owner_kind.into(),
                file: link.file.clone(),
                line: link.line,
            });
        }
        if link.inline && belongs_to_deleted_event(&link.target, target) {
            continue;
        }
        for value in &link.values {
            let value_target = TargetRef::new(value_kind, value);
            if affected_by_deletion(&value_target, target) {
                references.push(ReferenceInfo {
                    source: file_target(&link.file),
                    target: value_target,
                    kind: value_reference_kind.into(),
                    file: link.file.clone(),
                    line: link.line,
                });
            }
        }
    }
}

fn deduplicate_references(references: &mut Vec<ReferenceInfo>) {
    references.sort_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then(left.line.cmp(&right.line))
            .then(left.source.cmp(&right.source))
            .then(left.target.cmp(&right.target))
            .then(left.kind.cmp(&right.kind))
    });
    references.dedup_by(|left, right| {
        left.file == right.file
            && left.line == right.line
            && left.source == right.source
            && left.target == right.target
            && left.kind == right.kind
    });
}

fn collect_stmt_references(
    content: &CompileResult,
    body: &[Stmt],
    owner: &TargetRef,
    file: &str,
    target: &TargetRef,
    references: &mut Vec<ReferenceInfo>,
) {
    for statement in body {
        match statement {
            Stmt::Text(text) => {
                collect_text_references(content, &text.parts, owner, file, target, references)
            }
            Stmt::Choice(choice) => {
                collect_text_references(content, &choice.label, owner, file, target, references);
                if let Some(condition) = &choice.cond {
                    collect_expr_references(content, condition, owner, file, target, references);
                }
                collect_stmt_references(content, &choice.body, owner, file, target, references);
            }
            Stmt::If(branches) => {
                for (condition, branch) in branches.branches.iter() {
                    if let Some(condition) = condition {
                        collect_expr_references(
                            content, condition, owner, file, target, references,
                        );
                    }
                    collect_stmt_references(content, branch, owner, file, target, references);
                }
            }
            Stmt::Let(declaration) => {
                collect_expr_references(content, &declaration.expr, owner, file, target, references)
            }
            Stmt::Set(set) => {
                collect_named_reference(
                    &set.name,
                    "variable",
                    owner,
                    file,
                    set.loc.line,
                    target,
                    references,
                );
                collect_expr_references(content, &set.expr, owner, file, target, references)
            }
            Stmt::Scene(scene) => {
                let scene_owner = scene_target(owner, &scene.name);
                collect_stmt_references(
                    content,
                    &scene.body,
                    &scene_owner,
                    file,
                    target,
                    references,
                );
            }
            Stmt::Divert(_) | Stmt::Change(_) | Stmt::Anchor(_) | Stmt::Effect(_) => {}
        }
    }
}

fn collect_text_references(
    content: &CompileResult,
    parts: &[TextPart],
    owner: &TargetRef,
    file: &str,
    target: &TargetRef,
    references: &mut Vec<ReferenceInfo>,
) {
    for part in parts {
        if let TextPart::Expr(expression) = part {
            collect_expr_references(content, expression, owner, file, target, references);
        }
    }
}

fn collect_expr_references(
    content: &CompileResult,
    expression: &Expr,
    owner: &TargetRef,
    file: &str,
    target: &TargetRef,
    references: &mut Vec<ReferenceInfo>,
) {
    match expression {
        Expr::Call { name, args, loc } => {
            if matches!(name.as_str(), "seen" | "visits") {
                if let Some(Expr::Str(node_name)) = args.first() {
                    if let Some(referenced_target) = resolve_node(content, node_name) {
                        collect_resolved_reference(
                            owner,
                            file,
                            loc.line,
                            referenced_target,
                            target,
                            references,
                        );
                    }
                }
            } else if name == "has" {
                for (argument, kind) in args.iter().zip(["state", "tag"]) {
                    if let Some(referenced_target) = resolve_static_target(argument, kind) {
                        collect_resolved_reference(
                            owner,
                            file,
                            loc.line,
                            referenced_target,
                            target,
                            references,
                        );
                    }
                }
            }
            if !matches!(name.as_str(), "seen" | "visits" | "has") {
                for argument in args {
                    collect_expr_references(content, argument, owner, file, target, references);
                }
            }
        }
        Expr::Unary { expr, .. } => {
            collect_expr_references(content, expr, owner, file, target, references);
        }
        Expr::Binary { lhs, rhs, .. } => {
            collect_expr_references(content, lhs, owner, file, target, references);
            collect_expr_references(content, rhs, owner, file, target, references);
        }
        Expr::Var { name, loc } => {
            collect_named_reference(name, "variable", owner, file, loc.line, target, references)
        }
        Expr::Num(_) | Expr::Str(_) | Expr::Bool(_) => {}
    }
}

fn collect_resolved_reference(
    owner: &TargetRef,
    file: &str,
    line: u32,
    referenced_target: TargetRef,
    target: &TargetRef,
    references: &mut Vec<ReferenceInfo>,
) {
    if affected_by_deletion(&referenced_target, target) && !belongs_to_deleted_event(owner, target)
    {
        references.push(ReferenceInfo {
            source: owner.clone(),
            target: referenced_target,
            kind: "条件引用".into(),
            file: file.into(),
            line,
        });
    }
}

fn collect_named_reference(
    name: &str,
    kind: &str,
    owner: &TargetRef,
    file: &str,
    line: u32,
    target: &TargetRef,
    references: &mut Vec<ReferenceInfo>,
) {
    collect_resolved_reference(
        owner,
        file,
        line,
        TargetRef::new(kind, name),
        target,
        references,
    );
}

fn resolve_static_target(expression: &Expr, kind: &str) -> Option<TargetRef> {
    match expression {
        Expr::Str(id) => Some(TargetRef::new(kind, id)),
        Expr::Var { name, .. } => Some(TargetRef::new(kind, name)),
        _ => None,
    }
}

fn is_internal_catalog_reference(
    catalog: &Catalog,
    reference: &ReferenceInfo,
    target: &TargetRef,
) -> bool {
    match reference.kind.as_str() {
        "标签引用" => catalog.marks.iter().any(|link| {
            link.inline
                && link.file == reference.file
                && link.line == reference.line
                && link.target == reference.target
                && affected_by_deletion(&link.target, target)
                && link
                    .values
                    .iter()
                    .any(|value| reference.source == TargetRef::new("tag", value))
        }),
        "素材引用" => catalog.attachments.iter().any(|link| {
            link.inline
                && link.file == reference.file
                && link.line == reference.line
                && link.target == reference.source
                && affected_by_deletion(&link.target, target)
                && link
                    .values
                    .iter()
                    .any(|value| reference.target == TargetRef::new("asset", value))
        }),
        _ => false,
    }
}

fn resolve_node(content: &CompileResult, name: &str) -> Option<TargetRef> {
    let path = content.analysis.symbols.resolve_node(name)?;
    let event = content.program.events.get(path.event)?;
    if path.scenes.is_empty() {
        Some(TargetRef::new("event", &event.name))
    } else {
        Some(TargetRef::new("scene", &path.full_name(&event.name)))
    }
}

fn scene_target(owner: &TargetRef, scene: &str) -> TargetRef {
    TargetRef::new("scene", &format!("{}.{}", owner.id, scene))
}

/// 判断引用目标是否会随删除目标一起消失。
///
/// 事件删除同时移除其场景子节点；删除独立场景只影响该场景本身。
pub(crate) fn affected_by_deletion(referenced: &TargetRef, target: &TargetRef) -> bool {
    referenced == target
        || (target.kind == "event"
            && referenced.kind == "scene"
            && referenced.id.starts_with(&format!("{}.", target.id)))
}

fn belongs_to_deleted_event(source: &TargetRef, target: &TargetRef) -> bool {
    target.kind == "event"
        && ((source.kind == "event" && source.id == target.id)
            || (source.kind == "scene" && source.id.starts_with(&format!("{}.", target.id))))
}

fn file_target(file: &str) -> TargetRef {
    TargetRef::new("file", file)
}

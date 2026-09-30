//! 使用正式插值 AST 的字符范围改写，转义、引号和链接语法由 core 原解析器决定。
use crate::ast::{Expr, TextPart};
use crate::catalog::TargetRef;
type Edit = (usize, usize, String);
pub(super) fn rewrite(
    raw: &str,
    target: &TargetRef,
    new_id: &str,
    decorations: bool,
    quoted: bool,
) -> (String, usize) {
    let body = if decorations {
        crate::parser::split_text_decorations(raw).0
    } else {
        raw.to_owned()
    };
    let chars: Vec<_> = body.chars().collect();
    let parse = if quoted {
        crate::expression::parse_quoted_interpolations_with_options
    } else {
        crate::expression::parse_interpolations_with_options
    };
    let parts = parse(
        &body,
        "rename.wl",
        1,
        0,
        &mut Vec::new(),
        crate::CompileOptions::v1_11(),
    );
    let mut edits = Vec::new();
    for part in parts {
        match part {
            TextPart::Expr(expr) => expression(&expr, &chars, target, new_id, &mut edits),
            TextPart::Link(link) if link.target == *target => edits.push((
                link.start,
                link.end,
                format!("[[{}:{new_id}|{}]]", target.kind, link.label),
            )),
            _ => {}
        }
    }
    edits.sort_by_key(|e| e.0);
    edits.dedup();
    let count = edits.len();
    let mut result = chars;
    for (start, end, text) in edits.into_iter().rev() {
        if end <= result.len() {
            result.splice(start..end, text.chars());
        }
    }
    let mut result: String = result.into_iter().collect();
    result.push_str(&raw[body.len()..]);
    (result, count)
}
fn expression(
    expr: &Expr,
    chars: &[char],
    target: &TargetRef,
    new_id: &str,
    edits: &mut Vec<Edit>,
) {
    match expr {
        Expr::Call { name, args, loc } => {
            let start = loc.column.saturating_sub(1) as usize;
            if target.kind == "rule"
                && name == &target.id
                && chars
                    .get(start..start + name.len())
                    .is_some_and(|s| s.iter().collect::<String>() == *name)
            {
                edits.push((start, start + name.len(), new_id.into()));
            }
            if name == &target.kind
                && matches!(name.as_str(), "tag" | "state")
                && args.first().and_then(crate::language::static_id) == Some(target.id.as_str())
            {
                let mut at = start + name.len();
                while chars.get(at).is_some_and(|c| c.is_whitespace()) {
                    at += 1;
                }
                if chars.get(at) == Some(&'(') {
                    at += 1;
                    while chars.get(at).is_some_and(|c| c.is_whitespace()) {
                        at += 1;
                    }
                    if chars.get(at) == Some(&'\\') && chars.get(at + 1) == Some(&'"') {
                        let inner = at + 2;
                        if chars
                            .get(inner..inner + target.id.len())
                            .is_some_and(|s| s.iter().collect::<String>() == target.id)
                            && chars.get(inner + target.id.len()) == Some(&'\\')
                            && chars.get(inner + target.id.len() + 1) == Some(&'"')
                        {
                            edits.push((inner, inner + target.id.len(), new_id.into()));
                        }
                    } else if chars.get(at) == Some(&'"') {
                        if let Ok((value, end)) =
                            crate::lexer::parse_quoted(chars, at, "rename.wl", 1, &mut Vec::new())
                        {
                            if value == target.id {
                                edits.push((at, end, crate::authoring::quote(new_id)));
                            }
                        }
                    } else if chars
                        .get(at..at + target.id.len())
                        .is_some_and(|s| s.iter().collect::<String>() == target.id)
                    {
                        edits.push((at, at + target.id.len(), new_id.into()));
                    }
                }
                return;
            }
            if name == "has" && matches!(target.kind.as_str(), "state" | "tag") {
                let selected = usize::from(target.kind == "tag");
                if args.get(selected).and_then(crate::language::static_id)
                    == Some(target.id.as_str())
                {
                    if let Some(arg) = args.get(selected) {
                        if let Expr::Var { name, loc } = arg {
                            let at = loc.column.saturating_sub(1) as usize;
                            edits.push((at, at + name.len(), new_id.into()));
                        } else {
                            let mut at = start + name.len();
                            while chars.get(at).is_some_and(|c| c.is_whitespace()) {
                                at += 1;
                            }
                            at += 1;
                            for index in 0..=selected {
                                while chars
                                    .get(at)
                                    .is_some_and(|c| c.is_whitespace() || *c == ',')
                                {
                                    at += 1;
                                }
                                let Some((inner, end, next)) = static_argument(chars, at) else {
                                    break;
                                };
                                if index == selected
                                    && chars[inner..end].iter().collect::<String>() == target.id
                                {
                                    edits.push((inner, end, new_id.into()));
                                }
                                at = next;
                            }
                        }
                    }
                }
                return;
            }
            for arg in args {
                expression(arg, chars, target, new_id, edits);
            }
        }
        Expr::Unary { expr, .. } => expression(expr, chars, target, new_id, edits),
        Expr::Binary { lhs, rhs, .. } => {
            expression(lhs, chars, target, new_id, edits);
            expression(rhs, chars, target, new_id, edits);
        }
        _ => {}
    }
}

fn static_argument(chars: &[char], at: usize) -> Option<(usize, usize, usize)> {
    if chars.get(at) == Some(&'\\') && chars.get(at + 1) == Some(&'"') {
        let start = at + 2;
        let end = (start..chars.len().saturating_sub(1))
            .find(|i| chars[*i] == '\\' && chars[*i + 1] == '"')?;
        return Some((start, end, end + 2));
    }
    if chars.get(at) == Some(&'"') {
        let (_, end) =
            crate::lexer::parse_quoted(chars, at, "rename.wl", 1, &mut Vec::new()).ok()?;
        return Some((at + 1, end - 1, end));
    }
    let mut end = at;
    while chars
        .get(end)
        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
    {
        end += 1;
    }
    (end > at).then_some((at, end, end))
}

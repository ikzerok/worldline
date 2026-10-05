//! 仅源码结构查询启用的有界正式表达式解析。普通编译没有这个资源门。
//! token 在现有字符串/内插解码之后取得，不把显示名或普通正文当表达式。
use super::Tok;
use std::cell::Cell;

thread_local! {
    static EXCEEDED: Cell<Option<bool>> = const { Cell::new(None) };
}

struct Restore(Option<bool>);
impl Drop for Restore {
    fn drop(&mut self) {
        EXCEEDED.set(self.0);
    }
}

pub(crate) fn scoped<T>(parse: impl FnOnce() -> T) -> Result<T, ()> {
    let restore = Restore(EXCEEDED.replace(Some(false)));
    let result = parse();
    let exceeded = EXCEEDED.get() == Some(true);
    drop(restore);
    if exceeded {
        Err(())
    } else {
        Ok(result)
    }
}

pub(super) fn allow(tokens: &[(Tok, u32, u32)]) -> bool {
    let Some(exceeded) = EXCEEDED.get() else {
        return true;
    };
    let allowed = !exceeded && within_limits(tokens);
    if !allowed {
        EXCEEDED.set(Some(true));
    }
    allowed
}

fn within_limits(tokens: &[(Tok, u32, u32)]) -> bool {
    if tokens.len() > 256 {
        return false;
    }
    let mut frames = Vec::new();
    let mut depth = 0;
    let mut unary = 0;
    let mut operand = true;
    for (token, _, _) in tokens {
        match token {
            Tok::KwNot | Tok::Op("-") if operand => unary += 1,
            Tok::LParen => {
                let added = unary + 1;
                frames.push(added);
                depth += added;
                unary = 0;
                operand = true;
            }
            Tok::RParen => {
                depth -= frames.pop().unwrap_or(0);
                unary = 0;
                operand = false;
            }
            Tok::Op(_) | Tok::KwAnd | Tok::KwOr | Tok::Comma => {
                unary = 0;
                operand = true;
            }
            _ => {
                unary = 0;
                operand = false;
            }
        }
        if depth + unary > 64 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::parse_expr_src;

    #[test]
    fn outline_scope_restores_normal_compilation_and_nested_scopes() {
        let source = format!("{}1{}", "(".repeat(65), ")".repeat(65));
        let parse = || parse_expr_src(&source, "test.wl", 1, 0, &mut Vec::new());
        assert!(scoped(parse).is_err());
        assert!(EXCEEDED.get().is_none());
        let mut diagnostics = Vec::new();
        parse_expr_src(&source, "test.wl", 1, 0, &mut diagnostics);
        assert!(diagnostics.is_empty());
        assert!(scoped(|| {
            assert!(scoped(parse).is_err());
            parse_expr_src("1", "test.wl", 1, 0, &mut Vec::new())
        })
        .is_ok());
        assert!(std::panic::catch_unwind(|| scoped(|| panic!("scope unwind"))).is_err());
        assert!(EXCEEDED.get().is_none());
    }
}

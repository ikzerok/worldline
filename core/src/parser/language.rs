use super::*;
use crate::language::*;

impl Parser<'_> {
    fn language_error(&mut self, file: &str, loc: Loc, message: &str) {
        self.diags.push(Diagnostic::error(
            "P004",
            file,
            Span::new(loc.line, loc.column, 4),
            message,
        ));
    }
    fn signature<'s>(
        &mut self,
        source: &'s str,
        file: &str,
        loc: Loc,
    ) -> Option<(String, Vec<Parameter>, &'s str)> {
        let (name, rest) = source.split_once('(')?;
        let (params, tail) = rest.split_once(')')?;
        if !valid_name(name.trim()) {
            self.language_error(file, loc, "规则/片段名必须是标识符");
            return None;
        }
        let mut parameters = Vec::new();
        if !params.trim().is_empty() {
            for part in params.split(',') {
                let Some((name, kind)) = part.split_once(':') else {
                    self.language_error(file, loc, "参数必须写作 name: type");
                    return None;
                };
                let Some(kind) = value_kind(kind) else {
                    self.language_error(file, loc, "未知参数类型");
                    return None;
                };
                if !valid_name(name.trim()) {
                    self.language_error(file, loc, "参数名必须是标识符");
                    return None;
                }
                parameters.push(Parameter {
                    name: name.trim().into(),
                    kind,
                    loc,
                });
            }
        }
        Some((name.trim().into(), parameters, tail.trim()))
    }
    pub(super) fn parse_language_declaration(
        &mut self,
        program: &mut Program,
        keyword: &str,
        source: &str,
        loc: Loc,
        indent: u32,
        file: &str,
    ) {
        let Some((name, parameters, tail)) = self.signature(source, file, loc) else {
            self.language_error(file, loc, "顶层需要完整 rule / fragment 签名");
            return;
        };
        match keyword {
            "rule" => {
                let Some((result, expr)) = tail.strip_prefix("->").and_then(|s| s.split_once('='))
                else {
                    self.language_error(file, loc, "规则需要 -> type = expression");
                    return;
                };
                let Some(result) = value_kind(result) else {
                    self.language_error(file, loc, "未知规则返回类型");
                    return;
                };
                let expr = self.language_expr(keyword, source, expr.trim(), loc, file);
                program.rules.push(RuleDecl {
                    name,
                    parameters,
                    result,
                    expr,
                    file: file.into(),
                    loc,
                });
            }
            "fragment" => {
                if !tail.is_empty() {
                    self.language_error(file, loc, "fragment 签名之后不得包含其他内容");
                }
                let body = self.parse_block(indent, file, true);
                program.fragments.push(FragmentDecl {
                    name,
                    parameters,
                    body,
                    file: file.into(),
                    loc,
                });
            }
            _ => self.language_error(file, loc, "此1.11语句只能写在执行块内"),
        }
    }
    pub(super) fn parse_language_statement(
        &mut self,
        keyword: &str,
        source: &str,
        loc: Loc,
        file: &str,
    ) -> Stmt {
        let fallback = || {
            Stmt::Text(TextStmt {
                parts: Vec::new(),
                tags: Vec::new(),
                glue: false,
                localization_id: None,
                loc,
            })
        };
        match keyword {
            "return" => {
                if !source.is_empty() {
                    self.language_error(file, loc, "return 不接受参数");
                }
                Stmt::Return(loc)
            }
            "call" => match self.language_expr(keyword, source, source, loc, file) {
                Expr::Call { name, args, .. } => Stmt::Call(CallStmt { name, args, loc }),
                _ => {
                    self.language_error(file, loc, "call 需要片段名及括号参数");
                    fallback()
                }
            },
            "local" => {
                let Some((head, expr)) = source.split_once('=') else {
                    self.language_error(file, loc, "local 需要 name: type = expression");
                    return fallback();
                };
                let Some((name, kind)) = head.split_once(':') else {
                    self.language_error(file, loc, "local 必须显式声明类型");
                    return fallback();
                };
                let Some(kind) = value_kind(kind) else {
                    self.language_error(file, loc, "未知局部类型");
                    return fallback();
                };
                if !valid_name(name.trim()) {
                    self.language_error(file, loc, "局部名必须是标识符");
                }
                Stmt::Local(LocalStmt {
                    name: name.trim().into(),
                    kind,
                    expr: self.language_expr(keyword, source, expr.trim(), loc, file),
                    loc,
                })
            }
            "say" => self.parse_say(source, loc, file),
            "become" => {
                let Some((state, tags, kind)) = dynamic_change_parts(source) else {
                    self.language_error(
                        file,
                        loc,
                        "动态状态操作需要 with/add/remove from 集合表达式",
                    );
                    return fallback();
                };
                Stmt::DynamicChange(DynamicChangeStmt {
                    state: self.language_expr(keyword, source, state.trim(), loc, file),
                    tags: self.language_expr(keyword, source, tags.trim(), loc, file),
                    kind,
                    loc,
                })
            }
            _ => {
                self.language_error(file, loc, "rule / fragment 只能顶层声明");
                fallback()
            }
        }
    }
    fn language_expr(
        &mut self,
        keyword: &str,
        source: &str,
        expr: &str,
        loc: Loc,
        file: &str,
    ) -> Expr {
        let start = (expr.as_ptr() as usize)
            .saturating_sub(source.as_ptr() as usize)
            .min(source.len());
        let base =
            loc.column + keyword.chars().count() as u32 + source[..start].chars().count() as u32;
        parse_expr_src(expr, file, loc.line, base, self.diags)
    }
    fn parse_say(&mut self, source: &str, loc: Loc, file: &str) -> Stmt {
        let (speaker, rest) = source
            .split_once(char::is_whitespace)
            .unwrap_or((source, ""));
        let chars: Vec<char> = rest.trim().chars().collect();
        if chars.first() != Some(&'"') {
            self.language_error(file, loc, "say正文必须是引号字符串");
        }
        let (raw, end) = crate::lexer::parse_quoted_raw(&chars, 0, file, loc.line, self.diags)
            .unwrap_or_default();
        let tail: String = chars[end..].iter().collect();
        let mut tail = tail.trim().to_string();
        let mut direction = None;
        if let Some(rest) = tail.strip_prefix("direction ") {
            let chars: Vec<char> = rest.trim().chars().collect();
            if chars.first() != Some(&'"') {
                self.language_error(file, loc, "direction必须是引号字符串");
            }
            if let Ok((text, end)) =
                crate::lexer::parse_quoted(&chars, 0, file, loc.line, self.diags)
            {
                direction = Some(text);
                tail = chars[end..].iter().collect::<String>().trim().into();
            }
        }
        let mut localization_id = None;
        if let Some(id) = tail.strip_prefix("#wl-localization:") {
            let (_, value) = super::text::extract_localization_id(
                vec![format!("wl-localization:{id}")],
                file,
                loc,
                self.allow_localization_ids,
                self.diags,
            );
            localization_id = value;
        } else if !tail.is_empty() {
            self.language_error(file, loc, "say 正文后只允许 direction 与本地化ID");
        }
        if !valid_name(speaker) {
            self.language_error(file, loc, "say 需要角色ID");
        }
        let parts = crate::expression::parse_quoted_interpolations_with_options(
            &raw,
            file,
            loc.line,
            loc.column + speaker.chars().count() as u32 + 5,
            self.diags,
            self.options(),
        );
        Stmt::Say(SayStmt {
            speaker: speaker.into(),
            text: TextStmt {
                parts,
                glue: false,
                tags: Vec::new(),
                localization_id,
                loc,
            },
            direction,
            loc,
        })
    }
}
fn valid_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

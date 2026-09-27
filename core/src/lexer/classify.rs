use super::*;

mod catalog;
mod declarations;
mod statements;

struct ClassifyInput<'a> {
    word: &'a str,
    content: &'a str,
    rest_trim: &'a str,
}

pub(super) fn classify(
    file: &str,
    no: u32,
    chars: &[char],
    diags: &mut Vec<Diagnostic>,
    options: crate::compiler::CompileOptions,
) -> LineKind {
    // 转义开头:`\choice ...` 视为文本
    if chars[0] == '\\' {
        let content: String = chars[1..].iter().collect();
        return LineKind::Text {
            content: content.trim_start().to_string(),
            loc: Loc::new(no, 2),
        };
    }
    if chars[0] == '-' && chars.get(1) == Some(&'>') {
        // 跃迁;->> 为漂流
        let mut drift = false;
        let mut base = 2;
        if chars.get(2) == Some(&'>') {
            drift = true;
            base = 3;
        }
        let rest = skip_spaces(chars, base);
        let (target, end) = match scan_qualified(chars, rest) {
            Some((n, e)) if e == chars.len() => (n, e),
            _ if chars[rest..].iter().collect::<String>().trim() == "END" => {
                if drift {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, 1, 3),
                        "漂流 `->>` 不能以 END 为目标(END 没有故事线);请使用 `-> END`",
                    ));
                    drift = false;
                }
                return LineKind::Divert {
                    target: "END".to_string(),
                    drift,
                    span: Span::new(no, (rest + 1) as u32, 3),
                };
            }
            _ => {
                let target: String = chars[rest..].iter().collect::<String>();
                let target = target.trim().to_string();
                let len = target.chars().count() as u32;
                if target.is_empty() {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, 1, 2),
                        "`->` 后缺少跃迁目标",
                    ));
                    return LineKind::Divert {
                        target,
                        drift,
                        span: Span::new(no, 1, 2),
                    };
                }
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, (rest + 1) as u32, len),
                    format!("非法的跃迁目标 `{target}`"),
                ));
                return LineKind::Divert {
                    target,
                    drift,
                    span: Span::new(no, (rest + 1) as u32, len),
                };
            }
        };
        return LineKind::Divert {
            target,
            drift,
            span: Span::new(no, (rest + 1) as u32, (end - rest) as u32),
        };
    }

    let content: String = chars.iter().collect();
    let (word, rest) = split_word(&content);
    let rest_trim = rest.trim();
    let word_col = (content.len() - rest.len() - word.len()) as u32 + 1;
    match word {
        "tag" | "asset" | "mark" | "attach" | "anchor_def" | "anchor_link" | "alias"
        | "property" | "description" | "relation" => {
            catalog::classify_catalog(word, rest_trim, file, no, word_col, diags, options)
        }
        "relation_type" | "relation_def" | "inverse" | "direction" | "from_kind" | "to_kind"
        | "from" | "to" | "source_note" | "scope" | "scope_ref"
            if options.language_version.supports_relations() =>
        {
            catalog::classify_catalog(word, rest_trim, file, no, word_col, diags, options)
        }
        "include" | "let" | "const" | "set" | "event" | "storyline" | "character" | "world"
        | "period" | "state" | "become" => declarations::classify_declaration(
            ClassifyInput {
                word,
                content: &content,
                rest_trim,
            },
            file,
            no,
            word_col,
            diags,
            options,
        ),
        "entity" if options.language_version.supports_entities() => {
            declarations::classify_declaration(
                ClassifyInput {
                    word,
                    content: &content,
                    rest_trim,
                },
                file,
                no,
                word_col,
                diags,
                options,
            )
        }
        "effect" | "grant" | "revoke" | "meet" | "part" | "to" | "anchor" | "scene" | "choice"
        | "if" | "else" => statements::classify_statement(
            ClassifyInput {
                word,
                content: &content,
                rest_trim,
            },
            file,
            no,
            word_col,
            diags,
            options,
        ),
        _ => LineKind::Text {
            content,
            loc: Loc::new(no, 1),
        },
    }
}

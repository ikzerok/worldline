use super::super::*;
use super::ClassifyInput;

pub(super) fn classify_statement(
    input: ClassifyInput<'_>,
    file: &str,
    no: u32,
    word_col: u32,
    diags: &mut Vec<Diagnostic>,
    options: crate::compiler::CompileOptions,
) -> LineKind {
    let ClassifyInput {
        word,
        content,
        rest_trim,
    } = input;
    match word {
        "effect" => {
            let rc = rest_trim.chars().collect::<Vec<char>>();
            let i0 = skip_spaces(&rc, 0);
            let (w, wi) = scan_word(&rc, i0);
            let (when_src, cond_src) = if w == "on" {
                let i1 = skip_spaces(&rc, wi);
                let (w2, wi2) = scan_word(&rc, i1);
                if w2 != "enter" && w2 != "done" && w2 != "exit" {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, word_col, 6),
                        "effect 的生效时机只能是 `on enter`、`on exit` 或 `on done`",
                    ));
                    (w2, None)
                } else {
                    let i2 = skip_spaces(&rc, wi2);
                    let (w3, wi3) = scan_word(&rc, i2);
                    if w3 == "if" {
                        let rest: String = rc[wi3..].iter().collect();
                        let t = rest.trim().to_string();
                        if t.is_empty() {
                            diags.push(Diagnostic::error(
                                "P004",
                                file,
                                Span::new(no, word_col, 6),
                                "`effect on … if` 之后需要条件表达式",
                            ));
                        }
                        (w2, Some(t))
                    } else if !w3.is_empty() {
                        diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(no, word_col, 6),
                            "effect 时机之后只能是 `if 条件` 或换行开启效果块",
                        ));
                        (w2, None)
                    } else {
                        (w2, None)
                    }
                }
            } else {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, 6),
                    "effect 需要生效时机:`effect on enter`、`effect on exit` 或 `effect on done`",
                ));
                (String::new(), None)
            };
            LineKind::Effect {
                when_src,
                cond_src,
                loc: Loc::new(no, word_col),
            }
        }
        "grant" | "revoke" | "meet" | "part" => {
            let kind = match word {
                "grant" => crate::ast::ChangeKind::Grant,
                "revoke" => crate::ast::ChangeKind::Revoke,
                "meet" => crate::ast::ChangeKind::Meet,
                _ => crate::ast::ChangeKind::Part,
            };
            let rc = rest_trim.chars().collect::<Vec<char>>();
            let (id, note) = match scan_qualified(&rc, 0) {
                Some((n, end)) if !n.contains('.') && !n.is_empty() => {
                    let (note, _) = parse_as_note(&rc, end, file, no, diags);
                    (n, note)
                }
                _ => {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, word_col, word.chars().count() as u32),
                        format!("`{word}` 后需要对象名(权限或角色标识符)"),
                    ));
                    (String::new(), None)
                }
            };
            LineKind::ChangeLine {
                kind,
                id,
                note,
                loc: Loc::new(no, word_col),
            }
        }
        "to" => {
            let rc = rest_trim.chars().collect::<Vec<char>>();
            let (storyline, note) = match scan_qualified(&rc, 0) {
                Some((n, end)) if !n.contains('.') && !n.is_empty() => {
                    let (note, _) = parse_as_note(&rc, end, file, no, diags);
                    (n, note)
                }
                _ => {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, word_col, 2),
                        "`to` 后需要故事线名(此语句只能写在效果块内)",
                    ));
                    (String::new(), None)
                }
            };
            LineKind::ToLine {
                storyline,
                note,
                loc: Loc::new(no, word_col),
            }
        }
        "anchor" => {
            let rc = rest_trim.chars().collect::<Vec<char>>();
            let i0 = skip_spaces(&rc, 0);
            let (name, note) = if i0 < rc.len() && rc[i0] == '"' {
                match parse_quoted(&rc, i0, file, no, diags) {
                    Ok((n, end)) => {
                        let (note, _) = parse_as_note(&rc, end, file, no, diags);
                        (n, note)
                    }
                    Err(_) => (String::new(), None),
                }
            } else {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, 6),
                    "anchor 后需要带引号的锚点名,如 anchor \"听闻密室\" as \"说明\"",
                ));
                (String::new(), None)
            };
            LineKind::Anchor {
                name,
                note,
                loc: Loc::new(no, word_col),
            }
        }
        "scene" => {
            let rc = rest_trim.chars().collect::<Vec<char>>();
            match scan_qualified(&rc, 0) {
                Some((name, end)) if !name.contains('.') && end == rc.len() && !name.is_empty() => {
                    let col = word_col + 5;
                    let len = name.chars().count() as u32;
                    LineKind::Scene {
                        name,
                        loc: Span::new(no, col, len),
                    }
                }
                _ => {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, word_col, 5),
                        "scene 后需要简单标识符(场景通过嵌套归属事件)",
                    ));
                    LineKind::Scene {
                        name: String::new(),
                        loc: Span::new(no, word_col, 5),
                    }
                }
            }
        }
        "choice" => {
            let rc: Vec<char> = rest_trim.chars().collect();
            let off = (content.chars().count() - rc.len()) as u32;
            let mut i = skip_spaces(&rc, 0);
            let mut once = false;
            // choice [once] ["label"] [if expr] [#wl-localization:id]
            if rc[i..].starts_with(&['o', 'n', 'c', 'e']) {
                let after = i + 4;
                let is_word = after >= rc.len() || rc[after] == ' ' || rc[after] == '"';
                if is_word && after <= rc.len() {
                    once = true;
                    i = skip_spaces(&rc, after);
                }
            }
            if i >= rc.len() || rc[i] != '"' {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, off + i as u32 + 1, 6),
                    "choice 需要双引号标签,如 choice \"去市场\"",
                ));
                return LineKind::Choice {
                    once,
                    label_raw: String::new(),
                    cond_src: None,
                    localization_id: None,
                    loc: Loc::new(no, word_col),
                    label_span: Span::new(no, off + i as u32 + 1, 1),
                };
            }
            let label_start = i + 1;
            let label = match parse_quoted(&rc, i, file, no, diags) {
                Ok((s, end)) => {
                    i = skip_spaces(&rc, end);
                    s
                }
                Err(_) => {
                    return LineKind::Choice {
                        once,
                        label_raw: String::new(),
                        cond_src: None,
                        localization_id: None,
                        loc: Loc::new(no, word_col),
                        label_span: Span::new(no, off + i as u32 + 1, 1),
                    }
                }
            };
            let label_span = Span::new(
                no,
                off + label_start as u32 + 1,
                label.chars().count().max(1) as u32,
            );
            let raw_tail: String = rc[i..].iter().collect();
            let (tail, localization_id) = split_choice_localization_annotation(
                &raw_tail,
                file,
                no,
                off + i as u32 + 1,
                options.localization_ids,
                diags,
            );
            let mut cond_src = None;
            if tail.trim().starts_with("if") {
                let s = tail.trim();
                if let Some(cond) = s.strip_prefix("if") {
                    cond_src = Some(cond.trim().to_string());
                }
            } else if !tail.trim().is_empty() {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, off + i as u32 + 1, tail.chars().count() as u32),
                    "choice 标签之后只能是 `if 条件` 或 `#wl-localization:<id>`",
                ));
            }
            LineKind::Choice {
                once,
                label_raw: label,
                cond_src,
                localization_id,
                loc: Loc::new(no, word_col),
                label_span,
            }
        }
        "if" => {
            if rest_trim.is_empty() {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, 2),
                    "if 后需要条件表达式",
                ));
            }
            LineKind::If {
                cond_src: rest_trim.to_string(),
                loc: Loc::new(no, word_col),
            }
        }
        "else" => {
            let rc: Vec<char> = rest_trim.chars().collect();
            if rc.is_empty() {
                return LineKind::Else {
                    loc: Loc::new(no, word_col),
                };
            }
            // else if ...
            let s: String = rc.iter().collect();
            if let Some(cond) = s.strip_prefix("if") {
                let cond = cond.trim().to_string();
                if cond.is_empty() {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, word_col, 7),
                        "else if 后需要条件表达式",
                    ));
                }
                return LineKind::ElseIf {
                    cond_src: cond,
                    loc: Loc::new(no, word_col),
                };
            }
            diags.push(Diagnostic::error(
                "P004",
                file,
                Span::new(no, word_col, 4),
                "else 之后不能有其他内容(可用 `else if 条件`)",
            ));
            LineKind::Else {
                loc: Loc::new(no, word_col),
            }
        }
        _ => unreachable!("classification dispatches statement keywords"),
    }
}

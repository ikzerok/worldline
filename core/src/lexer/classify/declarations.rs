use super::super::*;
use super::ClassifyInput;

#[allow(clippy::too_many_arguments)]
pub(super) fn classify_declaration(
    input: ClassifyInput<'_>,
    file: &str,
    no: u32,
    word_col: u32,
    diags: &mut Vec<Diagnostic>,
    options: crate::compiler::CompileOptions,
    source: &mut LineSource,
) -> LineKind {
    let ClassifyInput {
        word,
        content,
        rest_trim,
    } = input;
    match word {
        "include" => {
            let rc: Vec<char> = rest_trim.chars().collect();
            if rc.is_empty() || rc[0] != '"' {
                diags.push(Diagnostic::error(
                    "P007",
                    file,
                    Span::new(no, source.remainder + 1, 1),
                    "include 需要双引号路径,如 include \"chapter2.wl\"",
                ));
                return LineKind::Include {
                    path: String::new(),
                    span: Span::new(no, source.remainder + 1, 1),
                };
            }
            match parse_quoted(&rc, 0, file, no, diags) {
                Ok((path, _end)) => {
                    let len = _end as u32;
                    LineKind::Include {
                        path,
                        span: Span::new(no, source.remainder + 1, len),
                    }
                }
                Err(_) => LineKind::Include {
                    path: String::new(),
                    span: Span::new(no, source.remainder + 1, 1),
                },
            }
        }
        "let" | "const" | "set" => {
            let rc: Vec<char> = rest_trim.chars().collect();
            let off = (content.chars().count() - rc.len()) as u32;
            let name_start = skip_spaces(&rc, 0);
            match scan_qualified(&rc, name_start) {
                Some((name, mut end)) if !name.contains('.') => {
                    end = skip_spaces(&rc, end);
                    if end >= rc.len() || rc[end] != '=' || name.is_empty() {
                        diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(no, off + name_start as u32 + 1, name.chars().count() as u32),
                            format!("`{word}` 需要 `{name} = 表达式` 的形式"),
                        ));
                    }
                    source.value = off + skip_spaces(&rc, (end + 1).min(rc.len())) as u32;
                    let expr_src: String = rc[(end + 1).min(rc.len())..].iter().collect();
                    let expr_src = expr_src.trim().to_string();
                    let loc = Loc::new(no, off + 1);
                    let name_span =
                        Span::new(no, off + name_start as u32 + 1, name.chars().count() as u32);
                    match word {
                        "let" => LineKind::Let {
                            name,
                            expr_src,
                            loc,
                            name_span,
                        },
                        "const" => LineKind::Const {
                            name,
                            expr_src,
                            loc,
                            name_span,
                        },
                        _ => LineKind::Set {
                            name,
                            expr_src,
                            loc,
                            name_span,
                        },
                    }
                }
                _ => {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, word_col, word.chars().count() as u32),
                        format!("`{word}` 后需要变量名与 `= 表达式`"),
                    ));
                    let loc = Loc::new(no, word_col);
                    let name_span = Span::new(no, word_col, 1);
                    let src = rest_trim.to_string();
                    match word {
                        "let" => LineKind::Let {
                            name: String::new(),
                            expr_src: src,
                            loc,
                            name_span,
                        },
                        "const" => LineKind::Const {
                            name: String::new(),
                            expr_src: src,
                            loc,
                            name_span,
                        },
                        _ => LineKind::Set {
                            name: String::new(),
                            expr_src: src,
                            loc,
                            name_span,
                        },
                    }
                }
            }
        }
        "event" => {
            let rc = rest_trim.chars().collect::<Vec<char>>();
            match scan_qualified(&rc, 0) {
                Some((name, end)) if !name.is_empty() => {
                    let col = source.remainder + 1;
                    let len = name.chars().count() as u32;
                    let loc = Span::new(no, col, len);
                    // 头部子句:as "简述" → with 角色… → perm 权限 → after 表达式(固定顺序)
                    let mut i = end;
                    let mut summary = None;
                    let mut order = None;
                    let mut period = None;
                    let mut predecessors = Vec::new();
                    let mut characters = Vec::new();
                    let mut perm = None;
                    let mut after_src: Option<String> = None;
                    loop {
                        i = skip_spaces(&rc, i);
                        if i >= rc.len() {
                            break;
                        }
                        let (w, wi) = scan_word(&rc, i);
                        match w.as_str() {
                            "during" | "follows" => {
                                i = skip_spaces(&rc, wi);
                                loop {
                                    match scan_qualified(&rc, i) {
                                        Some((id, end))
                                            if w == "follows" || valid_identifier(&id) =>
                                        {
                                            if w == "during" {
                                                period = Some(id);
                                            } else {
                                                predecessors.push(id);
                                            }
                                            i = skip_spaces(&rc, end);
                                            if w == "follows" && rc.get(i) == Some(&',') {
                                                i = skip_spaces(&rc, i + 1);
                                            } else {
                                                break;
                                            }
                                        }
                                        _ => {
                                            diags.push(Diagnostic::error(
                                                "P004",
                                                file,
                                                Span::new(no, 1, 6),
                                                "during / follows 后需要合法 ID",
                                            ));
                                            i = rc.len();
                                            break;
                                        }
                                    }
                                }
                            }
                            "at" => {
                                i = skip_spaces(&rc, wi);
                                let start = i;
                                while i < rc.len() && rc[i].is_ascii_digit() {
                                    i += 1;
                                }
                                order = rc[start..i]
                                    .iter()
                                    .collect::<String>()
                                    .parse::<u32>()
                                    .ok()
                                    .filter(|n| *n > 0);
                                if order.is_none() {
                                    diags.push(Diagnostic::error(
                                        "P004",
                                        file,
                                        Span::new(no, 1, 2),
                                        "at 后需要正整数序号",
                                    ));
                                    break;
                                }
                            }
                            "as" => {
                                i = skip_spaces(&rc, wi);
                                match parse_quoted(&rc, i, file, no, diags) {
                                    Ok((s, e2)) => {
                                        summary = Some(s);
                                        i = e2;
                                    }
                                    Err(_) => break,
                                }
                            }
                            "with" => {
                                i = wi;
                                loop {
                                    i = skip_spaces(&rc, i);
                                    match scan_qualified(&rc, i) {
                                        Some((c, e2)) if !c.contains('.') && !c.is_empty() => {
                                            characters.push(c);
                                            i = skip_spaces(&rc, e2);
                                            if i < rc.len() && rc[i] == ',' {
                                                i += 1;
                                            } else {
                                                break;
                                            }
                                        }
                                        _ => {
                                            diags.push(Diagnostic::error(
                                                "P004",
                                                file,
                                                Span::new(no, word_col, 4),
                                                "`with` 后需要角色名(多个以逗号分隔)",
                                            ));
                                            break;
                                        }
                                    }
                                }
                            }
                            "perm" => {
                                i = skip_spaces(&rc, wi);
                                match scan_qualified(&rc, i) {
                                    Some((p, e2)) if !p.contains('.') && !p.is_empty() => {
                                        perm = Some(p);
                                        i = e2;
                                    }
                                    _ => {
                                        diags.push(Diagnostic::error(
                                            "P004",
                                            file,
                                            Span::new(no, word_col, 4),
                                            "`perm` 后需要权限名",
                                        ));
                                        break;
                                    }
                                }
                            }
                            "after" => {
                                source.condition = source.remainder + skip_spaces(&rc, wi) as u32;
                                let rest: String = rc[wi..].iter().collect();
                                let t = rest.trim().to_string();
                                if t.is_empty() {
                                    diags.push(Diagnostic::error(
                                        "P004",
                                        file,
                                        Span::new(no, word_col, 5),
                                        "`after` 后需要条件表达式(如 after seen(hall))",
                                    ));
                                }
                                after_src = Some(t);
                                break;
                            }
                            _ => {
                                diags.push(Diagnostic::error(
                                    "P004",
                                    file,
                                    Span::new(no, word_col, 5),
                                    "事件头部子句只能按 as / with / perm / after 顺序书写",
                                ));
                                break;
                            }
                        }
                    }
                    LineKind::Event {
                        name,
                        summary,
                        order,
                        period,
                        predecessors,
                        characters,
                        perm,
                        after_src,
                        loc,
                    }
                }
                _ => {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, word_col, 5),
                        "event 后需要事件名(标识符,可含 . 定义场景)",
                    ));
                    LineKind::Event {
                        name: String::new(),
                        summary: None,
                        order: None,
                        period: None,
                        predecessors: Vec::new(),
                        characters: Vec::new(),
                        perm: None,
                        after_src: None,
                        loc: Span::new(no, word_col, 5),
                    }
                }
            }
        }
        "storyline" => {
            let rc = rest_trim.chars().collect::<Vec<char>>();
            let (name, display) = match scan_qualified(&rc, 0) {
                Some((n, end)) if !n.contains('.') && !n.is_empty() => {
                    let (note, _) = parse_as_note(&rc, end, file, no, diags);
                    (n, note)
                }
                _ => {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, word_col, 9),
                        "storyline 后需要故事线名(标识符),如 storyline main as \"主线\"",
                    ));
                    (String::new(), None)
                }
            };
            LineKind::Storyline {
                name,
                display,
                loc: Span::new(no, word_col, 9),
            }
        }
        "character" | "world" | "period" => {
            let rc = rest_trim.chars().collect::<Vec<char>>();
            let mut parent = None;
            let (name, display) = match scan_qualified(&rc, 0) {
                Some((n, end)) if !n.contains('.') && !n.is_empty() => {
                    let (note, consumed) = parse_as_note(&rc, end, file, no, diags);
                    let mut tail = skip_spaces(&rc, consumed.max(end));
                    if word == "period" && scan_word(&rc, tail).0 == "within" {
                        tail = skip_spaces(&rc, scan_word(&rc, tail).1);
                        if let Some((id, end)) = scan_qualified(&rc, tail) {
                            if !id.contains('.') {
                                parent = Some(id);
                                tail = skip_spaces(&rc, end);
                            }
                        }
                        if parent.is_none() {
                            diags.push(Diagnostic::error(
                                "P004",
                                file,
                                Span::new(no, 1, 6),
                                "within 后需要上级时段 ID",
                            ));
                        }
                    }
                    if tail != rc.len() {
                        diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(no, 1, 5),
                            "声明 ID 后只能是 as \"显示名\"",
                        ));
                    }
                    (n, note)
                }
                _ => {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, word_col, 9),
                        "character 后需要角色名(标识符),如 character servant as \"女仆\"",
                    ));
                    (String::new(), None)
                }
            };
            let loc = Span::new(no, word_col, word.len() as u32);
            if word == "period" {
                LineKind::Period {
                    name,
                    display,
                    parent,
                    loc,
                }
            } else if word == "world" {
                LineKind::World { name, display, loc }
            } else {
                LineKind::Character { name, display, loc }
            }
        }
        "entity" if options.language_version.supports_entities() => {
            let rc = rest_trim.chars().collect::<Vec<char>>();
            let mut cursor = 0;
            let (name, end) = match scan_qualified(&rc, cursor) {
                Some((n, end)) if !n.contains('.') && !n.is_empty() => (n, end),
                _ => {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, word_col, 6),
                        "entity 后需要实体 ID(标识符)",
                    ));
                    (String::new(), 0)
                }
            };
            cursor = skip_spaces(&rc, end);
            let (kind_word, kind_end) = scan_word(&rc, cursor);
            if kind_word != "kind" {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, 6),
                    "entity ID 后需要 `kind 实体分类`",
                ));
            } else {
                cursor = skip_spaces(&rc, kind_end);
            }
            let (entity_type, type_end) = scan_word(&rc, cursor);
            if entity_type.is_empty() || !valid_identifier(&entity_type) {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, 6),
                    "entity 的 kind 后需要实体分类标识符",
                ));
            }
            let (display, consumed) = parse_as_note(&rc, type_end, file, no, diags);
            if skip_spaces(&rc, consumed) != rc.len() {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, 6),
                    "entity 声明末尾只能是 as \"显示名\"",
                ));
            }
            LineKind::Entity {
                name,
                entity_type,
                display,
                loc: Span::new(no, word_col, 6),
            }
        }
        "state" => LineKind::Catalog(crate::catalog::CatalogDecl::State(
            crate::states::parse_declaration_with_options(rest_trim, file, no, diags, options),
        )),
        "become" => LineKind::Become(crate::states::parse_change(rest_trim, file, no, diags)),
        _ => unreachable!("classification dispatches declaration keywords"),
    }
}

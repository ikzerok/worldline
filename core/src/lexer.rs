//! 行导向词法分析:注释剥离 → 缩进测量 → 行分类。
//! worldline 是行式语言,词法层产出"物理行",由 parser 按缩进组块。

use crate::ast::Loc;
use crate::diagnostic::{Diagnostic, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotedStringError;

#[derive(Debug, Clone)]
pub struct Line {
    pub file: String,
    pub no: u32,
    pub indent: u32,
    pub kind: LineKind,
}

#[derive(Debug, Clone)]
pub enum LineKind {
    Become(crate::ast::Change),
    Catalog(crate::catalog::CatalogDecl),
    Include {
        path: String,
        span: Span,
    },
    Let {
        name: String,
        expr_src: String,
        loc: Loc,
        name_span: Span,
    },
    Const {
        name: String,
        expr_src: String,
        loc: Loc,
        name_span: Span,
    },
    Event {
        name: String,
        summary: Option<String>,
        order: Option<u32>,
        period: Option<String>,
        predecessors: Vec<String>,
        characters: Vec<String>,
        perm: Option<String>,
        after_src: Option<String>,
        loc: Span,
    },
    Storyline {
        name: String,
        display: Option<String>,
        loc: Span,
    },
    Character {
        name: String,
        display: Option<String>,
        loc: Span,
    },
    Entity {
        name: String,
        entity_type: String,
        display: Option<String>,
        loc: Span,
    },
    RelationType {
        name: String,
        display: Option<String>,
        loc: Span,
    },
    RelationDef {
        id: String,
        relation_type: String,
        from_kind: String,
        from_id: String,
        to_kind: String,
        to_id: String,
        loc: Span,
    },
    RelationField {
        name: String,
        value: String,
        loc: Loc,
    },
    World {
        name: String,
        display: Option<String>,
        loc: Span,
    },
    Period {
        name: String,
        display: Option<String>,
        parent: Option<String>,
        loc: Span,
    },
    Property {
        name: String,
        value_src: String,
        loc: Loc,
    },
    Description {
        text: String,
        loc: Loc,
    },
    Relation {
        target: String,
        label: String,
        loc: Loc,
    },
    Effect {
        when_src: String,
        cond_src: Option<String>,
        loc: Loc,
    },
    ChangeLine {
        kind: crate::ast::ChangeKind,
        id: String,
        note: Option<String>,
        loc: Loc,
    },
    ToLine {
        storyline: String,
        note: Option<String>,
        loc: Loc,
    },
    Anchor {
        name: String,
        note: Option<String>,
        loc: Loc,
    },
    Scene {
        name: String,
        loc: Span,
    },
    Choice {
        once: bool,
        label_raw: String,
        cond_src: Option<String>,
        loc: Loc,
        label_span: Span,
    },
    Divert {
        target: String,
        drift: bool,
        span: Span,
    },
    If {
        cond_src: String,
        loc: Loc,
    },
    ElseIf {
        cond_src: String,
        loc: Loc,
    },
    Else {
        loc: Loc,
    },
    Set {
        name: String,
        expr_src: String,
        loc: Loc,
        name_span: Span,
    },
    Text {
        content: String,
        loc: Loc,
    },
}

/// 字符串字面量解码:`\n \" \\ \{ \} \~ \#`。
pub fn decode_escapes(raw: &str, file: &str, span: Span, diags: &mut Vec<Diagnostic>) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('{') => out.push('{'),
            Some('}') => out.push('}'),
            Some('#') => out.push('#'),
            Some('~') => out.push('~'),
            Some(other) => {
                diags.push(Diagnostic::error(
                    "P003",
                    file,
                    span,
                    format!("未知的转义 \\{other}(仅支持 \\n \\t \\\" \\\\ \\{{ \\}} \\# \\~)"),
                ));
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// 解析双引号字符串字面量(含两侧引号)。返回 (解码内容, 结束引号后位置)。
pub fn parse_quoted(
    chars: &[char],
    start: usize,
    file: &str,
    line: u32,
    diags: &mut Vec<Diagnostic>,
) -> Result<(String, usize), QuotedStringError> {
    let mut raw = String::new();
    let mut i = start + 1;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            raw.push(c);
            raw.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '"' {
            let span = Span::new(line, (start + 1) as u32, (i - start) as u32);
            let decoded = decode_escapes(&raw, file, span, diags);
            return Ok((decoded, i + 1));
        }
        if c == '\n' {
            break;
        }
        raw.push(c);
        i += 1;
    }
    let span = Span::new(
        line,
        (start + 1) as u32,
        (chars.len() - start).max(1) as u32,
    );
    diags.push(Diagnostic::error("P003", file, span, "字符串未闭合"));
    Err(QuotedStringError)
}

/// 注释剥离:字符串感知(引号内的 `//` `/*` 不算注释),跨行块注释。
/// 注释字符替换为空格,保持行号与列不漂移。
pub fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;
    let mut in_block = false;
    while let Some(c) = chars.next() {
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                out.push_str("  ");
                in_block = false;
            } else if c == '\n' {
                out.push('\n');
            } else {
                out.push(' ');
            }
            continue;
        }
        if in_string {
            out.push(c);
            match c {
                '\\' => {
                    if let Some(&n) = chars.peek() {
                        out.push(n);
                        chars.next();
                    }
                }
                '"' | '\n' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' => {
                if chars.peek() == Some(&'/') {
                    // 行注释:吃掉本行剩余
                    chars.next();
                    out.push_str("  ");
                    for n in chars.by_ref() {
                        if n == '\n' {
                            out.push('\n');
                            in_string = false;
                            break;
                        }
                        out.push(' ');
                    }
                } else if chars.peek() == Some(&'*') {
                    chars.next();
                    out.push_str("  ");
                    in_block = true;
                } else {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// (第一个词, 其后剩余内容 trimmed_start)
fn split_word(content: &str) -> (&str, &str) {
    let trimmed = content.trim_start();
    let mut chars = trimmed.char_indices();
    match chars.next() {
        Some((_, c)) if is_ident_start(c) => {}
        _ => return ("", trimmed),
    }
    for (i, c) in chars {
        if !is_ident_char(c) {
            return (&trimmed[..i], &trimmed[i..]);
        }
    }
    (trimmed, "")
}

/// 解析 `ident` 或 `ident.scene.path`;返回 (名字, 结束位置 char 下标)。
fn scan_qualified(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut i = start;
    let mut name = String::new();
    loop {
        if i >= chars.len() || !is_ident_start(chars[i]) {
            return None;
        }
        while i < chars.len() && is_ident_char(chars[i]) {
            name.push(chars[i]);
            i += 1;
        }
        // 允许 `.` 连接
        if i + 1 < chars.len() && chars[i] == '.' && is_ident_start(chars[i + 1]) {
            name.push('.');
            i += 1;
        } else {
            break;
        }
    }
    Some((name, i))
}

pub fn valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn skip_spaces(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i] == ' ' {
        i += 1;
    }
    i
}

/// 从 start 起扫描一个标识符词;返回 (词, 结束下标)。
fn scan_word(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start;
    while i < chars.len() && is_ident_char(chars[i]) {
        i += 1;
    }
    (chars[start..i].iter().collect(), i)
}

/// 解析可选的 `as "备注"`;返回 (备注, 消费后的下标)。
fn parse_as_note(
    chars: &[char],
    start: usize,
    file: &str,
    no: u32,
    diags: &mut Vec<Diagnostic>,
) -> (Option<String>, usize) {
    let i = skip_spaces(chars, start);
    if i >= chars.len() {
        return (None, start);
    }
    let (w, wi) = scan_word(chars, i);
    if w != "as" {
        return (None, start);
    }
    let j = skip_spaces(chars, wi);
    if j < chars.len() && chars[j] == '"' {
        match parse_quoted(chars, j, file, no, diags) {
            Ok((s, end)) => (Some(s), end),
            Err(_) => (None, start),
        }
    } else {
        diags.push(Diagnostic::error(
            "P004",
            file,
            Span::new(no, (wi + 1) as u32, 2),
            "`as` 之后需要带引号的文本",
        ));
        (None, start)
    }
}

/// 词法入口:一个源文件 → 物理行序列(空行与纯注释行已剔除)。
pub fn lex_source(file: &str, src: &str, diags: &mut Vec<Diagnostic>) -> Vec<Line> {
    lex_source_with_options(file, src, diags, crate::compiler::CompileOptions::default())
}

/// 依据显式语言版本进行词法分类。1.9 保持 `entity` 为普通文本，避免
/// 新关键字改变旧正文的解释。
pub fn lex_source_with_options(
    file: &str,
    src: &str,
    diags: &mut Vec<Diagnostic>,
    options: crate::compiler::CompileOptions,
) -> Vec<Line> {
    let cleaned = strip_comments(src);
    let mut lines = Vec::new();
    for (idx, raw_line) in cleaned.lines().enumerate() {
        let no = (idx + 1) as u32;
        // 缩进
        let mut indent = 0u32;
        let mut chars: Vec<char> = Vec::new();
        let mut leading = true;
        let mut bad_tab = false;
        for c in raw_line.chars() {
            if leading && c == ' ' {
                indent += 1;
                continue;
            }
            if leading && c == '\t' {
                bad_tab = true;
                continue;
            }
            leading = false;
            chars.push(c);
        }
        if bad_tab {
            diags.push(Diagnostic::error(
                "P002",
                file,
                Span::new(no, 1, 1),
                "缩进使用了 Tab:worldline 只允许空格缩进",
            ));
        }
        if chars.is_empty() {
            continue;
        }
        let content: String = chars.iter().collect();
        let content_trim = content.trim_end().to_string();
        if content_trim.is_empty() {
            continue;
        }
        let chars: Vec<char> = content_trim.chars().collect();
        let kind = classify(file, no, &chars, diags, options);
        lines.push(Line {
            file: file.to_string(),
            no,
            indent,
            kind,
        });
    }
    lines
}

fn classify(
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
        "tag" | "asset" | "mark" | "attach" | "anchor_def" | "anchor_link" | "alias" => {
            LineKind::Catalog(crate::catalog_syntax::parse_with_options(
                word, rest_trim, file, no, diags, options,
            ))
        }
        "property" => {
            let (name, value_src) = rest_trim.split_once('=').unwrap_or((rest_trim, ""));
            let name = name.trim();
            if !valid_identifier(name) || value_src.trim().is_empty() {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, 1, 8),
                    "属性需要 property 名称 = 字面量",
                ));
            }
            LineKind::Property {
                name: name.into(),
                value_src: value_src.trim().into(),
                loc: Loc::new(no, 1),
            }
        }
        "description" => {
            let rc: Vec<char> = rest_trim.chars().collect();
            let text = if rc.first() == Some(&'"') {
                match parse_quoted(&rc, 0, file, no, diags) {
                    Ok((text, end)) if skip_spaces(&rc, end) == rc.len() => text,
                    _ => {
                        diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(no, 1, 11),
                            "description 只接受一个带引号的字符串",
                        ));
                        String::new()
                    }
                }
            } else {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, 1, 11),
                    "description 后需要带引号的字符串",
                ));
                String::new()
            };
            LineKind::Description {
                text,
                loc: Loc::new(no, 1),
            }
        }
        "relation_type" if options.language_version.supports_relations() => {
            let rc = rest_trim.chars().collect::<Vec<char>>();
            let (name, end) = match scan_qualified(&rc, 0) {
                Some((name, end)) if !name.contains('.') && valid_identifier(&name) => (name, end),
                _ => {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, word_col, 13),
                        "relation_type 后需要类型 ID",
                    ));
                    (String::new(), 0)
                }
            };
            let (display, consumed) = parse_as_note(&rc, end, file, no, diags);
            if skip_spaces(&rc, consumed) != rc.len() {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, 13),
                    "relation_type 声明末尾只能是 as \"显示名\"",
                ));
            }
            LineKind::RelationType {
                name,
                display,
                loc: Span::new(no, word_col, 13),
            }
        }
        "relation_def" if options.language_version.supports_relations() => {
            let tokens = crate::catalog_syntax::tokenize(rest_trim, file, no, diags);
            let token = |index: usize| {
                tokens
                    .get(index)
                    .map(|(value, _)| value.as_str())
                    .unwrap_or("")
            };
            let quoted = |index: usize| tokens.get(index).is_some_and(|(_, quoted)| *quoted);
            let qualified =
                |value: &str| !value.is_empty() && value.split('.').all(valid_identifier);
            let id = token(0).to_string();
            let relation_type = token(2).to_string();
            let from_kind = token(4).to_string();
            let from_id = token(5).to_string();
            let to_kind = token(7).to_string();
            let to_id = token(8).to_string();
            let endpoint = |kind: &str, id: &str, index: usize| {
                if kind == "file" {
                    quoted(index) && !id.is_empty()
                } else {
                    !quoted(index) && qualified(id)
                }
            };
            let valid = tokens.len() == 9
                && !quoted(0)
                && valid_identifier(&id)
                && token(1) == "type"
                && !quoted(2)
                && valid_identifier(&relation_type)
                && token(3) == "from"
                && !quoted(4)
                && valid_identifier(&from_kind)
                && endpoint(&from_kind, &from_id, 5)
                && token(6) == "to"
                && !quoted(7)
                && valid_identifier(&to_kind)
                && endpoint(&to_kind, &to_id, 8);
            if !valid {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, 12),
                    "relation_def 必须完整写出 from/to 的对象类型和 ID",
                ));
            }
            LineKind::RelationDef {
                id,
                relation_type,
                from_kind,
                from_id,
                to_kind,
                to_id,
                loc: Span::new(no, word_col, 12),
            }
        }
        "inverse" | "direction" | "from_kind" | "to_kind" | "from" | "to"
            if options.language_version.supports_relations() =>
        {
            let value = rest_trim.to_string();
            if value.is_empty() {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, word.len() as u32),
                    format!("{word} 后需要值"),
                ));
            }
            let value = if matches!(word, "inverse") {
                let rc = value.chars().collect::<Vec<char>>();
                match parse_quoted(&rc, 0, file, no, diags) {
                    Ok((text, end)) if skip_spaces(&rc, end) == rc.len() => text,
                    _ => {
                        diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(no, word_col, word.len() as u32),
                            "inverse 后需要一个带引号的显示名",
                        ));
                        String::new()
                    }
                }
            } else {
                value
            };
            LineKind::RelationField {
                name: word.into(),
                value,
                loc: Loc::new(no, word_col),
            }
        }
        "source_note" | "scope" | "scope_ref" if options.language_version.supports_relations() => {
            let value = rest_trim.to_string();
            if value.is_empty() {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, word.len() as u32),
                    format!("{word} 后需要值"),
                ));
            }
            let value = if word == "source_note" {
                let rc = value.chars().collect::<Vec<char>>();
                match parse_quoted(&rc, 0, file, no, diags) {
                    Ok((text, end)) if skip_spaces(&rc, end) == rc.len() => text,
                    _ => {
                        diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(no, word_col, word.len() as u32),
                            "source_note 后需要一个带引号的字符串",
                        ));
                        String::new()
                    }
                }
            } else {
                value
            };
            LineKind::RelationField {
                name: word.into(),
                value,
                loc: Loc::new(no, word_col),
            }
        }
        "relation" => {
            let rc: Vec<char> = rest_trim.chars().collect();
            let (target, label) = match scan_qualified(&rc, 0) {
                Some((target, end)) if valid_identifier(&target) => {
                    let (label, end) = parse_as_note(&rc, end, file, no, diags);
                    if label.as_ref().is_none_or(|s| s.trim().is_empty())
                        || skip_spaces(&rc, end) != rc.len()
                    {
                        diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(no, 1, 8),
                            "关系需要 relation 角色ID as \"关系名称\"",
                        ));
                    }
                    (target, label.unwrap_or_default())
                }
                _ => {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, 1, 8),
                        "relation 后需要角色 ID",
                    ));
                    (String::new(), String::new())
                }
            };
            LineKind::Relation {
                target,
                label,
                loc: Loc::new(no, 1),
            }
        }
        "include" => {
            let rc: Vec<char> = rest_trim.chars().collect();
            if rc.is_empty() || rc[0] != '"' {
                diags.push(Diagnostic::error(
                    "P007",
                    file,
                    Span::new(no, word_col + 7, 1),
                    "include 需要双引号路径,如 include \"chapter2.wl\"",
                ));
                return LineKind::Include {
                    path: String::new(),
                    span: Span::new(no, word_col + 7, 1),
                };
            }
            match parse_quoted(&rc, 0, file, no, diags) {
                Ok((path, _)) => {
                    let len = path.chars().count().max(1) as u32 + 2;
                    LineKind::Include {
                        path,
                        span: Span::new(no, word_col + 7, len),
                    }
                }
                Err(_) => LineKind::Include {
                    path: String::new(),
                    span: Span::new(no, word_col + 7, 1),
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
                    let col = word_col + 5;
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
            // choice [once] ["label"] [if expr]
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
            let mut cond_src = None;
            if rc[i..].iter().collect::<String>().trim().starts_with("if") {
                let s: String = rc[i..].iter().collect();
                let s = s.trim();
                if let Some(cond) = s.strip_prefix("if") {
                    cond_src = Some(cond.trim().to_string());
                }
            } else if i < rc.len() {
                let tail: String = rc[i..].iter().collect();
                if !tail.trim().is_empty() {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, off + i as u32 + 1, tail.chars().count() as u32),
                        "choice 标签之后只能是 `if 条件`",
                    ));
                }
            }
            LineKind::Choice {
                once,
                label_raw: label,
                cond_src,
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
        _ => LineKind::Text {
            content,
            loc: Loc::new(no, 1),
        },
    }
}

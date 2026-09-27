//! 行导向词法分析:注释剥离 → 缩进测量 → 行分类。
//! worldline 是行式语言,词法层产出"物理行",由 parser 按缩进组块。

use crate::ast::Loc;
use crate::diagnostic::{Diagnostic, Span};
mod classify;

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
        localization_id: Option<String>,
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
        let kind = classify::classify(file, no, &chars, diags, options);
        lines.push(Line {
            file: file.to_string(),
            no,
            indent,
            kind,
        });
    }
    lines
}

fn split_choice_localization_annotation(
    raw: &str,
    file: &str,
    line: u32,
    column: u32,
    enabled: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> (String, Option<String>) {
    const MARKER: &str = "#wl-localization:";
    let chars = raw.chars().collect::<Vec<_>>();
    let marker = MARKER.chars().collect::<Vec<_>>();
    let mut positions = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    for index in 0..chars.len() {
        let current = chars[index];
        if quoted {
            if escaped {
                escaped = false;
            } else if current == '\\' {
                escaped = true;
            } else if current == '"' {
                quoted = false;
            }
        } else if current == '"' {
            quoted = true;
        } else if chars[index..].starts_with(&marker)
            && (index == 0 || chars[index - 1].is_whitespace())
        {
            positions.push(index);
        }
    }
    let Some(&start) = positions.first() else {
        return (raw.trim_end().to_string(), None);
    };
    let source = chars[..start]
        .iter()
        .collect::<String>()
        .trim_end()
        .to_string();
    let id = chars[start + marker.len()..].iter().collect::<String>();
    let annotation_column = column + start as u32;
    if positions.len() != 1 || id.is_empty() || !crate::workspace_documents::valid_id(&id) {
        diagnostics.push(Diagnostic::error(
            "P004",
            file,
            Span::new(line, annotation_column, MARKER.chars().count() as u32),
            "本地化注记必须是唯一的 `#wl-localization:<id>`",
        ));
        return (source, None);
    }
    if !enabled {
        diagnostics.push(Diagnostic::error(
            "P004",
            file,
            Span::new(
                line,
                annotation_column,
                MARKER.chars().count() as u32 + id.len() as u32,
            ),
            "本地化注记需要清单能力 content.localization.v1",
        ));
        return (source, None);
    }
    (source, Some(id))
}

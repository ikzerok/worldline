use super::*;
/// 从原始文本行提取 (正文, 是否粘接~, 标签列表)。
/// 顺序:先剥行尾标签,再判粘接;正文中的转义交给 parse_interpolations。
pub(crate) fn split_text_decorations(raw: &str) -> (String, bool, Vec<String>) {
    let chars: Vec<char> = raw.chars().collect();
    // 1. 标签:未转义的 `#` 且前一字符是空白或行首
    let mut text_end = chars.len();
    let mut tags = Vec::new();
    let mut i = 0;
    let mut in_brace = 0u8; // 插值内的 # 不算标签
    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' if i == 0 || chars[i - 1] != '\\' => in_brace = in_brace.saturating_add(1),
            '}' if i == 0 || chars[i - 1] != '\\' => in_brace = in_brace.saturating_sub(1),
            '#' if in_brace == 0
                && (i == 0 || chars[i - 1].is_whitespace())
                && i + 1 < chars.len()
                && !chars[i + 1].is_whitespace() =>
            {
                if i < text_end {
                    text_end = i;
                }
                // 收集标签到空白或行尾
                let mut j = i + 1;
                let mut tag = String::new();
                while j < chars.len() && !chars[j].is_whitespace() {
                    tag.push(chars[j]);
                    j += 1;
                }
                tags.push(tag);
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    let mut text: String = chars[..text_end].iter().collect();
    // 2. 粘接:行尾(去尾随空格后)未转义的 `~`
    let trimmed = text.trim_end().to_string();
    let tchars: Vec<char> = trimmed.chars().collect();
    let mut glue = false;
    if let Some(&last) = tchars.last() {
        if last == '~' && (tchars.len() < 2 || tchars[tchars.len() - 2] != '\\') {
            glue = true;
            text = tchars[..tchars.len() - 1].iter().collect();
            text = text.trim_end().to_string();
        } else {
            text = trimmed;
        }
    }
    (text, glue, tags)
}

pub(super) fn extract_localization_id(
    tags: Vec<String>,
    file: &str,
    loc: Loc,
    enabled: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<String>, Option<String>) {
    let mut output_tags = Vec::with_capacity(tags.len());
    let mut localization_id = None;
    let mut annotation_count = 0;
    let tags_len = tags.len();
    for (index, tag) in tags.into_iter().enumerate() {
        let Some(id) = tag.strip_prefix("wl-localization:") else {
            if tag == "wl-localization" {
                diagnostics.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(loc.line, loc.column, tag.chars().count() as u32),
                    "本地化注记需要 `#wl-localization:<id>`",
                ));
                annotation_count += 1;
            } else {
                output_tags.push(tag);
            }
            continue;
        };
        annotation_count += 1;
        if index + 1 != tags_len {
            diagnostics.push(Diagnostic::error(
                "P004",
                file,
                Span::new(loc.line, loc.column, tag.chars().count() as u32),
                "`#wl-localization:<id>` 必须是文本行最后一个标签",
            ));
            continue;
        }
        if !enabled {
            diagnostics.push(Diagnostic::error(
                "P004",
                file,
                Span::new(loc.line, loc.column, tag.chars().count() as u32),
                "本地化注记需要清单能力 content.localization.v1",
            ));
            continue;
        }
        if !crate::workspace_documents::valid_id(id) {
            diagnostics.push(Diagnostic::error(
                "P004",
                file,
                Span::new(loc.line, loc.column, tag.chars().count() as u32),
                "本地化 ID 只能包含 ASCII 字母、数字、下划线和连字符",
            ));
            continue;
        }
        if localization_id.replace(id.to_string()).is_some() {
            diagnostics.push(Diagnostic::error(
                "P004",
                file,
                Span::new(loc.line, loc.column, tag.chars().count() as u32),
                "同一文本行只能声明一个本地化 ID",
            ));
        }
    }
    if annotation_count > 1 {
        localization_id = None;
    }
    (output_tags, localization_id)
}

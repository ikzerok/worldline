use super::inline::{collect_references, heading_slug, parse_inline};
use super::{
    hash_bytes, loss, valid_id, Heading, MarkdownImportConflict, MarkdownImportOptions, SourcePage,
    TargetRef,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(super) fn parse_page(relative: &str, bytes: &[u8], text: &str) -> Result<SourcePage, String> {
    let mut lines = text.lines().enumerate().peekable();
    let mut frontmatter = BTreeMap::new();
    if lines
        .peek()
        .is_some_and(|(_, line)| line.trim_end_matches('\r') == "---")
    {
        lines.next();
        let mut closed = false;
        for (line_index, raw) in lines.by_ref() {
            let line = raw.trim_end_matches('\r');
            if line == "---" || line == "..." {
                closed = true;
                break;
            }
            let Some((key, value)) = line.split_once(':') else {
                return Err(format!(
                    "front matter 必须为 key: value 标量：{relative}:{}",
                    line_index + 1
                ));
            };
            let key = key.trim();
            let value = parse_scalar(value.trim()).ok_or_else(|| {
                format!("front matter 不支持该标量值：{relative}:{}", line_index + 1)
            })?;
            if !valid_id(key) {
                return Err(format!(
                    "front matter 键名无效：{relative}:{}",
                    line_index + 1
                ));
            }
            if frontmatter.insert(key.to_string(), value).is_some() {
                return Err(format!("front matter 存在重复键：{relative}:{key}"));
            }
        }
        if !closed {
            return Err(format!("front matter 缺少结束分隔线：{relative}"));
        }
    }
    let mut title = frontmatter.get("title").cloned();
    let mut explicit_id = frontmatter.get("id").cloned();
    let entity_type = frontmatter
        .get("kind")
        .cloned()
        .unwrap_or_else(|| "lore".into());
    if !valid_id(&entity_type) {
        return Err(format!("front matter kind 不是有效标识符：{relative}"));
    }
    let mut losses = Vec::new();
    for key in frontmatter.keys() {
        if !matches!(key.as_str(), "id" | "title" | "kind") {
            losses.push(loss(
                "UNSUPPORTED_FRONT_MATTER_FIELD",
                relative,
                1,
                format!("front matter 字段 `{key}` 不会转成实体字段"),
                None,
            ));
        }
    }
    if title.as_ref().is_some_and(|value| value.trim().is_empty()) {
        title = None;
    }
    let fallback_id = format!("md_{:016x}", hash_bytes(relative.as_bytes()));
    let preferred_id = explicit_id.take().unwrap_or_else(|| fallback_id.clone());
    let valid_explicit_id = valid_id(&preferred_id);
    let id = if valid_explicit_id {
        preferred_id
    } else {
        losses.push(loss(
            "INVALID_FRONT_MATTER_ID",
            relative,
            1,
            "front matter id 无效；预览会提供确定的映射候选".into(),
            None,
        ));
        fallback_id
    };

    let body_start = if text.starts_with("---\n") || text.starts_with("---\r\n") {
        let mut offset = text.find('\n').map_or(0, |newline| newline + 1);
        let mut boundary = None;
        for line in text.split_inclusive('\n').skip(1) {
            offset += line.len();
            let content = line
                .strip_suffix('\n')
                .unwrap_or(line)
                .trim_end_matches('\r');
            if content == "---" || content == "..." {
                boundary = Some(offset);
                break;
            }
        }
        boundary.unwrap_or(text.len())
    } else {
        0
    };
    let body = &text[body_start..];
    let body_first_line = text[..body_start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count() as u32
        + 1;
    let mut rendered = Vec::new();
    let mut headings = Vec::new();
    let mut links = Vec::new();
    let mut attachments = Vec::new();
    let mut in_fence = false;
    let mut fence_start_line = 0u32;
    let mut first_heading_title = None;
    let mut prose = Vec::<String>::new();

    let flush_prose = |prose: &mut Vec<String>, rendered: &mut Vec<String>| {
        if !prose.is_empty() {
            rendered.push(prose.join(" "));
            prose.clear();
        }
    };
    for (body_line_index, raw) in body.lines().enumerate() {
        let line_number = body_first_line + body_line_index as u32;
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if !in_fence {
                fence_start_line = line_number;
                flush_prose(&mut prose, &mut rendered);
                losses.push(loss(
                    "UNSUPPORTED_CODE_BLOCK",
                    relative,
                    line_number,
                    "代码块只保留在原文副本中，不执行或转换".into(),
                    None,
                ));
                rendered.push("[未转换代码块；请查看原文副本]".into());
            }
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.is_empty() {
            flush_prose(&mut prose, &mut rendered);
            continue;
        }
        if let Some((level, heading_text)) = parse_atx_heading(trimmed) {
            flush_prose(&mut prose, &mut rendered);
            let (visible, references, formatting_loss) = parse_inline(heading_text, line_number);
            if formatting_loss {
                losses.push(loss(
                    "UNSUPPORTED_INLINE_MARKUP",
                    relative,
                    line_number,
                    "标题内行内标记被折叠为可见文字，原文副本保留原语法".into(),
                    None,
                ));
            }
            if level == 1 && first_heading_title.is_none() {
                first_heading_title = Some(visible.clone());
            }
            let slug = heading_slug(&visible);
            headings.push(Heading {
                slug,
                id: String::new(),
                title: visible.clone(),
                line: line_number,
            });
            rendered.push(visible);
            collect_references(
                references,
                relative,
                &mut links,
                &mut attachments,
                &mut losses,
            );
            continue;
        }
        if trimmed.starts_with('>') {
            flush_prose(&mut prose, &mut rendered);
            losses.push(loss(
                "UNSUPPORTED_BLOCK_QUOTE",
                relative,
                line_number,
                "该块结构只保留在原文副本中".into(),
                None,
            ));
            let quote_text = trimmed.trim_start_matches('>').trim_start();
            let (visible, references, formatting_loss) = parse_inline(quote_text, line_number);
            if formatting_loss {
                losses.push(loss(
                    "UNSUPPORTED_INLINE_MARKUP",
                    relative,
                    line_number,
                    "引用块内行内标记被折叠为可见文字，原文副本保留原语法".into(),
                    None,
                ));
            }
            rendered.push(visible);
            collect_references(
                references,
                relative,
                &mut links,
                &mut attachments,
                &mut losses,
            );
            continue;
        }
        if trimmed.starts_with('|') && trimmed.contains('|') {
            flush_prose(&mut prose, &mut rendered);
            losses.push(loss(
                "UNSUPPORTED_TABLE",
                relative,
                line_number,
                "表格结构会折叠为单行文字，原文副本保留原始单元格".into(),
                None,
            ));
            let mut cells = Vec::new();
            for cell in trimmed.trim_matches('|').split('|') {
                let (visible, references, formatting_loss) = parse_inline(cell.trim(), line_number);
                if formatting_loss {
                    losses.push(loss(
                        "UNSUPPORTED_INLINE_MARKUP",
                        relative,
                        line_number,
                        "表格单元格内行内标记被折叠为可见文字，原文副本保留原语法".into(),
                        None,
                    ));
                }
                cells.push(visible);
                collect_references(
                    references,
                    relative,
                    &mut links,
                    &mut attachments,
                    &mut losses,
                );
            }
            rendered.push(cells.join(" | "));
            continue;
        }
        if trimmed.starts_with('<') {
            flush_prose(&mut prose, &mut rendered);
            losses.push(loss(
                "RAW_HTML_ISOLATED",
                relative,
                line_number,
                "HTML 不会执行，原文只保留在隔离副本中".into(),
                None,
            ));
            rendered.push("[未转换 HTML；请查看原文副本]".into());
            continue;
        }
        if let Some(item_text) = strip_list_marker(trimmed) {
            flush_prose(&mut prose, &mut rendered);
            losses.push(loss(
                "UNSUPPORTED_LIST",
                relative,
                line_number,
                "列表层级会折叠为独立文字行，原文副本保留标记与缩进".into(),
                None,
            ));
            let (visible, references, formatting_loss) = parse_inline(item_text, line_number);
            if formatting_loss {
                losses.push(loss(
                    "UNSUPPORTED_INLINE_MARKUP",
                    relative,
                    line_number,
                    "列表项内行内标记被折叠为可见文字，原文副本保留原语法".into(),
                    None,
                ));
            }
            rendered.push(visible);
            collect_references(
                references,
                relative,
                &mut links,
                &mut attachments,
                &mut losses,
            );
            continue;
        }
        if is_horizontal_rule(trimmed) {
            flush_prose(&mut prose, &mut rendered);
            losses.push(loss(
                "UNSUPPORTED_HORIZONTAL_RULE",
                relative,
                line_number,
                "分隔线只保留在原文副本中".into(),
                None,
            ));
            continue;
        }
        let text_line = trimmed;
        let (visible, references, formatting_loss) = parse_inline(text_line, line_number);
        if formatting_loss {
            losses.push(loss(
                "UNSUPPORTED_INLINE_MARKUP",
                relative,
                line_number,
                "段落内行内标记被折叠为可见文字，原文副本保留原语法".into(),
                None,
            ));
        }
        prose.push(visible);
        collect_references(
            references,
            relative,
            &mut links,
            &mut attachments,
            &mut losses,
        );
    }
    flush_prose(&mut prose, &mut rendered);
    if in_fence {
        losses.push(loss(
            "UNCLOSED_CODE_BLOCK",
            relative,
            fence_start_line.max(1),
            "代码块未闭合；原文副本保留全部内容".into(),
            None,
        ));
    }
    if title.is_none() {
        title = first_heading_title;
    }
    if title.is_none() {
        title = Path::new(relative)
            .file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_owned);
    }
    let title = title
        .filter(|title| !title.trim().is_empty())
        .ok_or_else(|| format!("无法生成 Markdown 页面显示名：{relative}"))?;
    let description = rendered
        .into_iter()
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok(SourcePage {
        relative: relative.to_string(),
        bytes: bytes.to_vec(),
        id,
        invalid_front_matter_id: !valid_explicit_id,
        title,
        entity_type,
        description,
        headings,
        links,
        attachments,
        losses,
    })
}

pub(super) fn resolve_page_ids(
    current: &crate::CompileResult,
    pages: &mut [SourcePage],
    options: &MarkdownImportOptions,
    conflicts: &mut Vec<MarkdownImportConflict>,
) {
    let mut used = BTreeSet::new();
    for page in pages {
        let had_override = options.id_overrides.contains_key(&page.relative);
        if page.invalid_front_matter_id && !had_override {
            conflicts.push(MarkdownImportConflict {
                code: "INVALID_FRONT_MATTER_ID".into(),
                source: Some(page.relative.clone()),
                preferred_id: Some(page.id.clone()),
                candidates: vec![page.id.clone()],
                message: "front matter ID 无效；请通过 source-relative-path 显式确认替代 ID".into(),
            });
            continue;
        }
        let preferred = options
            .id_overrides
            .get(&page.relative)
            .cloned()
            .unwrap_or_else(|| page.id.clone());
        if !valid_id(&preferred) {
            conflicts.push(MarkdownImportConflict {
                code: "INVALID_ID_MAPPING".into(),
                source: Some(page.relative.clone()),
                preferred_id: Some(preferred.clone()),
                candidates: vec![format!("md_{:016x}", hash_bytes(page.relative.as_bytes()))],
                message: format!("目标 ID 不是有效的语言标识符：{preferred}"),
            });
            continue;
        }
        let target = TargetRef::new("entity", &preferred);
        let exists = current.analysis.catalog.object(&target).is_some();
        if exists || !used.insert(preferred.clone()) {
            let candidates = id_candidates(&preferred, current, &used);
            conflicts.push(MarkdownImportConflict {
                code: if exists {
                    "ENTITY_ID_CONFLICT".into()
                } else {
                    "DUPLICATE_IMPORT_ID".into()
                },
                source: Some(page.relative.clone()),
                preferred_id: Some(preferred.clone()),
                candidates,
                message: format!(
                    "ID `{preferred}` 已被占用；不会自动合并，请显式提供此来源页的 ID 映射"
                ),
            });
            continue;
        }
        page.id = preferred;
    }
}

fn id_candidates(
    preferred: &str,
    current: &crate::CompileResult,
    used: &BTreeSet<String>,
) -> Vec<String> {
    let mut candidates = Vec::new();
    for suffix in 2..=4 {
        let candidate = format!("{preferred}_import_{suffix}");
        if !used.contains(&candidate)
            && current
                .analysis
                .catalog
                .object(&TargetRef::new("entity", &candidate))
                .is_none()
        {
            candidates.push(candidate);
        }
    }
    candidates
}

fn parse_scalar(value: &str) -> Option<String> {
    if value.len() >= 2 {
        let first = value.as_bytes()[0];
        let last = *value.as_bytes().last()?;
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            let inner = &value[1..value.len() - 1];
            if first == b'"' && inner.contains('\\') {
                return None;
            }
            if first == b'\'' && inner.contains('\'') {
                return None;
            }
            return Some(inner.to_string());
        }
    }
    (!value.is_empty()
        && !value.starts_with(|character: char| {
            matches!(character, '[' | '{' | '&' | '*' | '!' | '|' | '>')
        }))
    .then(|| value.to_string())
}

fn parse_atx_heading(line: &str) -> Option<(usize, &str)> {
    let count = line.bytes().take_while(|byte| *byte == b'#').count();
    if count == 0
        || count > 6
        || !line
            .as_bytes()
            .get(count)
            .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        return None;
    }
    let text = line[count..].trim().trim_end_matches('#').trim_end();
    Some((count, text))
}

fn strip_list_marker(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'-' | b'+' | b'*') && bytes[1].is_ascii_whitespace()
    {
        return Some(line[2..].trim_start());
    }
    let digit_end = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digit_end > 0
        && bytes
            .get(digit_end)
            .is_some_and(|byte| matches!(byte, b'.' | b')'))
        && bytes
            .get(digit_end + 1)
            .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        return Some(line[digit_end + 2..].trim_start());
    }
    None
}

fn is_horizontal_rule(line: &str) -> bool {
    let marker = line
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    marker.len() >= 3
        && matches!(marker[0], b'-' | b'*' | b'_')
        && marker.iter().all(|byte| *byte == marker[0])
}

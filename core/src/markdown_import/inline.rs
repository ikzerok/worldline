use super::{hash_bytes, loss, InlineReference, MarkdownImportLoss};
use std::path::{Component, Path};

pub(super) fn parse_inline(input: &str, line: u32) -> (String, Vec<(InlineReference, bool)>, bool) {
    let mut output = String::new();
    let mut references = Vec::new();
    let link_delimiters = unescaped_positions(input, b"](");
    let close_parens = unescaped_positions(input, b")");
    let mut index = 0usize;
    let mut formatted = false;
    let mut code_delimiter = None;
    while index < input.len() {
        let rest = &input[index..];
        if rest.starts_with('`') && !is_escaped(input, index) {
            let width = rest.bytes().take_while(|byte| *byte == b'`').count();
            match code_delimiter {
                None => {
                    code_delimiter = Some(width);
                    formatted = true;
                    index += width;
                    continue;
                }
                Some(open_width) if width == open_width => {
                    code_delimiter = None;
                    index += width;
                    continue;
                }
                Some(_) => {
                    output.push_str(&rest[..width]);
                    index += width;
                    continue;
                }
            }
        }
        if code_delimiter.is_some() {
            let character = rest.chars().next().unwrap();
            output.push(character);
            index += character.len_utf8();
            continue;
        }
        if let Some(unescaped) = rest.strip_prefix('\\') {
            if let Some(escaped) = unescaped.chars().next() {
                if escaped.is_ascii_punctuation() {
                    output.push(escaped);
                    index += 1 + escaped.len_utf8();
                    continue;
                }
            }
        }
        let image = rest.starts_with("![");
        let opening = if image { index + 1 } else { index };
        if (image || rest.starts_with('[')) && !is_escaped(input, index) {
            if let Some((label, href, end)) =
                parse_link_at(input, opening, &link_delimiters, &close_parens)
            {
                let reference = InlineReference {
                    href: href.clone(),
                    label: label.clone(),
                    line,
                };
                references.push((reference, image));
                output.push_str(&label);
                index = end;
                continue;
            } else {
                formatted = true;
            }
        }
        let character = rest.chars().next().unwrap();
        if character == '<' && rest.contains('>') {
            formatted = true;
        }
        if matches!(character, '*' | '_') {
            formatted = true;
            let marker_width = if rest.starts_with("**") || rest.starts_with("__") {
                2
            } else {
                1
            };
            if rest.len() >= marker_width * 2 {
                index += marker_width;
                continue;
            }
        }
        output.push(character);
        index += character.len_utf8();
    }
    (output, references, formatted)
}

fn parse_link_at(
    input: &str,
    opening: usize,
    link_delimiters: &[usize],
    close_parens: &[usize],
) -> Option<(String, String, usize)> {
    let closing_label = next_after(link_delimiters, opening)?;
    let closing_href = next_after(close_parens, closing_label + 1)?;
    let label = input[opening + 1..closing_label].to_string();
    let href = input[closing_label + 2..closing_href]
        .split_once(char::is_whitespace)
        .map(|(href, _)| href)
        .unwrap_or(&input[closing_label + 2..closing_href])
        .trim_matches(['<', '>'])
        .to_string();
    Some((label, href, closing_href + 1))
}

fn unescaped_positions(input: &str, marker: &[u8]) -> Vec<usize> {
    let bytes = input.as_bytes();
    let mut positions = Vec::new();
    let mut index = 0;
    let mut backslashes = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            backslashes += 1;
            index += 1;
            continue;
        }
        if backslashes.is_multiple_of(2) && bytes[index..].starts_with(marker) {
            positions.push(index);
        }
        backslashes = 0;
        let character = input[index..].chars().next().unwrap();
        index += character.len_utf8();
    }
    positions
}

fn next_after(positions: &[usize], index: usize) -> Option<usize> {
    positions
        .get(positions.partition_point(|position| *position <= index))
        .copied()
}

fn is_escaped(value: &str, index: usize) -> bool {
    value.as_bytes()[..index]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

pub(super) fn collect_references(
    references: Vec<(InlineReference, bool)>,
    _source: &str,
    links: &mut Vec<InlineReference>,
    attachments: &mut Vec<InlineReference>,
    losses: &mut Vec<MarkdownImportLoss>,
) {
    for (reference, image) in references {
        if image {
            attachments.push(reference);
        } else if is_markdown_link(&reference.href) || has_uri_scheme(&reference.href) {
            links.push(reference);
        } else if reference.href.is_empty() {
            losses.push(loss(
                "UNSUPPORTED_LINK_SYNTAX",
                _source,
                reference.line,
                "Markdown 链接目标为空，保留在原文副本".into(),
                None,
            ));
        } else {
            // 普通文件链接也作为附件预览；脚本会在目标分类阶段隔离。
            attachments.push(reference);
        }
    }
}

pub(super) enum Href {
    Local {
        path: String,
        fragment: Option<String>,
    },
    External,
    Unsafe,
    Malformed,
}

pub(super) fn resolve_relative_href(source: &str, href: &str) -> Href {
    let decoded = match percent_decode(href.trim()) {
        Some(value) => value,
        None => return Href::Malformed,
    };
    let href = decoded.as_str();
    if has_uri_scheme(href) {
        return Href::External;
    }
    if href.starts_with('/')
        || href.starts_with('\\')
        || href.as_bytes().get(1) == Some(&b':')
        || href.starts_with("//")
    {
        return Href::Unsafe;
    }
    let (without_fragment, fragment) = match href.split_once('#') {
        Some((path, fragment)) => (path, Some(normalize_fragment(fragment))),
        None => (href, None),
    };
    if without_fragment.contains('?') {
        return Href::Malformed;
    }
    let parent = Path::new(source).parent().unwrap_or_else(|| Path::new(""));
    let mut components = Vec::new();
    for component in parent.join(without_fragment).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => {
                let Some(value) = value.to_str() else {
                    return Href::Malformed;
                };
                components.push(value.to_string());
            }
            Component::ParentDir if !components.is_empty() => {
                components.pop();
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Href::Unsafe
            }
        }
    }
    let path = components.join("/");
    if path.is_empty() && fragment.is_none() {
        return Href::Malformed;
    }
    if path.is_empty() {
        return Href::Local {
            path: source.to_string(),
            fragment,
        };
    }
    Href::Local { path, fragment }
}

fn normalize_fragment(value: &str) -> String {
    heading_slug(value.trim().trim_start_matches('#'))
}

fn has_uri_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn is_markdown_link(href: &str) -> bool {
    if href.starts_with('#') {
        return true;
    }
    let decoded = percent_decode(href).unwrap_or_else(|| href.to_string());
    let path = decoded.split(['#', '?']).next().unwrap_or_default();
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            decoded.push((hex_value(high)? << 4) | hex_value(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(super) fn heading_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(character.to_ascii_lowercase());
        } else if character.is_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.extend(character.to_lowercase());
        } else if character.is_whitespace() || character == '-' {
            pending_dash = true;
        }
    }
    if slug.is_empty() {
        format!("h_{:016x}", hash_bytes(value.as_bytes()))
    } else {
        slug
    }
}

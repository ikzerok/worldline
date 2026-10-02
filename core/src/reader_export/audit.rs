use super::*;
use std::path::{Component, Path};

pub(super) fn audit(
    files: &BTreeMap<PathBuf, Vec<u8>>,
    progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
) -> Result<(), String> {
    let mut ids = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    let mut references = Vec::<(PathBuf, String)>::new();
    for (index, (path, bytes)) in files.iter().enumerate() {
        super::progress::report(progress, "audit", index, files.len())?;
        super::site::validate_output_path(path)?;
        let extension = path.extension().and_then(|ext| ext.to_str());
        if !matches!(extension, Some("html" | "css")) {
            continue;
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "生成文本不是有效 UTF-8")?;
        if extension == Some("css") {
            for url in css_urls(text)? {
                references.push((path.clone(), url));
            }
            continue;
        }
        let mut page_ids = BTreeSet::new();
        for (name, attributes) in tags(text)? {
            if matches!(
                name.as_str(),
                "iframe" | "object" | "embed" | "base" | "style"
            ) {
                return Err(format!("阅读包含有禁止的 HTML 元素：{name}"));
            }
            for (key, value) in attributes {
                if key.starts_with("on") {
                    return Err("阅读包不允许内联事件处理器".into());
                }
                if key == "id" && !page_ids.insert(value.clone()) {
                    return Err("生成页面包含重复 anchor".into());
                }
                if matches!(key.as_str(), "href" | "src" | "xlink:href") {
                    references.push((path.clone(), value.clone()));
                }
                if key == "style" || key == "clip-path" || key == "fill" || key == "stroke" {
                    for url in css_urls(&value)? {
                        references.push((path.clone(), url));
                    }
                }
            }
        }
        ids.insert(path.clone(), page_ids);
    }
    for (from, url) in references {
        let (path, anchor) = resolve(&from, &url)?;
        if !files.contains_key(&path) {
            return Err(format!("阅读包链接资源不存在：{}", path.display()));
        }
        if let Some(anchor) = anchor {
            if !ids.get(&path).is_some_and(|ids| ids.contains(&anchor)) {
                return Err("阅读包链接的片段 anchor 不存在".into());
            }
        }
    }
    Ok(())
}

fn resolve(from: &Path, url: &str) -> Result<(PathBuf, Option<String>), String> {
    if url.is_empty()
        || url.starts_with('/')
        || url.contains(['\\', ':', '?', '%', '&'])
        || url.chars().any(char::is_control)
    {
        return Err("阅读包链接必须是安全的包内相对地址".into());
    }
    let (relative, anchor) = url
        .split_once('#')
        .map(|(path, id)| (path, Some(id.to_owned())))
        .unwrap_or((url, None));
    let mut parts: Vec<_> = if relative.is_empty() {
        from.components()
            .filter_map(|part| {
                if let Component::Normal(value) = part {
                    Some(value.to_os_string())
                } else {
                    None
                }
            })
            .collect()
    } else {
        from.parent()
            .unwrap_or(Path::new(""))
            .components()
            .filter_map(|part| {
                if let Component::Normal(value) = part {
                    Some(value.to_os_string())
                } else {
                    None
                }
            })
            .collect()
    };
    if !relative.is_empty() {
        for part in relative.split('/') {
            match part {
                "" | "." => return Err("阅读包链接含有空路径或当前目录段".into()),
                ".." => {
                    if parts.pop().is_none() {
                        return Err("阅读包链接越出包目录".into());
                    }
                }
                value => parts.push(value.into()),
            }
        }
    }
    let path: PathBuf = parts.into_iter().collect();
    super::site::validate_output_path(&path)?;
    if anchor
        .as_ref()
        .is_some_and(|id| id.is_empty() || id.contains('#'))
    {
        return Err("阅读包片段地址无效".into());
    }
    Ok((path, anchor))
}

fn css_urls(text: &str) -> Result<Vec<String>, String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("@import") {
        return Err("阅读包样式不允许导入外部样式".into());
    }
    let mut rest = lower.as_str();
    let mut offset = 0;
    let mut result = Vec::new();
    while let Some(start) = rest.find("url(") {
        let value_start = offset + start + 4;
        let end = text[value_start..].find(')').ok_or("样式 url 缺少闭合")? + value_start;
        result.push(
            text[value_start..end]
                .trim()
                .trim_matches(['\'', '"'])
                .to_owned(),
        );
        offset = end + 1;
        rest = &lower[offset..];
    }
    Ok(result)
}

type Attributes = Vec<(String, String)>;
fn tags(text: &str) -> Result<Vec<(String, Attributes)>, String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut result = Vec::new();
    while index < bytes.len() {
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        index += 1;
        if matches!(bytes.get(index), Some(b'!' | b'/' | b'?')) {
            while index < bytes.len() && bytes[index] != b'>' {
                index += 1;
            }
            continue;
        }
        let start = index;
        while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'-')
        {
            index += 1;
        }
        if start == index {
            return Err("生成 HTML 标签无效".into());
        }
        let name = text[start..index].to_ascii_lowercase();
        let mut attributes = Vec::new();
        while index < bytes.len() {
            while index < bytes.len()
                && (bytes[index].is_ascii_whitespace() || bytes[index] == b'/')
            {
                index += 1;
            }
            if bytes.get(index) == Some(&b'>') {
                index += 1;
                break;
            }
            let start = index;
            while index < bytes.len()
                && !bytes[index].is_ascii_whitespace()
                && !matches!(bytes[index], b'=' | b'>' | b'/')
            {
                index += 1;
            }
            if start == index {
                return Err("生成 HTML 属性无效".into());
            }
            let key = text[start..index].to_ascii_lowercase();
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if bytes.get(index) != Some(&b'=') {
                continue;
            }
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            let quote = bytes
                .get(index)
                .copied()
                .filter(|b| matches!(b, b'\'' | b'"'));
            if quote.is_some() {
                index += 1;
            }
            let start = index;
            while index < bytes.len()
                && match quote {
                    Some(quote) => bytes[index] != quote,
                    None => !bytes[index].is_ascii_whitespace() && bytes[index] != b'>',
                }
            {
                index += 1;
            }
            if index == bytes.len() {
                return Err("生成 HTML 属性未闭合".into());
            }
            let value = text[start..index].to_owned();
            if quote.is_some() {
                index += 1;
            }
            attributes.push((key, value));
        }
        result.push((name, attributes));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_external_escape_missing_and_fragment() {
        for href in [
            "https://example.org/x",
            "../../outside",
            "missing.html",
            "index.html#missing",
        ] {
            let files = BTreeMap::from([(
                PathBuf::from("index.html"),
                format!("<a href=\"{href}\">公开</a>").into_bytes(),
            )]);
            assert!(audit(&files, &mut |_| true).is_err());
        }
    }
    #[test]
    fn accepts_deep_relative_links_and_local_fragments() {
        let files = BTreeMap::from([
            (
                PathBuf::from("index.html"),
                b"<a href=\"objects/page.html#entry\">read</a>".to_vec(),
            ),
            (
                PathBuf::from("objects/page.html"),
                b"<div id=\"entry\"><a href=\"../index.html\">home</a></div>".to_vec(),
            ),
        ]);
        assert!(audit(&files, &mut |_| true).is_ok());
    }

    #[test]
    fn rejects_inline_styles_even_when_the_renderer_does_not_create_them() {
        for body in [
            "<style>@import 'https://example.org/private.css';</style>",
            "<style>body{background:url(https://example.org/private)}</style>",
        ] {
            let files = BTreeMap::from([(PathBuf::from("index.html"), body.as_bytes().to_vec())]);
            assert!(audit(&files, &mut |_| true).is_err());
        }
    }
}

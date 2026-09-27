use super::*;
use serde::Serialize;
use std::path::{Component, Path};

pub(super) fn render_package(
    prepared: PreparedExport,
) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    #[derive(Serialize)]
    struct PublicSearchEntry<'a> {
        title: &'a str,
        url: String,
        text: &'a str,
    }
    #[derive(Serialize)]
    struct PublicManifestEntry<'a> {
        title: &'a str,
        url: String,
    }
    #[derive(Serialize)]
    struct PublicManifest<'a> {
        schema_version: u32,
        title: &'a str,
        pages: Vec<PublicManifestEntry<'a>>,
        attachments: Vec<PublicManifestEntry<'a>>,
    }

    let mut files = BTreeMap::new();
    let public_pages: Vec<_> = prepared
        .pages
        .iter()
        .map(|page| PublicManifestEntry {
            title: &page.title,
            url: page.output_path.to_string_lossy().into_owned(),
        })
        .collect();
    let public_attachments: Vec<_> = prepared
        .attachments
        .iter()
        .map(|attachment| PublicManifestEntry {
            title: &attachment.display,
            url: attachment.output_path.to_string_lossy().into_owned(),
        })
        .collect();
    let manifest = PublicManifest {
        schema_version: READER_EXPORT_SCHEMA_VERSION,
        title: &prepared.site_title,
        pages: public_pages,
        attachments: public_attachments,
    };
    insert_output(
        &mut files,
        "reader-manifest.json",
        json_for_script(&manifest)?.into_bytes(),
    )?;

    let mut search_entries = Vec::new();
    for page in &prepared.pages {
        search_entries.push(PublicSearchEntry {
            title: &page.title,
            url: page.output_path.to_string_lossy().into_owned(),
            text: &page.searchable_text,
        });
    }
    let search_json = json_for_script(&search_entries)?;
    insert_output(
        &mut files,
        "search-index.json",
        search_json.clone().into_bytes(),
    )?;
    let js_data = json_for_script(&search_entries)?;
    insert_output(
        &mut files,
        "search-data.js",
        format!("window.READER_SEARCH_DATA={js_data};\n").into_bytes(),
    )?;
    insert_output(&mut files, "reader.js", READER_JS.as_bytes().to_vec())?;
    insert_output(&mut files, "style.css", READER_CSS.as_bytes().to_vec())?;

    for page in &prepared.pages {
        let path = page.output_path.to_string_lossy();
        let stylesheet = relative_url(path.as_ref(), "style.css");
        let home = relative_url(path.as_ref(), "index.html");
        let search = relative_url(path.as_ref(), "search.html");
        let html = format!(
            "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><link rel=\"stylesheet\" href=\"{}\"></head><body><header><a href=\"{}\">{}</a> · <a href=\"{}\">搜索</a></header><main><h1>{}</h1>{}</main></body></html>",
            html_escape(&page.title),
            html_escape(&stylesheet),
            html_escape(&home),
            html_escape(&prepared.site_title),
            html_escape(&search),
            html_escape(&page.title),
            page.body_html
        );
        insert_output(&mut files, &page.output_path, html.into_bytes())?;
    }

    let object_pages: Vec<_> = prepared
        .pages
        .iter()
        .filter(|page| page.output_path.starts_with("objects"))
        .collect();
    let manuscript_pages: Vec<_> = prepared
        .pages
        .iter()
        .filter(|page| page.output_path.starts_with("manuscripts"))
        .collect();
    let attachments_html = prepared
        .attachments
        .iter()
        .map(|attachment| {
            let path = attachment.output_path.to_string_lossy();
            format!(
                "<li><a href=\"{}\">{}</a></li>",
                html_escape(&path),
                html_escape(&attachment.display)
            )
        })
        .collect::<String>();
    let index_html = format!(
        "{}<h1>{}</h1>{}<h2>资料对象</h2>{}<h2>书稿章节</h2>{}<h2>附件</h2><ul>{}</ul>",
        page_start(&prepared.site_title, "index.html", false),
        html_escape(&prepared.site_title),
        search_link(),
        page_links(&object_pages, "index.html"),
        page_links(&manuscript_pages, "index.html"),
        attachments_html
    );
    insert_output(&mut files, "index.html", page_end(index_html).into_bytes())?;
    insert_output(
        &mut files,
        "objects/index.html",
        page_end(format!(
            "{}<h1>资料对象</h1>{}",
            page_start(&prepared.site_title, "objects/index.html", true),
            page_links(&object_pages, "objects/index.html")
        ))
        .into_bytes(),
    )?;
    insert_output(
        &mut files,
        "manuscripts/index.html",
        page_end(format!(
            "{}<h1>书稿章节</h1>{}",
            page_start(&prepared.site_title, "manuscripts/index.html", true),
            page_links(&manuscript_pages, "manuscripts/index.html")
        ))
        .into_bytes(),
    )?;
    insert_output(
        &mut files,
        "search.html",
        page_end(format!(
            "{}<h1>搜索公开内容</h1><label>搜索 <input id=\"query\" type=\"search\" autocomplete=\"off\"></label><ul id=\"results\"></ul><script src=\"search-data.js\"></script><script src=\"reader.js\"></script>",
            page_start(&prepared.site_title, "search.html", false)
        ))
        .into_bytes(),
    )?;

    for attachment in prepared.attachments {
        insert_output(&mut files, attachment.output_path, attachment.bytes)?;
    }
    let total_bytes: usize = files.values().map(Vec::len).sum();
    if total_bytes > MAX_PACKAGE_BYTES {
        return Err("阅读包超过 128 MiB 限制".into());
    }
    Ok(files)
}

fn page_start(site_title: &str, current_path: &str, nested_index: bool) -> String {
    let css = if nested_index {
        "../style.css"
    } else {
        "style.css"
    };
    let home = if nested_index {
        "../index.html"
    } else {
        "index.html"
    };
    format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><link rel=\"stylesheet\" href=\"{}\"></head><body><header><a href=\"{}\">{}</a> · <a href=\"{}\">搜索</a></header><main>",
        html_escape(site_title),
        css,
        home,
        html_escape(site_title),
        relative_url(current_path, "search.html")
    )
}

fn page_end(body: String) -> String {
    format!("{body}</main></body></html>")
}

fn search_link() -> &'static str {
    "<p><a href=\"search.html\">搜索公开内容</a></p>"
}

fn page_links(pages: &[&PublicPage], from_path: &str) -> String {
    let items = pages
        .iter()
        .map(|page| {
            format!(
                "<li><a href=\"{}\">{}</a></li>",
                html_escape(&relative_url(from_path, &page.output_path,)),
                html_escape(&page.title)
            )
        })
        .collect::<String>();
    format!("<ul>{items}</ul>")
}

fn json_for_script<T: Serialize>(value: &T) -> Result<String, String> {
    let json = serde_json::to_string(value).map_err(|error| error.to_string())?;
    Ok(json
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029"))
}

fn insert_output(
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
    path: impl Into<PathBuf>,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let path = path.into();
    validate_output_path(&path)?;
    if files.insert(path.clone(), bytes).is_some() {
        return Err(format!("阅读包内部输出路径冲突：{}", path.display()));
    }
    Ok(())
}

pub(super) fn validate_output_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!("阅读包输出路径无效：{}", path.display()));
    }
    Ok(())
}

pub(super) fn relative_url(from_file: impl AsRef<Path>, to_file: impl AsRef<Path>) -> String {
    let from_parent: Vec<_> = from_file
        .as_ref()
        .parent()
        .unwrap_or(Path::new(""))
        .components()
        .filter_map(|part| match part {
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let target: Vec<_> = to_file
        .as_ref()
        .components()
        .filter_map(|part| match part {
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let common = from_parent
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = vec!["..".to_string(); from_parent.len() - common];
    parts.extend(target.into_iter().skip(common));
    parts.join("/")
}

pub(super) fn html_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

const READER_CSS: &str = "body{font:1rem/1.7 system-ui,sans-serif;max-width:68rem;margin:0 auto;padding:1rem;color:#222}header{border-bottom:1px solid #ddd;padding:.5rem 0}main{padding:1rem 0}.unavailable{color:#666;font-style:italic}.choice{margin:.5rem 0;padding:.5rem;border-left:3px solid #bbb}a{color:#174ea6}";

const READER_JS: &str = r#"(() => {
const input = document.getElementById('query');
const list = document.getElementById('results');
if (!input || !list) return;
const entries = window.READER_SEARCH_DATA || [];
input.addEventListener('input', () => {
  const query = input.value.trim().toLocaleLowerCase();
  list.replaceChildren();
  if (!query) return;
  for (const entry of entries) {
    if (!(entry.title + ' ' + entry.text).toLocaleLowerCase().includes(query)) continue;
    const item = document.createElement('li');
    const link = document.createElement('a');
    link.href = entry.url;
    link.textContent = entry.title;
    const excerpt = document.createElement('p');
    excerpt.textContent = entry.text;
    item.append(link, excerpt);
    list.append(item);
  }
});
})();
"#;

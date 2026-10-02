use super::*;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Component, Path};

pub(super) fn render_package(
    prepared: PreparedExport,
    progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let mut search = Vec::<Value>::new();
    for (index, page) in prepared.pages.iter().enumerate() {
        super::progress::report(progress, "render", index, prepared.pages.len())?;
        let url = super::portable_output_path(&page.output_path)?;
        let mut entry = json!({"title":page.title,"url":url,"text":page.searchable_text});
        if prepared.world_site {
            entry["kind"] = json!(page.kind);
            entry["aliases"] = json!(page.aliases);
            for anchor in &page.anchors {
                search.push(
                    json!({"title":anchor.label,"url":format!("{url}#{}",anchor.id),
                    "text":anchor.text,"kind":"map_placement","aliases":[]}),
                );
            }
        }
        search.push(entry);
        insert_output(
            &mut files,
            &page.output_path,
            document(
                &prepared.site_title,
                &page.title,
                &url,
                &page.body_html,
                prepared.world_site,
            )
            .into_bytes(),
        )?;
    }
    let objects: Vec<_> = prepared
        .pages
        .iter()
        .filter(|page| page.output_path.starts_with("objects"))
        .collect();
    let chapters: Vec<_> = prepared
        .pages
        .iter()
        .filter(|page| page.output_path.starts_with("manuscripts"))
        .collect();
    let maps: Vec<_> = prepared
        .pages
        .iter()
        .filter(|page| page.output_path.starts_with("maps"))
        .collect();
    for (path, title, entries) in [
        ("objects/index.html", "资料对象", &objects),
        ("manuscripts/index.html", "书稿章节", &chapters),
        ("maps/index.html", "地图", &maps),
    ] {
        insert_output(
            &mut files,
            path,
            document(
                &prepared.site_title,
                title,
                path,
                &page_links(entries, path),
                prepared.world_site,
            )
            .into_bytes(),
        )?;
    }
    let mut home = String::new();
    if prepared.world_site {
        home.push_str("<p class=\"lede\">浏览这个世界的公开资料、时间结构、地图与静态故事。</p><div class=\"category-grid\">");
        let kinds: BTreeSet<_> = objects.iter().map(|page| page.kind.as_str()).collect();
        for kind in kinds {
            let entries: Vec<_> = objects
                .iter()
                .copied()
                .filter(|page| page.kind == kind)
                .collect();
            let path = format!("objects/kind-{kind}.html");
            let title = super::semantics::kind_label(kind);
            let body = page_links(&entries, &path);
            insert_output(
                &mut files,
                &path,
                document(&prepared.site_title, title, &path, &body, true).into_bytes(),
            )?;
            home.push_str(&format!("<a class=\"category\" href=\"{path}\"><strong>{}</strong><span>{} 项公开内容</span></a>", html_escape(title), entries.len()));
        }
        home.push_str("</div>");
        for (path, title, count) in [
            ("manuscripts/index.html", "书稿章节", chapters.len()),
            ("maps/index.html", "地图", maps.len()),
        ] {
            home.push_str(&format!(
                "<p><a href=\"{path}\">{title}</a> · {count} 项</p>"
            ));
        }
    } else {
        home.push_str(&format!(
            "<h2>资料对象</h2>{}<h2>书稿章节</h2>{}<h2>地图</h2>{}",
            page_links(&objects, "index.html"),
            page_links(&chapters, "index.html"),
            page_links(&maps, "index.html")
        ));
    }
    home.push_str("<h2>公开附件</h2><ul>");
    for attachment in &prepared.attachments {
        home.push_str(&format!(
            "<li><a href=\"{}\">{}</a></li>",
            html_escape(&super::portable_output_path(&attachment.output_path)?),
            html_escape(&attachment.display)
        ));
    }
    home.push_str("</ul>");
    insert_output(
        &mut files,
        "index.html",
        document(
            &prepared.site_title,
            &prepared.site_title,
            "index.html",
            &home,
            prepared.world_site,
        )
        .into_bytes(),
    )?;
    let mut filters = String::new();
    if prepared.world_site {
        filters
            .push_str("<label>内容类型 <select id=\"kind\"><option value=\"\">全部类型</option>");
        let kinds: BTreeSet<_> = search
            .iter()
            .filter_map(|entry| entry["kind"].as_str())
            .collect();
        for kind in kinds {
            let label = if kind == "map_placement" {
                "地图图元"
            } else {
                super::semantics::kind_label(kind)
            };
            filters.push_str(&format!(
                "<option value=\"{}\">{}</option>",
                html_escape(kind),
                html_escape(label)
            ));
        }
        filters.push_str("</select></label>");
    }
    let body = format!("<div class=\"search-controls\"><label>搜索 <input id=\"query\" type=\"search\" autocomplete=\"off\" placeholder=\"正文、别名或公开字段\"></label>{filters}</div><p id=\"search-status\" aria-live=\"polite\"></p><ul id=\"results\"></ul><script src=\"search-data.js\"></script>");
    insert_output(
        &mut files,
        "search.html",
        document(
            &prepared.site_title,
            "搜索公开内容",
            "search.html",
            &body,
            prepared.world_site,
        )
        .into_bytes(),
    )?;
    let search_json = json_for_script(&search)?;
    insert_output(
        &mut files,
        "search-index.json",
        search_json.as_bytes().to_vec(),
    )?;
    insert_output(
        &mut files,
        "search-data.js",
        format!("window.READER_SEARCH_DATA={search_json};\n").into_bytes(),
    )?;
    insert_output(
        &mut files,
        "reader.js",
        super::site_assets::JS.as_bytes().to_vec(),
    )?;
    insert_output(
        &mut files,
        "style.css",
        super::site_assets::CSS.as_bytes().to_vec(),
    )?;
    let pages: Vec<_> = prepared
        .pages
        .iter()
        .map(|page| {
            Ok(json!({"title":page.title,"url":super::portable_output_path(&page.output_path)?}))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let attachments: Vec<_> = prepared
        .attachments
        .iter()
        .map(|asset| Ok(json!({"title":asset.display,"url":super::portable_output_path(&asset.output_path)?})))
        .collect::<Result<Vec<_>, String>>()?;
    for (index, attachment) in prepared.attachments.into_iter().enumerate() {
        super::progress::report(progress, "render", index, attachments.len())?;
        insert_output(&mut files, attachment.output_path, attachment.bytes)?;
    }
    let resources: Vec<_> = files.iter().map(|(path, bytes)| Ok(json!({"path":super::portable_output_path(path)?,"bytes":bytes.len(),"hash":super::routes::hash_bytes(bytes)}))).collect::<Result<Vec<_>, String>>()?;
    let manifest = json!({"schema_version":if prepared.world_site {READER_SITE_SCHEMA_VERSION} else {READER_EXPORT_SCHEMA_VERSION},
        "title":prepared.site_title,"pages":pages,"attachments":attachments,"hash_algorithm":"fnv1a64","resources":resources});
    insert_output(
        &mut files,
        "reader-manifest.json",
        json_for_script(&manifest)?.into_bytes(),
    )?;
    super::audit::audit(&files, progress)?;
    Ok(files)
}

fn document(site_title: &str, title: &str, path: &str, body: &str, world_site: bool) -> String {
    let mut navigation = String::new();
    for (destination, label) in [
        ("index.html", "首页"),
        ("objects/index.html", "资料"),
        ("manuscripts/index.html", "书稿"),
        ("maps/index.html", "地图"),
        ("timeline.html", "时间结构"),
        ("relations.html", "关系"),
        ("stories.html", "故事"),
        ("search.html", "搜索"),
    ] {
        if !world_site
            && matches!(
                destination,
                "timeline.html" | "relations.html" | "stories.html"
            )
        {
            continue;
        }
        navigation.push_str(&format!(
            "<a href=\"{}\">{label}</a>",
            html_escape(&relative_url(path, destination))
        ));
    }
    format!("<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><link rel=\"stylesheet\" href=\"{}\"></head><body><header><a class=\"site-title\" href=\"{}\">{}</a><nav aria-label=\"站点导航\">{navigation}</nav></header><main><h1>{}</h1>{body}</main><footer>公开世界资料 · 静态阅读</footer><script src=\"{}\"></script></body></html>",
        html_escape(title), html_escape(&relative_url(path, "style.css")), html_escape(&relative_url(path, "index.html")),
        html_escape(site_title), html_escape(title), html_escape(&relative_url(path, "reader.js")))
}

fn page_links(pages: &[&PublicPage], from: &str) -> String {
    let mut html = String::from("<ul class=\"page-list\">");
    for page in pages {
        html.push_str(&format!(
            "<li><a href=\"{}\">{}</a></li>",
            html_escape(&relative_url(from, &page.output_path)),
            html_escape(&page.title)
        ));
    }
    html.push_str("</ul>");
    html
}
pub(super) fn json_for_script<T: Serialize>(value: &T) -> Result<String, String> {
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
    let path = PathBuf::from(super::portable_output_path(&path)?);
    let total = files
        .values()
        .try_fold(bytes.len(), |total, value| total.checked_add(value.len()))
        .ok_or("阅读包大小超出限制")?;
    if total > MAX_PACKAGE_BYTES || files.len() >= MAX_OUTPUT_FILES {
        return Err("阅读包超过 128 MiB 或 10000 个文件限制".into());
    }
    if files.insert(path.clone(), bytes).is_some() {
        return Err(format!("阅读包内部输出路径冲突：{}", path.display()));
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

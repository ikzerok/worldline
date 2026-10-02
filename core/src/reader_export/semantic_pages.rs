use super::semantics::link;
use super::site::html_escape;
use super::*;

pub(super) fn append_world_pages(
    compiled: &CompileResult,
    selection: &ReaderExportSelection,
    routes: &BTreeMap<TargetRef, String>,
    pages: &mut Vec<PublicPage>,
) -> Result<(), String> {
    pages.push(timeline(compiled, routes));
    let mut relations = page("公开关系", "relations.html", "relations");
    for target in selection
        .objects
        .iter()
        .filter(|target| target.kind == "relation")
    {
        append_link(&mut relations, compiled, routes, target);
    }
    relations
        .body_html
        .push_str(&relation_graph(compiled, routes, None, "relations.html"));
    relations.empty_content = relations.searchable_text.trim().is_empty();
    pages.push(relations);
    let mut stories = page("静态故事目录", "stories.html", "stories");
    stories
        .body_html
        .push_str("<p>静态分支阅读，不执行条件或状态</p>");
    stories
        .searchable_text
        .push_str("静态分支阅读，不执行条件或状态\n");
    for target in selection
        .objects
        .iter()
        .filter(|target| matches!(target.kind.as_str(), "event" | "scene" | "fragment"))
    {
        append_link(&mut stories, compiled, routes, target);
    }
    stories.empty_content = selection
        .objects
        .iter()
        .all(|target| !matches!(target.kind.as_str(), "event" | "scene" | "fragment"));
    pages.push(stories);
    Ok(())
}

fn page(title: &str, path: &str, kind: &str) -> PublicPage {
    PublicPage {
        title: title.into(),
        output_path: path.into(),
        kind: kind.into(),
        body_html: String::new(),
        searchable_text: String::new(),
        aliases: Vec::new(),
        anchors: Vec::new(),
        empty_content: true,
    }
}

fn append_link(
    page: &mut PublicPage,
    compiled: &CompileResult,
    routes: &BTreeMap<TargetRef, String>,
    target: &TargetRef,
) {
    let (html, plain) = link(
        compiled,
        routes,
        &page.output_path.to_string_lossy(),
        target,
    );
    page.body_html.push_str(&format!("<p>{html}</p>"));
    page.searchable_text.push_str(&format!("{plain}\n"));
}

fn timeline(compiled: &CompileResult, routes: &BTreeMap<TargetRef, String>) -> PublicPage {
    let mut page = page("时间结构与显式先后关系", "timeline.html", "timeline");
    page.body_html
        .push_str("<p>仅展示公开时段层级与显式偏序关系；不推断完整时间顺序。</p>");
    for period in &compiled.program.periods {
        let target = TargetRef::new("period", &period.name);
        if !routes.contains_key(&target) {
            continue;
        }
        let (html, plain) = link(compiled, routes, "timeline.html", &target);
        page.body_html
            .push_str(&format!("<section><h2>{html}</h2>"));
        page.searchable_text.push_str(&format!("{plain}\n"));
        if let Some(parent) = &period.parent {
            let (html, plain) = link(
                compiled,
                routes,
                "timeline.html",
                &TargetRef::new("period", parent),
            );
            page.body_html.push_str(&format!("<p>父时段：{html}</p>"));
            page.searchable_text.push_str(&format!("父时段：{plain}\n"));
        }
        for event in compiled
            .program
            .events
            .iter()
            .filter(|event| event.period.as_deref() == Some(&period.name))
        {
            let target = TargetRef::new("event", &event.name);
            if routes.contains_key(&target) {
                append_link(&mut page, compiled, routes, &target);
            }
        }
        page.body_html.push_str("</section>");
    }
    page.body_html.push_str("<section><h2>其他公开事件</h2>");
    for event in &compiled.program.events {
        let target = TargetRef::new("event", &event.name);
        if !routes.contains_key(&target) {
            continue;
        }
        let public_period = event
            .period
            .as_ref()
            .is_some_and(|id| routes.contains_key(&TargetRef::new("period", id)));
        if !public_period {
            append_link(&mut page, compiled, routes, &target);
        }
    }
    page.body_html
        .push_str("</section><section><h2>显式先后关系</h2>");
    for event in &compiled.program.events {
        let target = TargetRef::new("event", &event.name);
        if !routes.contains_key(&target) {
            continue;
        }
        for predecessor in &event.predecessors {
            let predecessor = TargetRef::new("event", predecessor);
            if !routes.contains_key(&predecessor) {
                continue;
            }
            let (before_html, before_text) = link(compiled, routes, "timeline.html", &predecessor);
            let (after_html, after_text) = link(compiled, routes, "timeline.html", &target);
            page.body_html
                .push_str(&format!("<p>{before_html} → {after_html}</p>"));
            page.searchable_text
                .push_str(&format!("{before_text} → {after_text}\n"));
        }
    }
    page.body_html.push_str("</section>");
    page.empty_content = page.searchable_text.trim().is_empty();
    page
}

pub(super) fn relation_graph(
    compiled: &CompileResult,
    routes: &BTreeMap<TargetRef, String>,
    focus: Option<&TargetRef>,
    current: &str,
) -> String {
    let relations: Vec<_> = compiled
        .analysis
        .catalog
        .relations
        .values()
        .filter(|relation| {
            if focus.is_some_and(|target| {
                relation.from_ref != *target
                    && relation.to_ref != *target
                    && !(target.kind == "relation" && target.id == relation.id)
            }) {
                return false;
            }
            routes.contains_key(&TargetRef::new("relation", &relation.id))
                && routes.contains_key(&relation.from_ref)
                && routes.contains_key(&relation.to_ref)
        })
        .collect();
    if relations.is_empty() {
        return String::new();
    }
    let height = relations.len().saturating_mul(80).saturating_add(30);
    let mut html = format!("<section><h2>公开关系局部图</h2><svg role=\"img\" aria-label=\"公开关系局部图\" viewBox=\"0 0 800 {height}\" class=\"relation-graph\">");
    for (index, relation) in relations.iter().enumerate() {
        let y = index * 80 + 40;
        let relation_target = TargetRef::new("relation", &relation.id);
        html.push_str(&format!(
            "<line x1=\"165\" y1=\"{y}\" x2=\"635\" y2=\"{y}\" stroke=\"#6d8495\"/>"
        ));
        for (x, target) in [
            (20, &relation.from_ref),
            (315, &relation_target),
            (640, &relation.to_ref),
        ] {
            let Some(object) = compiled.analysis.catalog.object(target) else {
                continue;
            };
            let href = super::site::relative_url(current, &routes[target]);
            html.push_str(&format!("<a href=\"{}\"><rect x=\"{x}\" y=\"{}\" width=\"140\" height=\"40\" rx=\"6\" fill=\"#e6eef5\"/><text x=\"{}\" y=\"{}\" font-size=\"14\" fill=\"#172d3b\">{}</text></a>",
                html_escape(&href), y - 20, x + 8, y + 5, html_escape(&object.display)));
        }
    }
    html.push_str("</svg></section>");
    html
}

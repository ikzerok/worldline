use super::semantics::link;
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
    let graph = super::relation_graph::relation_graph(compiled, routes, None, "relations.html");
    relations.body_html.push_str(&graph.html);
    relations.searchable_text.push_str(&graph.text);
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

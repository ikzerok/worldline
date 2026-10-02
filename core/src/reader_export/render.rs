use super::site::{html_escape, relative_url};
use crate::ast::{Event, Stmt, TextPart};
use crate::catalog::TargetRef;
use crate::CompileResult;
use std::collections::BTreeMap;
use std::path::Path;

pub(super) fn render_object_body(
    compiled: &CompileResult,
    target: &TargetRef,
    routes: &BTreeMap<TargetRef, String>,
    file_routes: &BTreeMap<String, String>,
    selection: &super::ReaderExportSelection,
) -> Result<(String, String), String> {
    render_object_body_at(
        compiled,
        target,
        routes,
        file_routes,
        selection,
        &routes[target],
    )
}

pub(super) fn render_object_body_at(
    compiled: &CompileResult,
    target: &TargetRef,
    routes: &BTreeMap<TargetRef, String>,
    file_routes: &BTreeMap<String, String>,
    selection: &super::ReaderExportSelection,
    current_path: &str,
) -> Result<(String, String), String> {
    let projection = super::story::Projection::new(compiled, routes, selection, target);
    let mut html = String::new();
    let mut plain = String::new();
    if selection.schema_version == super::READER_SITE_SCHEMA_VERSION
        && matches!(target.kind.as_str(), "event" | "fragment" | "scene")
    {
        append_description(&mut html, &mut plain, "静态分支阅读，不执行条件或状态");
    }
    match target.kind.as_str() {
        "event" => {
            let event = compiled
                .program
                .events
                .iter()
                .find(|event| event.name == target.id)
                .ok_or_else(|| "选中的事件无法读取".to_string())?;
            for description in projection.event(event) {
                append_description(&mut html, &mut plain, &description);
            }
            let (body_html, body_text) = render_statements(
                &event.body,
                &event.name,
                current_path,
                compiled,
                routes,
                file_routes,
                &projection,
            );
            html.push_str(&body_html);
            plain.push_str(&body_text);
        }
        "rule" => {
            let rule = compiled
                .program
                .rules
                .iter()
                .find(|rule| rule.name == target.id)
                .ok_or("选中的规则无法读取")?;
            let signature = format!(
                "{}({}) → {}",
                rule.name,
                rule.parameters
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.kind.label()))
                    .collect::<Vec<_>>()
                    .join(", "),
                rule.result.label()
            );
            append_description(&mut html, &mut plain, &signature);
        }
        "fragment" => {
            let fragment = compiled
                .program
                .fragments
                .iter()
                .find(|fragment| fragment.name == target.id)
                .ok_or("选中的片段无法读取")?;
            let signature = format!(
                "{}({})",
                fragment.name,
                fragment
                    .parameters
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.kind.label()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            append_description(&mut html, &mut plain, &signature);
            let (body_html, body_text) = render_statements(
                &fragment.body,
                "",
                current_path,
                compiled,
                routes,
                file_routes,
                &projection,
            );
            html.push_str(&body_html);
            plain.push_str(&body_text);
        }
        "scene" => {
            let (event, body) = find_scene(&compiled.program.events, &target.id)
                .ok_or_else(|| "选中的场景无法读取".to_string())?;
            let (body_html, body_text) = render_statements(
                body,
                &event.name,
                current_path,
                compiled,
                routes,
                file_routes,
                &projection,
            );
            html.push_str(&body_html);
            plain.push_str(&body_text);
        }
        "entity" => {
            if let Some(entity) = compiled.analysis.catalog.entities.get(&target.id) {
                append_description(&mut html, &mut plain, &entity.description);
            }
        }
        "world" => {
            if let Some(world) = compiled
                .program
                .worlds
                .iter()
                .find(|item| item.name == target.id)
            {
                append_description(&mut html, &mut plain, &world.description);
            }
        }
        "anchor" => {
            if let Some(anchor) = compiled.analysis.catalog.anchors.get(&target.id) {
                append_description(&mut html, &mut plain, &anchor.description);
            }
        }
        "tag" => {
            if let Some(tag) = compiled.analysis.catalog.tags.get(&target.id) {
                append_description(&mut html, &mut plain, &tag.description);
            }
        }
        "relation" => {
            if let Some(relation) = compiled.analysis.catalog.relations.get(&target.id) {
                append_description(&mut html, &mut plain, &relation.description);
            }
        }
        _ => {}
    }
    super::fields::append_fields(
        compiled,
        target,
        routes,
        selection,
        &mut html,
        &mut plain,
        current_path,
    )?;
    if html.is_empty() {
        html.push_str("<p>该对象没有静态阅读正文。</p>");
    }
    Ok((html, plain))
}

fn append_description(html: &mut String, plain: &mut String, description: &str) {
    if !description.trim().is_empty() {
        html.push_str(&format!("<p>{}</p>", html_escape(description)));
        plain.push_str(description);
    }
}

fn find_scene<'a>(events: &'a [Event], target_id: &str) -> Option<(&'a Event, &'a [Stmt])> {
    for event in events {
        if let Some(body) = find_scene_in(&event.body, &event.name, target_id) {
            return Some((event, body));
        }
    }
    None
}

fn find_scene_in<'a>(statements: &'a [Stmt], prefix: &str, target_id: &str) -> Option<&'a [Stmt]> {
    for statement in statements {
        let Stmt::Scene(scene) = statement else {
            continue;
        };
        let full_name = format!("{prefix}.{}", scene.name);
        if full_name == target_id {
            return Some(&scene.body);
        }
        if let Some(body) = find_scene_in(&scene.body, &full_name, target_id) {
            return Some(body);
        }
    }
    None
}

fn render_statements(
    statements: &[Stmt],
    current_event: &str,
    current_path: &str,
    compiled: &CompileResult,
    routes: &BTreeMap<TargetRef, String>,
    file_routes: &BTreeMap<String, String>,
    projection: &super::story::Projection<'_>,
) -> (String, String) {
    let mut html = String::new();
    let mut plain = String::new();
    for statement in statements {
        for description in projection.statement(statement) {
            append_description(&mut html, &mut plain, &description);
        }
        match statement {
            Stmt::Text(text) => {
                let (text_html, text_plain) =
                    render_parts(&text.parts, current_path, routes, file_routes);
                if !text_html.is_empty() {
                    html.push_str(&format!("<p>{text_html}</p>"));
                    plain.push_str(&text_plain);
                    plain.push('\n');
                }
            }
            Stmt::Say(say) => {
                let (text_html, text_plain) =
                    render_parts(&say.text.parts, current_path, routes, file_routes);
                // Speaker identity and author direction are never inferred as public.
                html.push_str(&format!("<p>{text_html}</p>"));
                plain.push_str(&text_plain);
                plain.push('\n');
            }
            Stmt::Call(call) => {
                if let Some(destination) = routes.get(&TargetRef::new("fragment", &call.name)) {
                    html.push_str(&format!(
                        "<p><a href=\"{}\">阅读已公开的片段</a></p>",
                        html_escape(&relative_url(current_path, destination))
                    ));
                    plain.push_str("阅读已公开的片段\n");
                } else {
                    html.push_str("<p class=\"unavailable\">未公开内容</p>");
                    plain.push_str("未公开内容\n");
                }
            }
            Stmt::Choice(choice) => {
                let (label_html, label_plain) =
                    render_parts(&choice.label, current_path, routes, file_routes);
                html.push_str(&format!("<div class=\"choice\">{label_html}</div>"));
                plain.push_str(&label_plain);
                plain.push('\n');
                let (body_html, body_text) = render_statements(
                    &choice.body,
                    current_event,
                    current_path,
                    compiled,
                    routes,
                    file_routes,
                    projection,
                );
                html.push_str(&body_html);
                plain.push_str(&body_text);
            }
            Stmt::If(condition) => {
                // Conditions are intentionally not evaluated or disclosed in a reader package.
                for (condition, branch) in &condition.branches {
                    if projection.enabled {
                        let label = condition
                            .as_ref()
                            .map(|value| format!("分支条件：{}", projection.condition(value)))
                            .unwrap_or_else(|| "否则分支".into());
                        append_description(&mut html, &mut plain, &label);
                    }
                    let (branch_html, branch_text) = render_statements(
                        branch,
                        current_event,
                        current_path,
                        compiled,
                        routes,
                        file_routes,
                        projection,
                    );
                    html.push_str(&branch_html);
                    plain.push_str(&branch_text);
                }
            }
            Stmt::Divert(divert) => {
                if let crate::ast::DivertTarget::Node(name) = &divert.target {
                    if let Some(node) = compiled
                        .analysis
                        .symbols
                        .resolve_target(name, Some(current_event))
                    {
                        let event = &compiled.program.events[node.event];
                        let target = TargetRef::new(
                            if node.scenes.is_empty() {
                                "event"
                            } else {
                                "scene"
                            },
                            &node.full_name(&event.name),
                        );
                        if let Some(destination) = routes.get(&target) {
                            let href = relative_url(current_path, destination);
                            html.push_str(&format!(
                                "<p><a href=\"{}\">继续阅读</a></p>",
                                html_escape(&href)
                            ));
                            plain.push_str("继续阅读\n");
                        } else {
                            html.push_str("<p class=\"unavailable\">未公开内容</p>");
                            plain.push_str("未公开内容\n");
                        }
                    } else {
                        html.push_str("<p class=\"unavailable\">未公开内容</p>");
                        plain.push_str("未公开内容\n");
                    }
                }
            }
            Stmt::Scene(scene) => {
                let (body_html, body_text) = render_statements(
                    &scene.body,
                    current_event,
                    current_path,
                    compiled,
                    routes,
                    file_routes,
                    projection,
                );
                html.push_str(&body_html);
                plain.push_str(&body_text);
            }
            Stmt::Let(_)
            | Stmt::Set(_)
            | Stmt::Change(_)
            | Stmt::Anchor(_)
            | Stmt::Effect(_)
            | Stmt::Local(_)
            | Stmt::Return(_)
            | Stmt::DynamicChange(_) => {}
        }
    }
    (html, plain)
}

fn render_parts(
    parts: &[TextPart],
    current_path: &str,
    routes: &BTreeMap<TargetRef, String>,
    file_routes: &BTreeMap<String, String>,
) -> (String, String) {
    let mut html = String::new();
    let mut plain = String::new();
    for part in parts {
        match part {
            TextPart::Str(text) => {
                html.push_str(&html_escape(text));
                plain.push_str(text);
            }
            TextPart::Expr(_) => {
                html.push_str("（动态内容略）");
                plain.push_str("（动态内容略）");
            }
            TextPart::Link(link) => {
                let route = if link.target.kind == "file" {
                    let path = crate::compiler::source_path(Path::new(&link.target.id));
                    file_routes.get(&path.to_string_lossy().into_owned())
                } else {
                    routes.get(&link.target)
                };
                if let Some(route) = route {
                    let href = relative_url(current_path, route);
                    html.push_str(&format!(
                        "<a href=\"{}\">{}</a>",
                        html_escape(&href),
                        html_escape(&link.label)
                    ));
                    plain.push_str(&link.label);
                } else {
                    html.push_str("<span class=\"unavailable\">未公开内容</span>");
                    plain.push_str("未公开内容");
                }
            }
        }
    }
    (html, plain)
}

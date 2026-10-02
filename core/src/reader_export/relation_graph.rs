//! Public relation rows: bounded scan labels plus complete ordinary text links.
use super::site::{html_escape, relative_url};
use super::*;
use unicode_segmentation::UnicodeSegmentation;

pub(super) struct GraphProjection {
    pub html: String,
    pub text: String,
}

pub(super) fn relation_graph(
    compiled: &CompileResult,
    routes: &BTreeMap<TargetRef, String>,
    focus: Option<&TargetRef>,
    current: &str,
) -> GraphProjection {
    let mut html = String::new();
    let mut plain = String::new();
    let mut row = 0;
    for relation in compiled.analysis.catalog.relations.values() {
        let relation_target = TargetRef::new("relation", &relation.id);
        if focus.is_some_and(|target| {
            relation.from_ref != *target && relation.to_ref != *target && relation_target != *target
        }) {
            continue;
        }
        let targets = [&relation.from_ref, &relation_target, &relation.to_ref];
        let items: Option<Vec<_>> = targets
            .into_iter()
            .map(|target| {
                Some((
                    compiled.analysis.catalog.object(target)?,
                    routes.get(target)?,
                ))
            })
            .collect();
        let Some(items) = items else { continue };
        if row == 0 {
            html.push_str("<section class=\"relation-section\"><h2>公开关系局部图</h2><p class=\"relation-help\">图中显示简要名称；完整名称与链接在每行下方。</p><ol class=\"relation-rows\">");
        }
        html.push_str("<li><div class=\"relation-graph-scroll\"><svg role=\"img\" aria-label=\"公开关系局部图\" viewBox=\"0 0 800 104\" width=\"800\" height=\"104\" class=\"relation-graph\">");
        html.push_str("<path d=\"M240 52H288M512 52H560\" fill=\"none\" stroke=\"#6d8495\" stroke-width=\"2\"/>");
        for (column, (object, route)) in items.iter().enumerate() {
            let x = [16, 288, 560][column];
            let id = format!("relation-label-{row}-{column}");
            let full = display(&object.display);
            let escaped = html_escape(full);
            let href = html_escape(&relative_url(current, route));
            html.push_str(&format!("<defs><clipPath id=\"{id}\"><rect x=\"{}\" y=\"27\" width=\"200\" height=\"50\"/></clipPath></defs><a href=\"{href}\" aria-label=\"{escaped}\"><title>{escaped}</title><rect x=\"{x}\" y=\"16\" width=\"224\" height=\"72\" rx=\"8\" fill=\"#e6eef5\"/><text clip-path=\"url(#{id})\" font-size=\"14\" fill=\"#172d3b\">",x+12));
            let lines = scan_lines(full);
            let first_y = if lines.len() == 1 { 57 } else { 46 };
            for (line, value) in lines.iter().enumerate() {
                html.push_str(&format!(
                    "<tspan x=\"{}\" y=\"{}\">{}</tspan>",
                    x + 12,
                    first_y + line * 22,
                    html_escape(value)
                ));
            }
            html.push_str("</text></a>");
        }
        html.push_str("</svg></div><dl class=\"relation-labels\">");
        for (role, (object, route)) in ["来源", "关系", "目标"].iter().zip(items) {
            let full = display(&object.display);
            html.push_str(&format!(
                "<div><dt>{role}</dt><dd><a href=\"{}\">{}</a></dd></div>",
                html_escape(&relative_url(current, route)),
                html_escape(full)
            ));
            plain.push_str(&format!("\n{role}：{full}\n"));
        }
        html.push_str("</dl></li>");
        row += 1;
    }
    if row != 0 {
        html.push_str("</ol></section>");
    }
    GraphProjection { html, text: plain }
}

fn display(value: &str) -> &str {
    if value.trim().is_empty() {
        "未命名对象"
    } else {
        value
    }
}

/// This is a bounded scan summary, not font measurement. The SVG clip is the
/// final visual boundary; full text always remains available in HTML and title.
fn scan_lines(value: &str) -> Vec<String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut graphemes = normalized.graphemes(true).peekable();
    let mut lines = Vec::new();
    for line in 0..2 {
        let mut text: String = graphemes.by_ref().take(14).collect();
        if text.is_empty() {
            break;
        }
        if line == 1 && graphemes.peek().is_some() {
            text = text.graphemes(true).take(13).collect();
            text.push('…');
        }
        lines.push(text);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::scan_lines;
    use unicode_segmentation::UnicodeSegmentation;

    #[test]
    fn labels_have_two_bounded_grapheme_lines_without_splitting_emoji() {
        for text in [
            "中文".repeat(60),
            "W".repeat(120),
            "e\u{301}".repeat(50),
            "👩‍👩‍👧‍👦".repeat(40),
        ] {
            let lines = scan_lines(&text);
            assert_eq!(lines.len(), 2);
            assert!(lines.iter().all(|line| line.graphemes(true).count() <= 14));
            assert!(lines[1].ends_with('…'));
            assert!(!lines[0].ends_with('\u{200d}'));
        }
        assert_eq!(scan_lines("短名"), ["短名"]);
        assert_eq!(scan_lines(" one\n two \t three "), ["one two three"]);
        assert!(scan_lines("").is_empty());
    }
}

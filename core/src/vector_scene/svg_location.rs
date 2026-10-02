//! 错误定位只扫描已由 XML parser 核验的起始标签，不解释 SVG/CSS。
use super::svg_elements::Element;

pub(super) fn attribute_offset(element: &Element, source: &str, field: Option<&str>) -> usize {
    let Some(field) = field else {
        return element.offset;
    };
    let inline = element.attrs.get("style").is_some_and(|style| {
        style
            .split(';')
            .filter_map(|part| part.split_once(':'))
            .any(|(key, _)| key.trim() == field)
    });
    let target = if inline { "style" } else { field };
    let bytes = source.as_bytes();
    let mut index = element.offset.saturating_add(1);
    // 先跳过元素名；接着按引号跳过属性值，避免在值中误认同名词。
    while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'>' {
        index += 1;
    }
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == bytes.len() || matches!(bytes[index], b'>' | b'/') {
            break;
        }
        let start = index;
        while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'=' {
            index += 1;
        }
        let matched = source.get(start..index) == Some(target);
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if bytes.get(index) != Some(&b'=') {
            break;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let Some(quote @ (b'\'' | b'"')) = bytes.get(index).copied() else {
            break;
        };
        if matched {
            return start;
        }
        index += 1;
        while index < bytes.len() && bytes[index] != quote {
            index += 1;
        }
        index += 1;
    }
    element.offset
}

pub(super) fn check_text_dashes(
    arena: &[Element],
    index: usize,
    runs: &[super::TextRun],
    style: &super::SceneStyle,
    source: &str,
) -> Result<(), super::SceneError> {
    for (run_index, run) in runs.iter().enumerate() {
        let effective = run.style.inherited(style);
        if !run.text.is_empty()
            && effective
                .stroke_dasharray
                .as_ref()
                .is_some_and(|a| a.iter().any(|n| *n > 0.0))
        {
            let location = match arena[index].content.get(run_index) {
                Some(super::svg_elements::Content::Child(child)) => *child,
                _ => index,
            };
            return Err(arena[location].error(source, super::SceneError::new("SCENE_STYLE", "首版不支持文字/tspan 的非零虚线描边；请为文字片段显式选择实线（none），原输入保留"), "stroke-dasharray"));
        }
    }
    Ok(())
}

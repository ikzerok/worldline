use super::{
    style, svg_path, viewport, SceneError, SceneGeometry, SceneLimits, SceneProgress, TextRun,
};
use quick_xml::{events::Event, Reader, XmlVersion};
use std::collections::BTreeMap;

pub(super) enum Content {
    Child(usize),
    Text(String),
}
pub(super) struct Element {
    pub tag: String,
    pub attrs: BTreeMap<String, String>,
    pub content: Vec<Content>,
    pub offset: usize,
}

pub(super) fn profile(message: impl Into<String>) -> SceneError {
    SceneError::new("SCENE_SVG_PROFILE", message)
}

pub(super) fn located(source: &str, offset: usize, mut error: SceneError) -> SceneError {
    if error.line.is_none() {
        let mut offset = offset.min(source.len());
        while !source.is_char_boundary(offset) {
            offset -= 1;
        }
        let (mut line, mut column, mut cr) = (1, 1, false);
        for c in source[..offset].chars() {
            match c {
                '\r' => {
                    line += 1;
                    column = 1;
                }
                '\n' if cr => {}
                '\n' => {
                    line += 1;
                    column = 1;
                }
                _ => column += 1,
            }
            cr = c == '\r';
        }
        error.line = Some(line);
        error.column = Some(column);
    }
    error
}

impl Element {
    pub fn error(&self, source: &str, mut error: SceneError, field: &str) -> SceneError {
        if error.node_id.is_none() {
            error.node_id = self.attrs.get("id").cloned();
        }
        if error.field.is_none() {
            error.field = Some(field.into());
        }
        let offset = super::svg_location::attribute_offset(self, source, error.field.as_deref());
        located(source, offset, error)
    }
    pub fn children(&self) -> impl Iterator<Item = usize> + '_ {
        self.content.iter().filter_map(|item| match item {
            Content::Child(i) => Some(*i),
            _ => None,
        })
    }
    pub fn preserve(&self, inherited: bool) -> bool {
        self.attrs
            .get("xml:space")
            .map_or(inherited, |value| value == "preserve")
    }
}

pub(super) fn report(
    progress: &mut dyn FnMut(SceneProgress) -> bool,
    stage: &str,
    completed: usize,
    total: usize,
) -> Result<(), SceneError> {
    if progress(SceneProgress {
        stage: stage.into(),
        completed,
        total,
    }) {
        Ok(())
    } else {
        Err(SceneError::new("SCENE_CANCELLED", "SVG 导入已取消"))
    }
}

fn valid_chars(text: &str) -> bool {
    text.chars().all(valid_char)
}

fn valid_char(c: char) -> bool {
    matches!(c, '\t' | '\r' | '\n') || (c >= ' ' && !matches!(c, '\u{fffe}' | '\u{ffff}'))
}

fn attributes(element: &Element) -> Result<(), SceneError> {
    let tag = element.tag.as_str();
    let specific: &[&str] = match tag {
        "svg" => &[
            "xmlns",
            "xmlns:xml",
            "width",
            "height",
            "viewBox",
            "preserveAspectRatio",
            "version",
        ],
        "g" => &["data-worldline-viewport", "clip-path"],
        "rect" => &["x", "y", "width", "height", "rx", "ry"],
        "circle" => &["cx", "cy", "r"],
        "ellipse" => &["cx", "cy", "rx", "ry"],
        "line" => &["x1", "y1", "x2", "y2"],
        "polyline" | "polygon" => &["points"],
        "path" => &["d"],
        "text" | "tspan" => &["x", "y", "dx", "dy"],
        "defs" => &[],
        "clipPath" => &["id", "clipPathUnits"],
        _ => return Err(profile(format!("不支持的 SVG 元素：{tag}"))),
    };
    for (key, value) in &element.attrs {
        let common = !matches!(tag, "defs" | "clipPath")
            && (matches!(key.as_str(), "id" | "style" | "xml:space")
                || (key == "transform" && tag != "tspan")
                || style::STYLE_KEYS.contains(&key.as_str()));
        if !common && !specific.contains(&key.as_str()) {
            let mut error = profile(format!("不支持的 SVG 属性：{key}"));
            error.field = Some(key.clone());
            return Err(error);
        }
        let invalid = match key.as_str() {
            "xmlns" => value != "http://www.w3.org/2000/svg",
            "xmlns:xml" => value != "http://www.w3.org/XML/1998/namespace",
            "xml:space" => !matches!(value.as_str(), "default" | "preserve"),
            "version" => !matches!(value.as_str(), "1.0" | "1.1" | "2.0"),
            _ => false,
        };
        if invalid {
            let mut error = profile(format!("SVG 属性值无效：{key}"));
            error.field = Some(key.clone());
            return Err(error);
        }
    }
    Ok(())
}

fn child_allowed(parent: &str, child: &str) -> bool {
    match parent {
        "svg" | "g" => {
            matches!(
                child,
                "g" | "rect"
                    | "circle"
                    | "ellipse"
                    | "line"
                    | "polyline"
                    | "polygon"
                    | "path"
                    | "text"
            ) || (parent == "svg" && child == "defs")
        }
        "text" => child == "tspan",
        "defs" => child == "clipPath",
        "clipPath" => child == "rect",
        _ => false,
    }
}

fn append_text(
    arena: &mut [Element],
    stack: &[usize],
    text: String,
    total: &mut usize,
    limits: &SceneLimits,
) -> Result<(), SceneError> {
    if !valid_chars(&text) {
        return Err(profile("SVG 文本包含非法 XML 字符"));
    }
    let current = stack.last().copied();
    if current.is_none_or(|i| !matches!(arena[i].tag.as_str(), "text" | "tspan")) {
        return if text.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')) {
            Ok(())
        } else {
            Err(profile("SVG 非 text/tspan 元素不能包含正文"))
        };
    }
    *total = total.saturating_add(text.len());
    if *total > limits.max_text_bytes {
        return Err(super::validate::limit("SVG 文本字节数"));
    }
    let content = &mut arena[current.unwrap()].content;
    if let Some(Content::Text(previous)) = content.last_mut() {
        previous.push_str(&text);
    } else {
        content.push(Content::Text(text));
    }
    Ok(())
}

fn check_declaration(bytes: &[u8]) -> Result<(), SceneError> {
    let text = std::str::from_utf8(bytes).map_err(|_| profile("XML 声明不是 UTF-8"))?;
    let start = quick_xml::events::BytesStart::from_content(text, 3);
    let mut previous = 0;
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| profile("XML 声明属性重复或格式错误"))?;
        let (rank, valid) = match attribute.key.as_ref() {
            b"version" => (1, attribute.value.as_ref() == b"1.0"),
            b"encoding" => (2, attribute.value.eq_ignore_ascii_case(b"utf-8")),
            b"standalone" => (3, matches!(attribute.value.as_ref(), b"yes" | b"no")),
            _ => return Err(profile("XML 声明包含未知属性")),
        };
        if !valid || rank <= previous || (previous == 0 && rank != 1) {
            return Err(profile(
                "仅支持有序的 XML 1.0、UTF-8 和有效 standalone 声明",
            ));
        }
        previous = rank;
    }
    if previous == 0 {
        return Err(profile("XML 声明缺少版本"));
    }
    Ok(())
}

pub(super) fn parse_xml(
    source: &str,
    limits: &SceneLimits,
    progress: &mut dyn FnMut(SceneProgress) -> bool,
) -> Result<Vec<Element>, SceneError> {
    if let Some((offset, _)) = source.char_indices().find(|(_, c)| !valid_char(*c)) {
        return Err(located(source, offset, profile("SVG 包含非法 XML 字符")));
    }
    let mut reader = Reader::from_str(source);
    reader.config_mut().expand_empty_elements = true;
    reader.config_mut().check_comments = true;
    let mut arena: Vec<Element> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut text_bytes = 0;
    let mut declaration = false;
    loop {
        let offset = reader.buffer_position() as usize;
        report(progress, "svg_parse", offset, source.len())?;
        let event = reader.read_event().map_err(|error| {
            located(
                source,
                reader.error_position() as usize,
                profile(format!("SVG XML 无效：{error}")),
            )
        })?;
        let outcome = match event {
            Event::Start(event) => {
                let tag = std::str::from_utf8(event.name().as_ref())
                    .map_err(|_| located(source, offset, profile("SVG 元素名不是 UTF-8")))?
                    .to_owned();
                let mut element = Element {
                    tag,
                    attrs: BTreeMap::new(),
                    content: Vec::new(),
                    offset,
                };
                for attribute in event.attributes() {
                    let attribute = attribute.map_err(|e| {
                        located(source, offset, profile(format!("SVG 属性无效：{e}")))
                    })?;
                    let key = std::str::from_utf8(attribute.key.as_ref())
                        .map_err(|_| located(source, offset, profile("SVG 属性名无效")))?
                        .to_owned();
                    if attribute.value.contains(&b'<') {
                        return Err(element.error(
                            source,
                            profile("SVG 属性不能包含未转义的 <"),
                            &key,
                        ));
                    }
                    let value = attribute
                        .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|e| {
                            element.error(source, profile(format!("SVG 属性实体无效：{e}")), &key)
                        })?
                        .into_owned();
                    if !valid_chars(&value) {
                        return Err(element.error(
                            source,
                            profile("SVG 属性包含非法 XML 字符"),
                            &key,
                        ));
                    }
                    element.attrs.insert(key, value);
                }
                attributes(&element).map_err(|e| element.error(source, e, "element"))?;
                if stack
                    .last()
                    .is_some_and(|i| !child_allowed(&arena[*i].tag, &element.tag))
                    || (stack.is_empty() && (!arena.is_empty() || element.tag != "svg"))
                {
                    return Err(element.error(
                        source,
                        profile("SVG 必须是单根 svg；元素位置、嵌套 svg 或嵌套 tspan 不受支持"),
                        "element",
                    ));
                }
                if arena.len() >= limits.max_nodes || stack.len() >= limits.max_depth {
                    return Err(element.error(
                        source,
                        super::validate::limit("SVG 元素数或层级"),
                        "element",
                    ));
                }
                let index = arena.len();
                if let Some(parent) = stack.last() {
                    arena[*parent].content.push(Content::Child(index));
                }
                arena.push(element);
                stack.push(index);
                Ok(())
            }
            Event::End(_) => {
                stack
                    .pop()
                    .ok_or_else(|| profile("SVG 结束标签没有对应元素"))?;
                Ok(())
            }
            Event::Text(event) => {
                if event.as_ref().windows(3).any(|window| window == b"]]>") {
                    return Err(located(
                        source,
                        offset,
                        profile("SVG 普通文字不能包含未转义的 ]]>"),
                    ));
                }
                let text = event
                    .xml10_content()
                    .map_err(|e| located(source, offset, profile(format!("SVG 文本无效：{e}"))))?
                    .into_owned();
                append_text(&mut arena, &stack, text, &mut text_bytes, limits)
            }
            Event::CData(event) => {
                if stack
                    .last()
                    .is_none_or(|i| !matches!(arena[*i].tag.as_str(), "text" | "tspan"))
                {
                    return Err(located(
                        source,
                        offset,
                        profile("CDATA 仅允许作为 text/tspan 的安全文字"),
                    ));
                }
                let text = event
                    .xml10_content()
                    .map_err(|e| located(source, offset, profile(format!("SVG CDATA 无效：{e}"))))?
                    .into_owned();
                append_text(&mut arena, &stack, text, &mut text_bytes, limits)
            }
            Event::GeneralRef(event) => {
                if stack.is_empty() {
                    return Err(located(
                        source,
                        offset,
                        profile("SVG 字符实体必须位于根元素内"),
                    ));
                }
                let raw = format!(
                    "&{};",
                    event.decode().map_err(|_| located(
                        source,
                        offset,
                        profile("SVG 实体引用无效")
                    ))?
                );
                let text = quick_xml::escape::unescape(&raw)
                    .map_err(|e| {
                        located(
                            source,
                            offset,
                            profile(format!("SVG 仅允许预定义或数字实体：{e}")),
                        )
                    })?
                    .into_owned();
                append_text(&mut arena, &stack, text, &mut text_bytes, limits)
            }
            Event::Decl(event) => {
                if declaration || !arena.is_empty() || !matches!(offset, 0 | 3) {
                    return Err(located(
                        source,
                        offset,
                        profile("XML 声明只能出现一次且必须位于文档开头"),
                    ));
                }
                declaration = true;
                check_declaration(event.as_ref())
            }
            Event::Comment(_) => Ok(()),
            Event::Eof => break,
            _ => Err(profile("SVG 禁止 DTD、实体声明、处理指令或活动内容")),
        };
        outcome.map_err(|e| match stack.last() {
            Some(i) => arena[*i].error(source, e, "element"),
            None => located(source, offset, e),
        })?;
    }
    if arena.is_empty() || !stack.is_empty() {
        return Err(located(
            source,
            source.len(),
            profile("SVG 缺少根元素或元素未闭合"),
        ));
    }
    Ok(arena)
}

pub(super) fn scalar(element: &Element, name: &str, default: f64) -> Result<f64, SceneError> {
    let Some(value) = element.attrs.get(name) else {
        return Ok(default);
    };
    let value = value.trim();
    let values =
        svg_path::numbers(value.strip_suffix("px").unwrap_or(value)).map_err(|mut e| {
            e.field = Some(name.into());
            e
        })?;
    if values.len() == 1 {
        Ok(values[0])
    } else {
        let mut e = profile(format!("SVG {name} 必须是单个标量"));
        e.field = Some(name.into());
        Err(e)
    }
}

pub(super) fn geometry(
    element: &Element,
    max_segments: usize,
) -> Result<SceneGeometry, SceneError> {
    let n = |key| scalar(element, key, 0.0);
    Ok(match element.tag.as_str() {
        "svg" | "g" => SceneGeometry::Group {
            children: Vec::new(),
        },
        "rect" => {
            let rx = scalar(element, "rx", n("ry")?)?;
            let ry = scalar(element, "ry", rx)?;
            SceneGeometry::Rect {
                x: n("x")?,
                y: n("y")?,
                width: n("width")?,
                height: n("height")?,
                rx,
                ry,
            }
        }
        "circle" => SceneGeometry::Ellipse {
            cx: n("cx")?,
            cy: n("cy")?,
            rx: n("r")?,
            ry: n("r")?,
        },
        "ellipse" => SceneGeometry::Ellipse {
            cx: n("cx")?,
            cy: n("cy")?,
            rx: n("rx")?,
            ry: n("ry")?,
        },
        "line" => SceneGeometry::Polyline {
            points: vec![[n("x1")?, n("y1")?], [n("x2")?, n("y2")?]],
        },
        "polyline" | "polygon" => {
            let values = svg_path::numbers(
                element
                    .attrs
                    .get("points")
                    .map(String::as_str)
                    .unwrap_or(""),
            )?;
            if values.len() % 2 != 0 {
                return Err(profile("SVG points 必须是完整坐标对"));
            }
            if values.len() / 2 > max_segments {
                return Err(super::validate::limit("SVG 顶点数"));
            }
            let points = values.as_chunks::<2>().0.to_vec();
            if element.tag == "polygon" {
                SceneGeometry::Polygon { points }
            } else {
                SceneGeometry::Polyline { points }
            }
        }
        "path" => SceneGeometry::Path {
            segments: svg_path::parse_path(
                element.attrs.get("d").map(String::as_str).unwrap_or(""),
                max_segments,
            )?,
        },
        "text" => SceneGeometry::Text {
            x: n("x")?,
            y: n("y")?,
            runs: Vec::new(),
        },
        _ => return Err(profile("此 SVG 元素不是可编辑几何")),
    })
}

pub(super) fn text_runs(
    arena: &[Element],
    index: usize,
    inherited_space: bool,
    source: &str,
) -> Result<Vec<TextRun>, SceneError> {
    let element = &arena[index];
    let preserve = element.preserve(inherited_space);
    let mut runs = Vec::new();
    let mut seen = false;
    let mut pending_space = false;
    for content in &element.content {
        let (mut run, preserve_run) = match content {
            Content::Text(text) => (
                TextRun {
                    text: text.clone(),
                    ..TextRun::default()
                },
                preserve,
            ),
            Content::Child(index) => {
                let child = &arena[*index];
                let parse = || {
                    let optional = |key| {
                        if child.attrs.contains_key(key) {
                            scalar(child, key, 0.0).map(Some)
                        } else {
                            Ok(None)
                        }
                    };
                    let text = child
                        .content
                        .iter()
                        .filter_map(|item| match item {
                            Content::Text(s) => Some(s.as_str()),
                            _ => None,
                        })
                        .collect::<String>();
                    Ok::<_, SceneError>(TextRun {
                        text,
                        x: optional("x")?,
                        y: optional("y")?,
                        dx: scalar(child, "dx", 0.0)?,
                        dy: scalar(child, "dy", 0.0)?,
                        style: style::parse_style(&child.attrs)?,
                        ..TextRun::default()
                    })
                };
                (
                    parse().map_err(|e| child.error(source, e, "text"))?,
                    child.preserve(preserve),
                )
            }
        };
        if !preserve_run {
            let mut normalized = String::new();
            for c in run.text.chars().filter(|c| !matches!(c, '\r' | '\n')) {
                if matches!(c, ' ' | '\t') {
                    pending_space = true;
                } else {
                    if pending_space && seen {
                        normalized.push(' ');
                    }
                    pending_space = false;
                    seen = true;
                    normalized.push(c);
                }
            }
            run.text = normalized;
        } else {
            if pending_space && seen && !run.text.is_empty() {
                run.text.insert(0, ' ');
            }
            pending_space = false;
            seen |= !run.text.is_empty();
        }
        runs.push(run);
    }
    if runs.is_empty() {
        runs.push(TextRun::default());
    }
    let first = runs
        .iter()
        .position(|run| !run.text.is_empty())
        .unwrap_or(0);
    runs[first].dx = viewport::number(runs[first].dx + scalar(element, "dx", 0.0)?)?;
    runs[first].dy = viewport::number(runs[first].dy + scalar(element, "dy", 0.0)?)?;
    Ok(runs)
}

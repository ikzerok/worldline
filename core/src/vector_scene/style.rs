use super::{SceneError, SceneStyle, WORLD_LIMIT};
use std::collections::BTreeMap;

pub(super) const STYLE_KEYS: &[&str] = &[
    "fill",
    "stroke",
    "stroke-width",
    "stroke-dasharray",
    "stroke-dashoffset",
    "opacity",
    "fill-opacity",
    "stroke-opacity",
    "fill-rule",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "font-size",
    "font-family",
    "font-weight",
    "font-style",
    "text-anchor",
];

pub(super) fn valid_color(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    if let Some((prefix, body)) = lower.split_once('(') {
        let Some(body) = body.strip_suffix(')') else {
            return false;
        };
        let parts: Vec<_> = body.split(',').map(str::trim).collect();
        let count = if prefix == "rgb" {
            3
        } else if prefix == "rgba" {
            4
        } else {
            return false;
        };
        return parts.len() == count
            && parts.iter().enumerate().all(|(i, p)| {
                let percent = p.ends_with('%');
                let text = p.strip_suffix('%').unwrap_or(p);
                text.parse::<f64>().is_ok_and(|n| {
                    n.is_finite()
                        && n >= 0.0
                        && n <= if percent {
                            100.0
                        } else if i == 3 {
                            1.0
                        } else {
                            255.0
                        }
                })
            });
    }
    const COLORS: &str = "none transparent aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgreen darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo ivory khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen steelblue tan teal thistle tomato turquoise violet wheat white whitesmoke yellow yellowgreen";
    COLORS.split_ascii_whitespace().any(|c| c == lower)
}

pub(super) fn validate_style(style: &SceneStyle) -> Result<(), SceneError> {
    super::dash::validate_style(style)?;
    let fail =
        |field: &str| SceneError::new("SCENE_STYLE", format!("样式 `{field}` 不在安全范围内"));
    for (name, color) in [("fill", &style.fill), ("stroke", &style.stroke)] {
        if color.as_deref().is_some_and(|v| !valid_color(v)) {
            return Err(fail(name));
        }
    }
    for (name, value, max) in [
        ("stroke_width", style.stroke_width, WORLD_LIMIT),
        ("opacity", style.opacity, 1.0),
        ("fill_opacity", style.fill_opacity, 1.0),
        ("stroke_opacity", style.stroke_opacity, 1.0),
        ("font_size", style.font_size, WORLD_LIMIT),
        ("miter_limit", style.miter_limit, WORLD_LIMIT),
    ] {
        if value.is_some_and(|v| !v.is_finite() || v < 0.0 || v > max) {
            return Err(fail(name));
        }
    }
    if style.miter_limit.is_some_and(|v| v < 1.0) {
        return Err(fail("miter_limit"));
    }
    for (name, value, allowed) in [
        ("fill_rule", &style.fill_rule, &["nonzero", "evenodd"][..]),
        (
            "line_cap",
            &style.line_cap,
            &["butt", "round", "square"][..],
        ),
        (
            "line_join",
            &style.line_join,
            &["miter", "round", "bevel"][..],
        ),
        (
            "font_style",
            &style.font_style,
            &["normal", "italic", "oblique"][..],
        ),
        (
            "text_anchor",
            &style.text_anchor,
            &["start", "middle", "end"][..],
        ),
    ] {
        if value.as_deref().is_some_and(|v| !allowed.contains(&v)) {
            return Err(fail(name));
        }
    }
    if style.font_weight.as_deref().is_some_and(|v| {
        !matches!(
            v,
            "normal"
                | "bold"
                | "100"
                | "200"
                | "300"
                | "400"
                | "500"
                | "600"
                | "700"
                | "800"
                | "900"
        )
    }) {
        return Err(fail("font_weight"));
    }
    if style.font_family.as_deref().is_some_and(|v| {
        v.is_empty()
            || v.len() > 512
            || !v
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, ' ' | ',' | '-' | '_' | '\'' | '"'))
    }) {
        return Err(fail("font_family"));
    }
    let reserved = [
        "fill",
        "stroke",
        "stroke_width",
        "stroke_dasharray",
        "stroke_dashoffset",
        "opacity",
        "fill_opacity",
        "stroke_opacity",
        "fill_rule",
        "line_cap",
        "line_join",
        "miter_limit",
        "font_size",
        "font_family",
        "font_weight",
        "font_style",
        "text_anchor",
    ];
    if style
        .extra
        .keys()
        .any(|key| reserved.contains(&key.as_str()))
    {
        return Err(fail("extra"));
    }
    Ok(())
}

pub(super) fn parse_style(attributes: &BTreeMap<String, String>) -> Result<SceneStyle, SceneError> {
    let mut values: BTreeMap<String, String> = attributes
        .iter()
        .filter(|(key, _)| STYLE_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if let Some(inline) = attributes.get("style") {
        for item in inline.split(';').filter(|part| !part.trim().is_empty()) {
            let (key, value) = item
                .split_once(':')
                .ok_or_else(|| SceneError::new("SCENE_SVG_PROFILE", "inline style 缺少冒号"))?;
            let key = key.trim();
            if !STYLE_KEYS.contains(&key) {
                let mut error =
                    SceneError::new("SCENE_SVG_PROFILE", format!("不支持的 style 属性：{key}"));
                error.field = Some(key.into());
                return Err(error);
            }
            values.insert(key.into(), value.trim().into());
        }
    }
    let mut style = SceneStyle::default();
    for (key, value) in values {
        let value = value.trim().to_owned();
        let num = || {
            let text = if matches!(key.as_str(), "stroke-width" | "font-size") {
                value.strip_suffix("px").unwrap_or(&value)
            } else {
                &value
            };
            text.parse::<f64>()
                .map_err(|_| SceneError::new("SCENE_STYLE", format!("样式数字无效：{key}")))
        };
        match key.as_str() {
            "fill" => style.fill = Some(value),
            "stroke" => style.stroke = Some(value),
            "stroke-width" => style.stroke_width = Some(num()?),
            "stroke-dasharray" => style.stroke_dasharray = super::dash::parse_array(&value)?,
            "stroke-dashoffset" => style.stroke_dashoffset = super::dash::parse_offset(&value)?,
            "opacity" => style.opacity = Some(num()?),
            "fill-opacity" => style.fill_opacity = Some(num()?),
            "stroke-opacity" => style.stroke_opacity = Some(num()?),
            "fill-rule" => style.fill_rule = Some(value),
            "stroke-linecap" => style.line_cap = Some(value),
            "stroke-linejoin" => style.line_join = Some(value),
            "stroke-miterlimit" => style.miter_limit = Some(num()?),
            "font-size" => style.font_size = Some(num()?),
            "font-family" => style.font_family = Some(value),
            "font-weight" => style.font_weight = Some(value),
            "font-style" => style.font_style = Some(value),
            "text-anchor" => style.text_anchor = Some(value),
            _ => unreachable!(),
        }
    }
    validate_style(&style)?;
    Ok(style)
}

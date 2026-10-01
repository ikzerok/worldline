//! 只编辑已识别的顶层值；其余 JSON 原始字节不重编码。
use crate::{parse_unique_json, LanguageVersion};
use serde_json::Value;
use std::collections::BTreeMap;
use std::ops::Range;

pub(super) fn prepare(
    before: Option<&[u8]>,
    entry: &str,
    version: LanguageVersion,
    added: &[String],
) -> Result<Vec<u8>, String> {
    let Some(bytes) = before else {
        return serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "language_version": version.as_str(),
            "entry": entry,
            "required_features": added,
        }))
        .map_err(|error| error.to_string());
    };
    let original = parse_unique_json(bytes)?;
    let object = original.as_object().ok_or("工程清单必须是对象")?;
    let source = std::str::from_utf8(bytes).map_err(|_| "工程清单不是有效 UTF-8")?;
    let (spans, close) = top_level_spans(source)?;
    let mut edits = Vec::<(Range<usize>, String)>::new();
    let mut missing = Vec::new();
    let mut expected = original.clone();
    let expected_object = expected.as_object_mut().ok_or("工程清单必须是对象")?;
    if object.get("language_version").and_then(Value::as_str) != Some(version.as_str()) {
        let quoted = serde_json::to_string(version.as_str()).map_err(|e| e.to_string())?;
        if let Some(range) = spans.get("language_version") {
            edits.push((range.clone(), quoted));
        } else {
            missing.push(format!("\"language_version\": {quoted}"));
        }
        expected_object.insert(
            "language_version".into(),
            Value::String(version.as_str().into()),
        );
    }
    if !added.is_empty() {
        let encoded = added
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
            .join(", ");
        let mut features = object
            .get("required_features")
            .map(|value| {
                value
                    .as_array()
                    .cloned()
                    .ok_or("required_features 必须是数组")
            })
            .transpose()?
            .unwrap_or_default();
        if let Some(range) = spans.get("required_features") {
            if source.as_bytes().get(range.end - 1) != Some(&b']') {
                return Err("无法确认清单能力数组的原始字节范围".into());
            }
            let separator = if features.is_empty() { "" } else { ", " };
            edits.push((
                range.end - 1..range.end - 1,
                format!("{separator}{encoded}"),
            ));
        } else {
            missing.push(format!("\"required_features\": [{encoded}]"));
        }
        features.extend(added.iter().cloned().map(Value::String));
        expected_object.insert("required_features".into(), Value::Array(features));
    }
    if !missing.is_empty() {
        let separator = if object.is_empty() { "" } else { "," };
        edits.push((
            close..close,
            format!("{separator}\n  {}\n", missing.join(",\n  ")),
        ));
    }
    edits.sort_by_key(|left| std::cmp::Reverse(left.0.start));
    let mut after = source.to_string();
    for (range, replacement) in edits {
        after.replace_range(range, &replacement);
    }
    if parse_unique_json(after.as_bytes())? != expected {
        return Err("清单字节编辑与候选能力不一致，未修改工程".into());
    }
    Ok(after.into_bytes())
}

fn whitespace(source: &[u8], at: &mut usize) {
    while source.get(*at).is_some_and(u8::is_ascii_whitespace) {
        *at += 1;
    }
}

fn expect(source: &[u8], at: &mut usize, byte: u8) -> Result<(), String> {
    whitespace(source, at);
    if source.get(*at) != Some(&byte) {
        return Err("清单 JSON 原始字节与解析结果不匹配".into());
    }
    *at += 1;
    Ok(())
}

type FieldSpans = BTreeMap<String, Range<usize>>;

fn top_level_spans(source: &str) -> Result<(FieldSpans, usize), String> {
    let mut at = 0;
    let mut spans = BTreeMap::new();
    expect(source.as_bytes(), &mut at, b'{')?;
    loop {
        whitespace(source.as_bytes(), &mut at);
        if source.as_bytes().get(at) == Some(&b'}') {
            return Ok((spans, at));
        }
        if !spans.is_empty() {
            expect(source.as_bytes(), &mut at, b',')?;
            whitespace(source.as_bytes(), &mut at);
        }
        let mut stream = serde_json::Deserializer::from_str(&source[at..]).into_iter::<String>();
        let key = stream
            .next()
            .ok_or("清单 JSON 缺少键")?
            .map_err(|e| e.to_string())?;
        at += stream.byte_offset();
        expect(source.as_bytes(), &mut at, b':')?;
        whitespace(source.as_bytes(), &mut at);
        let start = at;
        let mut stream = serde_json::Deserializer::from_str(&source[at..]).into_iter::<Value>();
        stream
            .next()
            .ok_or("清单 JSON 缺少值")?
            .map_err(|e| e.to_string())?;
        at += stream.byte_offset();
        if spans.insert(key, start..at).is_some() {
            return Err("清单包含重复字段，不能安全编辑".into());
        }
    }
}

//! JSON 引用变更先由正式注册文档语义决定，再定位原始字符串 token；不重排 JSON。
use super::preview::Edit;
use crate::catalog::TargetRef;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ops::Range;

type Spans = BTreeMap<(String, bool), Range<usize>>;

pub(crate) fn edits(
    source: &str,
    before: &Value,
    after: &Value,
    target: &TargetRef,
    new_id: &str,
) -> Result<Vec<Edit>, String> {
    let mut spans = Spans::new();
    let mut at = 0;
    scan(source, &mut at, "", before, &mut spans)?;
    let mut output = Vec::new();
    changes(before, after, "", &spans, target, new_id, &mut output)?;
    Ok(output)
}

fn pointer(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn changes(
    before: &Value,
    after: &Value,
    path: &str,
    spans: &Spans,
    target: &TargetRef,
    new_id: &str,
    output: &mut Vec<Edit>,
) -> Result<(), String> {
    if before == after {
        return Ok(());
    }
    match (before, after) {
        (Value::String(_), Value::String(new)) => add(path, false, new, spans, output),
        (Value::Array(old), Value::Array(new)) if old.len() == new.len() => {
            for (index, (old, new)) in old.iter().zip(new).enumerate() {
                changes(
                    old,
                    new,
                    &pointer(path, &index.to_string()),
                    spans,
                    target,
                    new_id,
                    output,
                )?;
            }
            Ok(())
        }
        (Value::Object(old), Value::Object(new)) if old.len() == new.len() => {
            for (key, value) in old {
                let field = pointer(path, key);
                if let Some(new_value) = new.get(key) {
                    changes(value, new_value, &field, spans, target, new_id, output)?;
                } else {
                    let new_key = format!("{}:{new_id}", target.kind);
                    if path != "/positions"
                        || *key != format!("{}:{}", target.kind, target.id)
                        || new.get(&new_key) != Some(value)
                    {
                        return Err("展示文档重构包含非引用结构变更".into());
                    }
                    add(&field, true, &new_key, spans, output)?;
                }
            }
            Ok(())
        }
        _ => Err("展示文档重构包含非引用字段变更".into()),
    }
}

fn add(
    path: &str,
    key: bool,
    value: &str,
    spans: &Spans,
    output: &mut Vec<Edit>,
) -> Result<(), String> {
    let range = spans
        .get(&(path.into(), key))
        .ok_or("无法定位展示引用的原始 token")?
        .clone();
    let quoted = serde_json::to_string(value).map_err(|error| error.to_string())?;
    output.push(Edit {
        range,
        replacement: quoted[1..quoted.len() - 1].into(),
        field: path.into(),
    });
    Ok(())
}

fn whitespace(source: &str, at: &mut usize) {
    while source
        .as_bytes()
        .get(*at)
        .is_some_and(u8::is_ascii_whitespace)
    {
        *at += 1;
    }
}

fn expect(source: &str, at: &mut usize, byte: u8) -> Result<(), String> {
    whitespace(source, at);
    if source.as_bytes().get(*at) != Some(&byte) {
        return Err("JSON 字节与解析结果不匹配".into());
    }
    *at += 1;
    Ok(())
}

fn string(source: &str, at: &mut usize) -> Result<(String, Range<usize>), String> {
    whitespace(source, at);
    let start = *at;
    let mut stream = serde_json::Deserializer::from_str(&source[start..]).into_iter::<String>();
    let value = stream
        .next()
        .ok_or("JSON 字符串缺失")?
        .map_err(|error| error.to_string())?;
    *at += stream.byte_offset();
    Ok((value, start + 1..*at - 1))
}

fn scan(
    source: &str,
    at: &mut usize,
    path: &str,
    value: &Value,
    spans: &mut Spans,
) -> Result<(), String> {
    whitespace(source, at);
    match value {
        Value::String(expected) => {
            let (actual, range) = string(source, at)?;
            if &actual != expected {
                return Err("JSON 字段内容不匹配".into());
            }
            spans.insert((path.into(), false), range);
        }
        Value::Object(fields) => {
            expect(source, at, b'{')?;
            for index in 0..fields.len() {
                if index > 0 {
                    expect(source, at, b',')?;
                }
                let (key, range) = string(source, at)?;
                let child = pointer(path, &key);
                spans.insert((child.clone(), true), range);
                expect(source, at, b':')?;
                scan(
                    source,
                    at,
                    &child,
                    fields.get(&key).ok_or("JSON 字段缺失")?,
                    spans,
                )?;
            }
            expect(source, at, b'}')?;
        }
        Value::Array(items) => {
            expect(source, at, b'[')?;
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    expect(source, at, b',')?;
                }
                scan(source, at, &pointer(path, &index.to_string()), item, spans)?;
            }
            expect(source, at, b']')?;
        }
        _ => {
            let mut stream =
                serde_json::Deserializer::from_str(&source[*at..]).into_iter::<Value>();
            let actual = stream
                .next()
                .ok_or("JSON 值缺失")?
                .map_err(|error| error.to_string())?;
            if &actual != value {
                return Err("JSON 值不匹配".into());
            }
            *at += stream.byte_offset();
        }
    }
    Ok(())
}

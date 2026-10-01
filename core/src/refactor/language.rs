//! 受语法位置约束的身份改写，不替换普通文字/字符串/作者备注。
use crate::catalog::TargetRef;

pub(super) fn rewrite(line: &str, target: &TargetRef, new_id: &str) -> (String, usize) {
    let cleaned = crate::lexer::strip_comments(line);
    let count = cleaned.trim_end().chars().count();
    let code_len = line
        .char_indices()
        .nth(count)
        .map(|(i, _)| i)
        .unwrap_or(line.len());
    let code = &line[..code_len.min(line.len())];
    let suffix = &line[code.len()..];
    let trimmed = code.trim_start();
    let mut output = String::new();
    let mut count = 0;
    let mut previous = String::new();
    let mut index = 0;
    let mut parens: Vec<String> = Vec::new();
    let mut argument_indices: Vec<usize> = Vec::new();
    let mut list_kind = "";
    let mut spoken_seen = false;
    // 1.12 显式绑定的目标是结构引用；不把旧正文整体按新版本重新分类。
    if trimmed.starts_with("bind ") {
        let tokens: Vec<_> = trimmed.split_whitespace().collect();
        if tokens.len() == 5
            && tokens[0] == "bind"
            && tokens[1] == target.kind
            && tokens[2] == target.id
            && tokens[3] == "to"
        {
            let start = code.find("bind ").unwrap_or(0);
            let prefix = &code[..start];
            return (
                format!(
                    "{prefix}bind {} {new_id} to {}{suffix}",
                    target.kind, tokens[4]
                ),
                1,
            );
        }
    }
    let text_line = crate::lexer::lex_source_with_options(
        "rename.wl",
        code,
        &mut Vec::new(),
        crate::CompileOptions::v1_11(),
    )
    .first()
    .is_some_and(|l| matches!(l.kind, crate::lexer::LineKind::Text { .. }));
    if text_line {
        let (text, hits) = super::text::rewrite(code, target, new_id, true, false);
        return (format!("{text}{suffix}"), hits);
    }
    while index < code.len() {
        let ch = code[index..].chars().next().unwrap();
        if code[index..].starts_with("/*") {
            let end = code[index + 2..]
                .find("*/")
                .map(|end| index + 2 + end + 2)
                .unwrap_or(code.len());
            output.push_str(&code[index..end]);
            index = end;
            continue;
        }
        if code[index..].starts_with("//") {
            output.push_str(&code[index..]);
            break;
        }
        if ch == '"' {
            let start = index;
            index += 1;
            let mut escaped = false;
            while index < code.len() {
                let c = code[index..].chars().next().unwrap();
                index += c.len_utf8();
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    break;
                }
            }
            let quoted = &code[start..index];
            if matches!(target.kind.as_str(), "tag" | "state")
                && (parens.last().is_some_and(|p| p == &target.kind)
                    || parens.last().is_some_and(|p| p == "has")
                        && argument_indices.last().is_some_and(|i| {
                            (*i == 0 && target.kind == "state") || (*i == 1 && target.kind == "tag")
                        }))
                && serde_json::from_str::<String>(quoted).ok().as_deref() == Some(&target.id)
            {
                output.push_str(&crate::authoring::quote(new_id));
                count += 1;
            } else if (trimmed.starts_with("say ") || trimmed.starts_with("choice "))
                && !spoken_seen
            {
                spoken_seen = true;
                let chars: Vec<_> = quoted.chars().collect();
                let is_say = trimmed.starts_with("say ");
                let parse = if is_say {
                    crate::lexer::parse_quoted_raw
                } else {
                    crate::lexer::parse_quoted
                };
                if let Ok((inner, _)) = parse(&chars, 0, "rename.wl", 1, &mut Vec::new()) {
                    let (text, hits) = super::text::rewrite(&inner, target, new_id, false, is_say);
                    if hits > 0 {
                        output.push_str(&if is_say {
                            format!("\"{text}\"")
                        } else {
                            crate::authoring::quote(&text)
                        });
                        count += hits;
                    } else {
                        output.push_str(quoted);
                    }
                } else {
                    output.push_str(quoted);
                }
            } else {
                output.push_str(quoted);
            }
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = index;
            index += ch.len_utf8();
            while index < code.len()
                && (code.as_bytes()[index].is_ascii_alphanumeric()
                    || code.as_bytes()[index] == b'_')
            {
                index += 1;
            }
            let word = &code[start..index];
            let next = code[index..].trim_start().chars().next();
            let structural = previous == target.kind
                || (target.kind == "state" && previous == "become")
                || (target.kind == "fragment" && previous == "call")
                || (target.kind == "character"
                    && matches!(previous.as_str(), "say" | "relation" | "meet" | "part"));
            let call = target.kind == "rule" && next == Some('(');
            let identity = matches!(target.kind.as_str(), "tag" | "state")
                && (parens.last().is_some_and(|p| p == &target.kind)
                    || parens.last().is_some_and(|p| p == "has")
                        && argument_indices.last().is_some_and(|index| {
                            (*index == 0 && target.kind == "state")
                                || (*index == 1 && target.kind == "tag")
                        }));
            let listed = target.kind == list_kind;
            if matches!(word, "with" | "add" | "remove") {
                if trimmed.starts_with("state ")
                    || trimmed.starts_with("become ")
                    || trimmed.starts_with("mark ")
                {
                    list_kind = "tag";
                }
                if trimmed.starts_with("event ") && word == "with" {
                    list_kind = "character";
                }
            } else if matches!(word, "as" | "after" | "during" | "follows" | "at" | "perm") {
                list_kind = "";
            }
            if word == target.id && (structural || call || identity || listed) {
                output.push_str(new_id);
                count += 1;
            } else {
                output.push_str(word);
            }
            previous = word.into();
            continue;
        }
        if ch == '(' {
            parens.push(previous.clone());
            argument_indices.push(0);
        } else if ch == ')' {
            parens.pop();
            argument_indices.pop();
        } else if ch == ',' {
            if let Some(index) = argument_indices.last_mut() {
                *index += 1;
            }
        }
        output.push(ch);
        index += ch.len_utf8();
    }
    output.push_str(suffix);
    (output, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_literals_and_author_notes_are_never_interpreted_as_references() {
        for line in [
            r#"  source_note "fare(2) [[rule:fare|普通记录]]" // fare(2)"#,
            r#"  property private = "fare(2) [[rule:fare|普通记录]]""#,
            r#"  say doctor "普通话" direction "fare(2) [[rule:fare|舞台备注]]""#,
        ] {
            assert_eq!(
                rewrite(line, &TargetRef::new("rule", "fare"), "cost"),
                (line.to_string(), 0)
            );
        }
    }
}

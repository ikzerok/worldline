use super::super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn classify_catalog(
    word: &str,
    rest_trim: &str,
    file: &str,
    no: u32,
    word_col: u32,
    diags: &mut Vec<Diagnostic>,
    options: crate::compiler::CompileOptions,
    source: &mut LineSource,
) -> LineKind {
    match word {
        "tag" | "asset" | "mark" | "attach" | "anchor_def" | "anchor_link" | "alias" => {
            LineKind::Catalog(crate::catalog_syntax::parse_with_options(
                word, rest_trim, file, no, diags, options,
            ))
        }
        "property" => {
            let (name, value_src) = rest_trim.split_once('=').unwrap_or((rest_trim, ""));
            source.value = source.remainder + rest_trim[..rest_trim.len() - value_src.trim_start().len()].chars().count() as u32;
            let name = name.trim();
            if !valid_identifier(name) || value_src.trim().is_empty() {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, 1, 8),
                    "属性需要 property 名称 = 字面量",
                ));
            }
            LineKind::Property {
                name: name.into(),
                value_src: value_src.trim().into(),
                loc: Loc::new(no, 1),
            }
        }
        "description" => {
            let rc: Vec<char> = rest_trim.chars().collect();
            let text = if rc.first() == Some(&'"') {
                match parse_quoted(&rc, 0, file, no, diags) {
                    Ok((text, end)) if skip_spaces(&rc, end) == rc.len() => text,
                    _ => {
                        diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(no, 1, 11),
                            "description 只接受一个带引号的字符串",
                        ));
                        String::new()
                    }
                }
            } else {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, 1, 11),
                    "description 后需要带引号的字符串",
                ));
                String::new()
            };
            LineKind::Description {
                text,
                loc: Loc::new(no, 1),
            }
        }
        "relation_type" if options.language_version.supports_relations() => {
            let rc = rest_trim.chars().collect::<Vec<char>>();
            let (name, end) = match scan_qualified(&rc, 0) {
                Some((name, end)) if !name.contains('.') && valid_identifier(&name) => (name, end),
                _ => {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, word_col, 13),
                        "relation_type 后需要类型 ID",
                    ));
                    (String::new(), 0)
                }
            };
            let (display, consumed) = parse_as_note(&rc, end, file, no, diags);
            if skip_spaces(&rc, consumed) != rc.len() {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, 13),
                    "relation_type 声明末尾只能是 as \"显示名\"",
                ));
            }
            LineKind::RelationType {
                name,
                display,
                loc: Span::new(no, word_col, 13),
            }
        }
        "relation_def" if options.language_version.supports_relations() => {
            let tokens = crate::catalog_syntax::tokenize(rest_trim, file, no, diags);
            let token = |index: usize| {
                tokens
                    .get(index)
                    .map(|(value, _)| value.as_str())
                    .unwrap_or("")
            };
            let quoted = |index: usize| tokens.get(index).is_some_and(|(_, quoted)| *quoted);
            let qualified =
                |value: &str| !value.is_empty() && value.split('.').all(valid_identifier);
            let id = token(0).to_string();
            let relation_type = token(2).to_string();
            let from_kind = token(4).to_string();
            let from_id = token(5).to_string();
            let to_kind = token(7).to_string();
            let to_id = token(8).to_string();
            let endpoint = |kind: &str, id: &str, index: usize| {
                if kind == "file" {
                    quoted(index) && !id.is_empty()
                } else {
                    !quoted(index) && qualified(id)
                }
            };
            let valid = tokens.len() == 9
                && !quoted(0)
                && valid_identifier(&id)
                && token(1) == "type"
                && !quoted(2)
                && valid_identifier(&relation_type)
                && token(3) == "from"
                && !quoted(4)
                && valid_identifier(&from_kind)
                && endpoint(&from_kind, &from_id, 5)
                && token(6) == "to"
                && !quoted(7)
                && valid_identifier(&to_kind)
                && endpoint(&to_kind, &to_id, 8);
            if !valid {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, 12),
                    "relation_def 必须完整写出 from/to 的对象类型和 ID",
                ));
            }
            LineKind::RelationDef {
                id,
                relation_type,
                from_kind,
                from_id,
                to_kind,
                to_id,
                loc: Span::new(no, word_col, 12),
            }
        }
        "inverse" | "direction" | "from_kind" | "to_kind" | "from" | "to"
            if options.language_version.supports_relations() =>
        {
            let value = rest_trim.to_string();
            if value.is_empty() {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, word.len() as u32),
                    format!("{word} 后需要值"),
                ));
            }
            let value = if matches!(word, "inverse") {
                let rc = value.chars().collect::<Vec<char>>();
                match parse_quoted(&rc, 0, file, no, diags) {
                    Ok((text, end)) if skip_spaces(&rc, end) == rc.len() => text,
                    _ => {
                        diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(no, word_col, word.len() as u32),
                            "inverse 后需要一个带引号的显示名",
                        ));
                        String::new()
                    }
                }
            } else {
                value
            };
            LineKind::RelationField {
                name: word.into(),
                value,
                loc: Loc::new(no, word_col),
            }
        }
        "source_note" | "scope" | "scope_ref" if options.language_version.supports_relations() => {
            let value = rest_trim.to_string();
            if value.is_empty() {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(no, word_col, word.len() as u32),
                    format!("{word} 后需要值"),
                ));
            }
            let value = if word == "source_note" {
                let rc = value.chars().collect::<Vec<char>>();
                match parse_quoted(&rc, 0, file, no, diags) {
                    Ok((text, end)) if skip_spaces(&rc, end) == rc.len() => text,
                    _ => {
                        diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(no, word_col, word.len() as u32),
                            "source_note 后需要一个带引号的字符串",
                        ));
                        String::new()
                    }
                }
            } else {
                value
            };
            LineKind::RelationField {
                name: word.into(),
                value,
                loc: Loc::new(no, word_col),
            }
        }
        "relation" => {
            let rc: Vec<char> = rest_trim.chars().collect();
            let (target, label) = match scan_qualified(&rc, 0) {
                Some((target, end)) if valid_identifier(&target) => {
                    let (label, end) = parse_as_note(&rc, end, file, no, diags);
                    if label.as_ref().is_none_or(|s| s.trim().is_empty())
                        || skip_spaces(&rc, end) != rc.len()
                    {
                        diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(no, 1, 8),
                            "关系需要 relation 角色ID as \"关系名称\"",
                        ));
                    }
                    (target, label.unwrap_or_default())
                }
                _ => {
                    diags.push(Diagnostic::error(
                        "P004",
                        file,
                        Span::new(no, 1, 8),
                        "relation 后需要角色 ID",
                    ));
                    (String::new(), String::new())
                }
            };
            LineKind::Relation {
                target,
                label,
                loc: Loc::new(no, 1),
            }
        }
        _ => unreachable!("classification dispatches catalog keywords"),
    }
}

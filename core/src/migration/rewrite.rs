use super::*;

struct Edit {
    start: usize,
    end: usize,
    text: String,
}
fn edit(edits: &mut Vec<Edit>, start: usize, end: usize, text: impl Into<String>) {
    edits.push(Edit {
        start,
        end,
        text: text.into(),
    });
}
fn expression_edits(
    expr: &mut Expr,
    chars: &[char],
    m: &PermissionMigration,
    edits: &mut Vec<Edit>,
) {
    visit_expr(expr, &mut |e| {
        let Some(p) = permission(e) else {
            return;
        };
        let Some(tag) = m.tags.get(p) else {
            return;
        };
        let Expr::Call { loc, .. } = e else {
            return;
        };
        let start = loc.column as usize - 1;
        let ts = tokens(&chars[start..]);
        if ts.len() < 4 || ts[0].value != "perm" || ts[1].value != "(" {
            return;
        }
        edit(edits, start, start + 4, "has");
        edit(
            edits,
            start + ts[1].end,
            start + ts[1].end,
            format!("{}, ", m.state),
        );
        let argument_start = start + ts[2].start;
        let argument_end = if chars.get(argument_start) == Some(&'\\')
            && chars.get(argument_start + 1) == Some(&'"')
        {
            let Some(end) = (argument_start + 2..chars.len()).find(|&i| {
                if chars[i] != '"' {
                    return false;
                }
                let backslashes = chars[..i].iter().rev().take_while(|&&c| c == '\\').count();
                backslashes % 4 == 1
            }) else {
                return;
            };
            end + 1
        } else {
            start + ts[2].end
        };
        edit(edits, argument_start, argument_end, tag);
    });
}

pub(crate) fn rewrite_sources(
    result: &crate::CompileResult,
) -> Result<BTreeMap<PathBuf, String>, String> {
    let Some(m) = &result.program.permission_migration else {
        return Ok(result.sources.clone());
    };
    let mut sources = result.sources.clone();
    let mut changed = false;
    for (path, source) in &mut sources {
        let cleaned = lexer::strip_comments(source);
        let clean_lines: Vec<_> = cleaned.lines().collect();
        let mut raw_lines: Vec<_> = source.split_inclusive('\n').map(str::to_string).collect();
        for line in lexer::lex_source_with_options(
            &path.to_string_lossy(),
            source,
            &mut Vec::new(),
            result.options,
        ) {
            let index = line.no as usize - 1;
            let clean = clean_lines[index];
            let chars: Vec<_> = clean.chars().collect();
            let ts = tokens(&chars);
            let mut edits = Vec::new();
            let mut expressions = Vec::new();
            let mut interpolation = None;
            let mut interpolation_quoted = false;
            match &line.kind {
                LineKind::Language111 {
                    keyword, source, ..
                } => match keyword.as_str() {
                    "rule" | "local" => {
                        if let Some((_, expr)) = source.split_once('=') {
                            expressions.push(expr.trim());
                        }
                    }
                    "call" => expressions.push(source.as_str()),
                    "say" => {
                        if let Some(token) = ts.iter().find(|t| t.quoted) {
                            let a = clean
                                .char_indices()
                                .nth(token.start + 1)
                                .map(|(i, _)| i)
                                .unwrap_or(clean.len());
                            let b = clean
                                .char_indices()
                                .nth(token.end - 1)
                                .map(|(i, _)| i)
                                .unwrap_or(clean.len());
                            interpolation = Some(&clean[a..b]);
                            interpolation_quoted = true;
                        }
                    }
                    "become" => {
                        if let Some((state, tags, _)) =
                            crate::language::dynamic_change_parts(source)
                        {
                            expressions.push(state);
                            expressions.push(tags);
                        }
                    }
                    _ => {}
                },
                LineKind::ChangeLine { kind, id, .. }
                    if matches!(kind, ChangeKind::Grant | ChangeKind::Revoke) =>
                {
                    let tag = m.tags.get(id).ok_or("迁移缺少权限标签映射")?;
                    edit(&mut edits, ts[0].start, ts[0].end, "become");
                    edit(
                        &mut edits,
                        ts[1].start,
                        ts[1].end,
                        format!(
                            "{} {} {tag}",
                            m.state,
                            if *kind == ChangeKind::Grant {
                                "add"
                            } else {
                                "remove"
                            }
                        ),
                    );
                }
                LineKind::Event {
                    perm, after_src, ..
                } => {
                    if let Some(p) = perm {
                        let limit = after_src
                            .as_ref()
                            .and_then(|s| clean.rfind(s))
                            .map(|i| clean[..i].chars().count())
                            .unwrap_or(chars.len());
                        let pos = ts
                            .iter()
                            .enumerate()
                            .skip(2)
                            .find(|(i, t)| {
                                t.start < limit
                                    && !t.quoted
                                    && t.value == "perm"
                                    && ts.get(i + 1).is_some_and(|t| &t.value == p)
                            })
                            .map(|(i, _)| i)
                            .ok_or("无法定位事件权限子句")?;
                        edit(
                            &mut edits,
                            ts[pos].start,
                            ts[pos].end,
                            format!("after has({},", m.state),
                        );
                        edit(
                            &mut edits,
                            ts[pos + 1].start,
                            ts[pos + 1].end,
                            format!("{})", m.tags[p]),
                        );
                        if let Some(src) = after_src {
                            let after = ts
                                .iter()
                                .skip(pos + 2)
                                .find(|t| !t.quoted && t.value == "after")
                                .ok_or("无法定位 after 子句")?;
                            edit(&mut edits, after.start, after.end, "and (");
                            let end = clean.rfind(src).ok_or("无法定位前置表达式")? + src.len();
                            let end = clean[..end].chars().count();
                            edit(&mut edits, end, end, ")");
                        }
                    }
                    if let Some(src) = after_src {
                        expressions.push(src.as_str());
                    }
                }
                LineKind::Let { expr_src, .. }
                | LineKind::Const { expr_src, .. }
                | LineKind::Set { expr_src, .. } => expressions.push(expr_src.as_str()),
                LineKind::If { cond_src, .. } | LineKind::ElseIf { cond_src, .. } => {
                    expressions.push(cond_src.as_str())
                }
                LineKind::Effect {
                    cond_src: Some(src),
                    ..
                } => {
                    expressions.push(src.as_str());
                }
                LineKind::Choice {
                    label_raw,
                    cond_src,
                    ..
                } => {
                    let label = ts.iter().find(|t| t.quoted).ok_or("无法定位选择文本")?;
                    let decoded: Vec<_> = label_raw.chars().collect();
                    let mut positions = Vec::new();
                    let mut pos = label.start + 1;
                    while pos < label.end - 1 {
                        positions.push(pos);
                        pos += if chars[pos] == '\\' { 2 } else { 1 };
                    }
                    positions.push(label.end - 1);
                    if positions.len() != decoded.len() + 1 {
                        return Err("选择文本的转义无法安全迁移".into());
                    }
                    let mut diagnostics = Vec::new();
                    let mut parts = crate::expression::parse_interpolations(
                        label_raw,
                        &line.file,
                        line.no,
                        0,
                        &mut diagnostics,
                    );
                    if !diagnostics.is_empty() {
                        return Err("选择文本有语法错误，无法迁移".into());
                    }
                    let mut label_edits = Vec::new();
                    for part in &mut parts {
                        if let TextPart::Expr(e) = part {
                            expression_edits(e, &decoded, m, &mut label_edits);
                        }
                    }
                    for e in label_edits {
                        edit(&mut edits, positions[e.start], positions[e.end], e.text);
                    }
                    if let Some(src) = cond_src {
                        expressions.push(src.as_str());
                    }
                }
                LineKind::Text { content, .. } => interpolation = Some(content.as_str()),
                _ => {}
            }
            for src in expressions {
                let base = clean.rfind(src).ok_or("无法定位表达式")?;
                let mut diagnostics = Vec::new();
                let mut expr = crate::expression::parse_expr_src(
                    src,
                    &line.file,
                    line.no,
                    clean[..base].chars().count() as u32,
                    &mut diagnostics,
                );
                if !diagnostics.is_empty() {
                    return Err("表达式有语法错误，无法安全迁移权限".into());
                }
                expression_edits(&mut expr, &chars, m, &mut edits);
            }
            if let Some(raw) = interpolation {
                let base = clean.find(raw).ok_or("无法定位文本插值")?;
                let mut diagnostics = Vec::new();
                let mut parts = if interpolation_quoted {
                    crate::expression::parse_quoted_interpolations_with_options(
                        raw,
                        &line.file,
                        line.no,
                        clean[..base].chars().count() as u32,
                        &mut diagnostics,
                        result.options,
                    )
                } else {
                    crate::expression::parse_interpolations(
                        raw,
                        &line.file,
                        line.no,
                        clean[..base].chars().count() as u32,
                        &mut diagnostics,
                    )
                };
                if !diagnostics.is_empty() {
                    return Err("文本插值有语法错误，无法安全迁移权限".into());
                }
                for part in &mut parts {
                    if let TextPart::Expr(e) = part {
                        expression_edits(e, &chars, m, &mut edits);
                    }
                }
            }
            if !edits.is_empty() {
                let mut raw: Vec<_> = raw_lines[index].chars().collect();
                edits.sort_by_key(|e| std::cmp::Reverse((e.start, e.end)));
                for e in edits {
                    raw.splice(e.start..e.end, e.text.chars());
                }
                raw_lines[index] = raw.iter().collect();
                changed = true;
            }
        }
        *source = raw_lines.concat();
    }
    if changed {
        let entry = result.program.files.first().ok_or("工程没有入口文件")?;
        let source = sources
            .get_mut(&PathBuf::from(entry))
            .ok_or("入口缓冲未载入")?;
        // 只删除本模块生成的整行元数据；普通作者注释不参与替换。
        *source = source
            .split_inclusive('\n')
            .filter(|l| !l.starts_with(HEADER) && !l.starts_with(TAG) && !l.starts_with(GATE))
            .collect();
        source.push('\n');
        source.push_str(&m.declarations);
        source.push_str(&m.metadata());
    }
    Ok(sources)
}

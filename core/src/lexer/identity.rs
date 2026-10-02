//! 正式行分类、目录 token 与表达式 AST 的身份范围；所有范围映回原始 UTF-8。
use super::{lex_source_with_options, LineKind};
use crate::ast::TextPart;
use crate::catalog::{CatalogDecl, TargetRef};
use crate::catalog_syntax::{tokenize_spanned, SourceToken};
use crate::CompileOptions;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentitySourceSpan {
    pub target: TargetRef,
    pub line: u32,
    pub field: String,
    pub range: Range<usize>,
}

/// entity/relation 身份的唯一源码投影；不是对正文内相似字符串的搜索。
pub fn identity_source_spans(
    file: &str,
    source: &str,
    options: CompileOptions,
) -> Vec<IdentitySourceSpan> {
    spans(file, source, options, false)
}

/// 正式路径 token 投影复用身份词法器，包括所有 file 链接、include 和素材声明。
pub(crate) fn path_source_spans(
    file: &str,
    source: &str,
    options: CompileOptions,
) -> Vec<IdentitySourceSpan> {
    spans(file, source, options, true)
}

fn spans(
    file: &str,
    source: &str,
    options: CompileOptions,
    paths: bool,
) -> Vec<IdentitySourceSpan> {
    let cleaned = super::strip_comments(source);
    let clean_lines: Vec<_> = cleaned.split_inclusive('\n').collect();
    let mut offset = 0;
    let raw_lines: Vec<_> = source
        .split_inclusive('\n')
        .map(|raw| {
            let entry = (offset, raw);
            offset += raw.len();
            entry
        })
        .collect();
    let mut output = Vec::new();
    for line in lex_source_with_options(file, source, &mut Vec::new(), options) {
        let index = line.no as usize - 1;
        let Some(&(base, raw)) = raw_lines.get(index) else {
            continue;
        };
        let Some(clean) = clean_lines.get(index) else {
            continue;
        };
        let tokens = tokenize_spanned(clean, file, line.no, &mut Vec::new());
        let mut push = |target: TargetRef, range: Range<usize>, field: &str| {
            if if paths {
                !matches!(target.kind.as_str(), "file" | "include_path" | "asset_path")
            } else {
                !matches!(target.kind.as_str(), "entity" | "relation")
            } {
                return;
            }
            let start = char_byte(raw, range.start);
            let end = char_byte(raw, range.end);
            if start < end {
                output.push(IdentitySourceSpan {
                    target,
                    line: line.no,
                    field: field.into(),
                    range: base + start..base + end,
                });
            }
        };
        let mut token = |target: TargetRef, index: usize, field: &str| {
            if let Some(token) = tokens.get(index).filter(|token| token.value == target.id) {
                let range = if token.quoted {
                    token.range.start + 1..token.range.end - 1
                } else {
                    token.range.clone()
                };
                push(target, range, field);
            }
        };
        match &line.kind {
            LineKind::Include { path, .. } if paths => {
                token(TargetRef::new("include_path", path), 1, "include.path");
            }
            LineKind::Entity { name, .. } => {
                token(TargetRef::new("entity", name), 1, "declaration.id")
            }
            LineKind::RelationDef {
                id,
                from_kind,
                from_id,
                to_kind,
                to_id,
                ..
            } => {
                token(TargetRef::new("relation", id), 1, "declaration.id");
                token(TargetRef::new(from_kind, from_id), 6, "from.id");
                token(TargetRef::new(to_kind, to_id), 9, "to.id");
            }
            LineKind::Catalog(declaration) => match declaration {
                CatalogDecl::Asset(asset) if paths => {
                    token(TargetRef::new("asset_path", &asset.path), 3, "asset.path");
                }
                CatalogDecl::Alias(alias) => token(alias.target.clone(), 2, "alias.target.id"),
                CatalogDecl::Mark(link) => token(link.target.clone(), 2, "mark.target.id"),
                CatalogDecl::Attach(link) => token(link.target.clone(), 2, "attach.target.id"),
                CatalogDecl::AnchorLink(link) => {
                    token(link.target.clone(), 3, "anchor_link.target.id")
                }
                CatalogDecl::State(state) => token(state.target.clone(), 4, "state.target.id"),
                _ => {}
            },
            LineKind::RelationField { name, .. }
                if matches!(name.as_str(), "scope" | "scope_ref") =>
            {
                if let (Some(kind), Some(id)) = (tokens.get(1), tokens.get(2)) {
                    token(TargetRef::new(&kind.value, &id.value), 2, "scope_ref.id");
                }
            }
            LineKind::Schema112 { keyword, .. } if keyword == "bind" => {
                if let (Some(kind), Some(id)) = (tokens.get(1), tokens.get(2)) {
                    token(TargetRef::new(&kind.value, &id.value), 2, "bind.target.id");
                }
            }
            LineKind::Property {
                value_src, name, ..
            } => {
                let Some(equal) = clean.find('=') else {
                    continue;
                };
                let tail = &clean[equal + 1..];
                let start = equal + 1 + tail.len() - tail.trim_start().len();
                let chars_before = clean[..start].chars().count();
                let expr =
                    crate::expression::parse_expr_src(value_src, file, line.no, 0, &mut Vec::new());
                if let crate::ast::Expr::Call {
                    name: call, args, ..
                } = expr
                {
                    if let [crate::ast::Expr::Str(kind), crate::ast::Expr::Str(id)] =
                        args.as_slice()
                    {
                        if call == "ref" {
                            if let Some(range) =
                                crate::expression::static_ref_id_range(value_src, kind, id)
                            {
                                push(
                                    TargetRef::new(kind, id),
                                    chars_before + range.start..chars_before + range.end,
                                    &format!("property.{name}.ref.id"),
                                );
                            }
                        }
                    }
                }
            }
            LineKind::Text { content, .. } => {
                // 正式分类移除缩进以及可选的行首转义；清理后的后缀保留精确起点。
                if let Some(at) = clean.trim_end().strip_suffix(content) {
                    let base_chars = at.chars().count();
                    let body = crate::parser::split_text_decorations(content).0;
                    for (target, range) in links(&body, file, line.no, options, false) {
                        push(
                            target,
                            base_chars + range.start..base_chars + range.end,
                            "text.link.id",
                        );
                    }
                }
            }
            LineKind::Choice {
                label_raw,
                label_span,
                ..
            } => {
                // 使用正式 choice 词法起点，兼容 choice"…" / once"…"。
                let chars: Vec<_> = clean.chars().collect();
                let start = line.indent as usize + label_span.column.saturating_sub(2) as usize;
                if let Ok((_, end)) =
                    super::parse_quoted_raw(&chars, start, file, line.no, &mut Vec::new())
                {
                    let quoted = SourceToken {
                        value: label_raw.clone(),
                        quoted: true,
                        range: start..end,
                    };
                    choice_links(clean, &quoted, file, line.no, options, &mut push);
                }
            }
            LineKind::Language111 { keyword, .. } if keyword == "say" => {
                if let Some(quoted) = tokens.iter().find(|token| token.quoted) {
                    let chars: Vec<_> = clean.chars().collect();
                    let inner: String = chars[quoted.range.start + 1..quoted.range.end - 1]
                        .iter()
                        .collect();
                    for (target, range) in links(&inner, file, line.no, options, true) {
                        let base = quoted.range.start + 1;
                        push(target, base + range.start..base + range.end, "say.link.id");
                    }
                }
            }
            _ => {}
        }
    }
    output.sort_by_key(|span| span.range.start);
    output
}

fn char_byte(raw: &str, index: usize) -> usize {
    raw.char_indices()
        .nth(index)
        .map_or(raw.len(), |(at, _)| at)
}

fn links(
    raw: &str,
    file: &str,
    line: u32,
    options: CompileOptions,
    quoted: bool,
) -> Vec<(TargetRef, Range<usize>)> {
    let parse = if quoted {
        crate::expression::parse_quoted_interpolations_with_options
    } else {
        crate::expression::parse_interpolations_with_options
    };
    parse(raw, file, line, 0, &mut Vec::new(), options)
        .into_iter()
        .filter_map(|part| {
            if let TextPart::Link(link) = part {
                Some((link.target, link.id_start..link.id_end))
            } else {
                None
            }
        })
        .collect()
}

fn choice_links(
    clean: &str,
    token: &SourceToken,
    file: &str,
    line: u32,
    options: CompileOptions,
    push: &mut impl FnMut(TargetRef, Range<usize>, &str),
) {
    // Choice 的正式词法语义先解码外层字符串，再解析正文；映射只投影位置。
    let chars: Vec<_> = clean.chars().collect();
    let mut positions = Vec::new();
    let mut at = token.range.start + 1;
    while at < token.range.end - 1 {
        if chars[at] == '\\' && at + 1 < token.range.end - 1 {
            let raw: String = chars[at..at + 2].iter().collect();
            let decoded =
                super::decode_escapes(&raw, file, crate::Span::new(line, 1, 2), &mut Vec::new());
            positions.extend(decoded.chars().enumerate().map(|(offset, _)| at + offset));
            at += 2;
        } else {
            positions.push(at);
            at += 1;
        }
    }
    positions.push(token.range.end - 1);
    for (target, range) in links(&token.value, file, line, options, false) {
        if let (Some(&start), Some(&end)) = (positions.get(range.start), positions.get(range.end)) {
            push(target, start..end, "choice.link.id");
        }
    }
}

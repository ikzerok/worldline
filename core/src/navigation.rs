//! 完整对象别名、正文链接与只读文字导航；不执行叙事。
use crate::ast::{Stmt, TextPart};
use crate::catalog::{Catalog, CatalogDecl, CatalogObject, ReferenceInfo, TargetRef, TARGET_KINDS};
use crate::lexer::{lex_source, LineKind};
use crate::{Diagnostic, Program, Span};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AliasInfo {
    pub target: TargetRef,
    pub name: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct InlineLink {
    pub target: TargetRef,
    pub label: String,
    pub start: usize,
    pub end: usize,
    pub column: u32,
}

/// 求值后文字中的显式链接；start/end 是 UTF-8 字节范围。
#[derive(Debug, Clone, Serialize)]
pub struct RenderedLink {
    pub target: TargetRef,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextLinkInfo {
    pub source: TargetRef,
    pub target: TargetRef,
    pub label: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
}

pub(crate) fn parse_link_with_options(
    inner: &str,
    options: crate::compiler::CompileOptions,
) -> Option<(TargetRef, String)> {
    let (destination, label) = inner.split_once('|')?;
    let (kind, id) = destination.split_once(':')?;
    if (!TARGET_KINDS.contains(&kind)
        || ((kind == "entity" || kind == "relation")
            && !options.language_version.supports_relations()))
        || id.is_empty()
        || label.trim().is_empty()
        || inner.contains(['[', ']', '{', '}', '\\', '"', '#', '~', '\n', '\r'])
        || label.contains('|')
        || inner.contains("//")
        || inner.contains("/*")
        || inner.contains("*/")
        || (kind != "file" && !id.split('.').all(crate::lexer::valid_identifier))
    {
        return None;
    }
    Some((TargetRef::new(kind, id), label.into()))
}

fn resolve(mut target: TargetRef, file: &str) -> TargetRef {
    if target.kind == "file" {
        target.id = crate::catalog::resolved_asset(file, &target.id)
            .to_string_lossy()
            .into_owned();
    }
    target
}

pub fn link_source(target: &TargetRef, label: &str, file: &str) -> Result<String, String> {
    let id = if target.kind == "file" {
        crate::catalog_edit::relative_source_path(
            std::path::Path::new(file).parent().ok_or("源文件无目录")?,
            std::path::Path::new(&target.id),
        )?
    } else {
        target.id.clone()
    };
    let inner = format!("{}:{id}|{label}", target.kind);
    let options = if target.kind == "entity" || target.kind == "relation" {
        crate::compiler::CompileOptions::v1_10()
    } else {
        crate::compiler::CompileOptions::default()
    };
    parse_link_with_options(&inner, options).ok_or("显示文字含有链接分隔符，请使用普通文字")?;
    Ok(format!("[[{inner}]]"))
}

impl Catalog {
    pub fn aliases_for(&self, target: &TargetRef) -> Vec<String> {
        self.aliases
            .iter()
            .filter(|a| &a.target == target)
            .map(|a| a.name.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn search_objects(&self, query: &str) -> Vec<CatalogObject> {
        let query = query.trim().to_lowercase();
        self.objects
            .iter()
            .filter(|object| {
                object.display.to_lowercase().contains(&query)
                    || object.target.id.to_lowercase().contains(&query)
                    || self.aliases.iter().any(|a| {
                        a.target == object.target && a.name.to_lowercase().contains(&query)
                    })
            })
            .cloned()
            .collect()
    }
}

pub(crate) fn collect(program: &Program, catalog: &mut Catalog, diags: &mut Vec<Diagnostic>) {
    for declaration in &program.catalog {
        if let CatalogDecl::Alias(alias) = declaration {
            let mut alias = alias.clone();
            alias.target = resolve(alias.target, &alias.file);
            validate(catalog, &alias.target, &alias.file, alias.line, 1, diags);
            catalog.aliases.push(alias);
        }
    }
    for (event, file) in program.events.iter().zip(&program.event_files) {
        collect_body(
            &event.body,
            &TargetRef::new("event", &event.name),
            file,
            catalog,
            diags,
        );
    }
}

fn validate(
    catalog: &Catalog,
    target: &TargetRef,
    file: &str,
    line: u32,
    column: u32,
    diags: &mut Vec<Diagnostic>,
) {
    if catalog.object(target).is_none() {
        diags.push(Diagnostic::error(
            "A218",
            file,
            Span::new(line, column, 2),
            format!("资料引用的对象 {} {} 不存在", target.kind, target.id),
        ));
    }
}

fn collect_parts(
    parts: &[TextPart],
    source: &TargetRef,
    file: &str,
    line: u32,
    catalog: &mut Catalog,
    diags: &mut Vec<Diagnostic>,
) {
    for part in parts {
        if let TextPart::Link(link) = part {
            let target = resolve(link.target.clone(), file);
            validate(catalog, &target, file, line, link.column, diags);
            catalog.references.push(ReferenceInfo {
                source: source.clone(),
                target: target.clone(),
                kind: "正文链接".into(),
                file: file.into(),
                line,
            });
            catalog.text_links.push(TextLinkInfo {
                source: source.clone(),
                target,
                label: link.label.clone(),
                file: file.into(),
                line,
                column: link.column,
            });
        }
    }
}

fn collect_body(
    body: &[Stmt],
    source: &TargetRef,
    file: &str,
    catalog: &mut Catalog,
    diags: &mut Vec<Diagnostic>,
) {
    for stmt in body {
        match stmt {
            Stmt::Text(text) => {
                collect_parts(&text.parts, source, file, text.loc.line, catalog, diags)
            }
            Stmt::Choice(choice) => {
                collect_parts(&choice.label, source, file, choice.loc.line, catalog, diags);
                collect_body(&choice.body, source, file, catalog, diags);
            }
            Stmt::Scene(scene) => collect_body(
                &scene.body,
                &TargetRef::new("scene", &format!("{}.{}", source.id, scene.name)),
                file,
                catalog,
                diags,
            ),
            Stmt::If(branches) => {
                for (_, body) in &branches.branches {
                    collect_body(body, source, file, catalog, diags);
                }
            }
            _ => {}
        }
    }
}

/// 保留插值和结构原文，只把显式链接换成可点击的显示文字。
#[derive(Debug, Clone)]
pub struct ReadingPart {
    pub text: String,
    pub target: Option<TargetRef>,
}

fn linked_parts(raw: &str, file: &str, options: crate::CompileOptions) -> Vec<ReadingPart> {
    let chars: Vec<_> = raw.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    for part in crate::expression::parse_interpolations_with_options(
        raw,
        file,
        1,
        1,
        &mut Vec::new(),
        options,
    ) {
        if let TextPart::Link(link) = part {
            if link.start > start {
                out.push(ReadingPart {
                    text: chars[start..link.start].iter().collect(),
                    target: None,
                });
            }
            out.push(ReadingPart {
                text: link.label,
                target: Some(resolve(link.target, file)),
            });
            start = link.end;
        }
    }
    if start < chars.len() {
        out.push(ReadingPart {
            text: chars[start..].iter().collect(),
            target: None,
        });
    }
    out
}

pub fn reading_lines(source: &str, file: &str) -> Vec<Vec<ReadingPart>> {
    reading_lines_with_options(source, file, crate::CompileOptions::default())
}

pub fn reading_lines_with_options(
    source: &str,
    file: &str,
    options: crate::CompileOptions,
) -> Vec<Vec<ReadingPart>> {
    let parsed = crate::lexer::lex_source_with_options(file, source, &mut Vec::new(), options);
    source
        .lines()
        .enumerate()
        .map(|(index, raw)| {
            match parsed
                .iter()
                .find(|line| line.no as usize == index + 1)
                .map(|l| &l.kind)
            {
                Some(LineKind::Text { content, .. }) => {
                    let mut parts = vec![ReadingPart {
                        text: raw.chars().take_while(|c| c.is_whitespace()).collect(),
                        target: None,
                    }];
                    parts.extend(linked_parts(content, file, options));
                    parts
                }
                Some(LineKind::Choice {
                    label_raw,
                    once,
                    cond_src,
                    ..
                }) => {
                    let indent: String = raw.chars().take_while(|c| c.is_whitespace()).collect();
                    let mut parts = vec![ReadingPart {
                        text: format!("{indent}choice {}\"", if *once { "once " } else { "" }),
                        target: None,
                    }];
                    parts.extend(linked_parts(label_raw, file, options));
                    parts.push(ReadingPart {
                        text: format!(
                            "\"{}",
                            cond_src
                                .as_ref()
                                .map(|c| format!(" if {c}"))
                                .unwrap_or_default()
                        ),
                        target: None,
                    });
                    parts
                }
                _ => vec![ReadingPart {
                    text: raw.into(),
                    target: None,
                }],
            }
        })
        .collect()
}

pub(crate) fn rename_links(raw: &str, old: &str, new: &str) -> String {
    let mut chars: Vec<_> = raw.chars().collect();
    let parts = crate::expression::parse_interpolations(raw, "", 1, 1, &mut Vec::new());
    for part in parts.into_iter().rev() {
        if let TextPart::Link(link) = part {
            if link.target == TargetRef::new("character", old) {
                chars.splice(
                    link.start..link.end,
                    format!("[[character:{new}|{}]]", link.label).chars(),
                );
            }
        }
    }
    chars.into_iter().collect()
}

impl crate::project::Project {
    /// 按核心提供的声明位置读取完整对象，不产生新的正文真源。
    pub fn object_source(&self, file: &str, line: u32) -> Result<String, String> {
        let source = self.document(std::path::Path::new(file))?;
        let parsed = lex_source(file, source, &mut Vec::new());
        let index = parsed
            .iter()
            .position(|l| l.no == line)
            .ok_or("对象来源不存在")?;
        let end = parsed
            .iter()
            .skip(index + 1)
            .find(|l| l.indent <= parsed[index].indent)
            .map(|l| l.no as usize - 1)
            .unwrap_or(source.lines().count());
        Ok(source
            .lines()
            .skip(line as usize - 1)
            .take(end.saturating_sub(line as usize - 1))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// 由 Project::edit 包裹，按目标替换全工程别名并保留注释。
    pub fn set_aliases(&mut self, target: &TargetRef, names: &[String]) -> Result<(), String> {
        if names
            .iter()
            .any(|name| name.trim().is_empty() || name.contains(['\n', '\r']))
        {
            return Err("别名不能为空白或包含换行".into());
        }
        if self.compile().analysis.catalog.object(target).is_none() {
            return Err("别名目标不存在".into());
        }
        let options = self.compile_options();
        for (path, document) in &mut self.documents {
            let removed: std::collections::BTreeSet<_> = crate::lexer::lex_source_with_options(
                &path.to_string_lossy(),
                &document.text,
                &mut Vec::new(),
                options,
            )
            .into_iter()
            .filter_map(|line| match line.kind {
                LineKind::Catalog(CatalogDecl::Alias(alias))
                    if resolve(alias.target.clone(), &alias.file) == *target =>
                {
                    Some(line.no as usize)
                }
                _ => None,
            })
            .collect();
            document.text = document
                .text
                .split_inclusive('\n')
                .enumerate()
                .map(|(i, raw)| {
                    if removed.contains(&(i + 1)) {
                        crate::authoring::comments(raw)
                    } else {
                        raw.into()
                    }
                })
                .collect();
        }
        let mut source = self.document(&self.entry)?.to_string();
        let id = if target.kind == "file" {
            crate::authoring::quote(&crate::catalog_edit::relative_source_path(
                &self.root,
                std::path::Path::new(&target.id),
            )?)
        } else {
            target.id.clone()
        };
        for name in names.iter().collect::<std::collections::BTreeSet<_>>() {
            source.push_str(&format!(
                "\nalias {} {} as {}\n",
                target.kind,
                id,
                crate::authoring::quote(name)
            ));
        }
        self.set_text(&self.entry.clone(), source)
    }
}

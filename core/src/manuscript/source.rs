use super::*;
use crate::ast::{Stmt, TextPart};
use crate::CompileResult;
use std::collections::BTreeMap;

pub(super) struct SourceLookup<'a> {
    objects: BTreeMap<&'a TargetRef, &'a crate::catalog::CatalogObject>,
    events: BTreeMap<&'a str, &'a [Stmt]>,
    fragments: BTreeMap<&'a str, &'a [Stmt]>,
}
impl<'a> SourceLookup<'a> {
    pub(super) fn new(content: &'a CompileResult) -> Self {
        let mut objects = BTreeMap::new();
        for object in &content.analysis.catalog.objects {
            objects.entry(&object.target).or_insert(object);
        }
        let mut events = BTreeMap::new();
        for event in &content.program.events {
            events
                .entry(event.name.as_str())
                .or_insert(event.body.as_slice());
        }
        let mut fragments = BTreeMap::new();
        for fragment in &content.program.fragments {
            fragments
                .entry(fragment.name.as_str())
                .or_insert(fragment.body.as_slice());
        }
        Self {
            objects,
            events,
            fragments,
        }
    }
}

pub(super) fn resolve_source(
    target: &TargetRef,
    content: &CompileResult,
    lookup: &SourceLookup<'_>,
    index: &mut ManuscriptIndex,
) -> ManuscriptSource {
    if !matches!(
        target.kind.as_str(),
        "event" | "scene" | "entity" | "fragment"
    ) {
        index.error(
            "MAN008",
            format!("书稿正文目标类型 `{}` 不受支持", target.kind),
        );
        return ManuscriptSource {
            status: ManuscriptReferenceStatus::Invalid,
            location: None,
            stats: None,
        };
    }
    if (target.kind == "entity" && !content.options.language_version.supports_entities())
        || (target.kind == "fragment" && !content.options.language_version.supports_language_111())
    {
        index.error(
            "MAN004",
            format!(
                "当前语言版本无法确认正文目标 `{}:{}`",
                target.kind, target.id
            ),
        );
        return ManuscriptSource {
            status: ManuscriptReferenceStatus::Unresolved,
            location: None,
            stats: None,
        };
    }
    let Some(object) = lookup.objects.get(target) else {
        let status = missing_or_unresolved(content);
        report_missing_target(target, status, index);
        return ManuscriptSource {
            status,
            location: None,
            stats: None,
        };
    };
    let text = match target.kind.as_str() {
        "event" => lookup
            .events
            .get(target.id.as_str())
            .map(|body| narrative_text(body)),
        "scene" => content
            .analysis
            .symbols
            .scenes
            .get(&target.id)
            .and_then(|path| {
                content
                    .program
                    .events
                    .get(path.event)
                    .map(|event| (event, path))
            })
            .and_then(|(event, path)| scene_body(&event.body, &path.scenes))
            .map(narrative_text),
        "fragment" => lookup
            .fragments
            .get(target.id.as_str())
            .map(|body| narrative_text(body)),
        "entity" => content
            .analysis
            .catalog
            .entities
            .get(&target.id)
            .map(|entity| entity.description.clone()),
        _ => None,
    };
    let Some(text) = text else {
        index.error(
            "MAN004",
            format!("正文目标 `{}` 的源码范围无法确认", target.id),
        );
        return ManuscriptSource {
            status: ManuscriptReferenceStatus::Unresolved,
            location: None,
            stats: None,
        };
    };
    ManuscriptSource {
        status: ManuscriptReferenceStatus::Resolved,
        location: Some(ManuscriptSourceLocation {
            file: object.file.clone(),
            line: object.line,
        }),
        stats: Some(text_stats(&text)),
    }
}

pub(super) fn resolve_perspective(
    target: &TargetRef,
    content: &CompileResult,
    lookup: &SourceLookup<'_>,
    index: &mut ManuscriptIndex,
) -> ManuscriptReferenceStatus {
    if target.kind != "character" {
        index.error(
            "MAN008",
            format!("POV 目标类型 `{}` 必须是 character", target.kind),
        );
        return ManuscriptReferenceStatus::Invalid;
    }
    if lookup.objects.contains_key(target) {
        ManuscriptReferenceStatus::Resolved
    } else {
        let status = missing_or_unresolved(content);
        report_missing_target(target, status, index);
        status
    }
}

fn missing_or_unresolved(content: &CompileResult) -> ManuscriptReferenceStatus {
    if content.has_errors() {
        ManuscriptReferenceStatus::Unresolved
    } else {
        ManuscriptReferenceStatus::Missing
    }
}

fn report_missing_target(
    target: &TargetRef,
    status: ManuscriptReferenceStatus,
    index: &mut ManuscriptIndex,
) {
    let (code, wording) = match status {
        ManuscriptReferenceStatus::Unresolved => ("MAN004", "无法确认是否存在"),
        ManuscriptReferenceStatus::Missing => ("MAN003", "不存在"),
        _ => return,
    };
    index.error(
        code,
        format!("书稿引用的 {} `{}` {wording}", target.kind, target.id),
    );
}

fn scene_body<'a>(body: &'a [Stmt], names: &[String]) -> Option<&'a [Stmt]> {
    let (name, rest) = names.split_first()?;
    let scene = body.iter().find_map(|statement| match statement {
        Stmt::Scene(scene) if &scene.name == name => Some(scene),
        _ => None,
    })?;
    if rest.is_empty() {
        Some(&scene.body)
    } else {
        scene_body(&scene.body, rest)
    }
}

fn narrative_text(statements: &[Stmt]) -> String {
    fn append_parts(parts: &[TextPart], output: &mut String) {
        for part in parts {
            match part {
                TextPart::Str(text) => output.push_str(text),
                TextPart::Link(link) => output.push_str(&link.label),
                TextPart::Expr(_) => {}
            }
        }
    }
    fn append(statements: &[Stmt], output: &mut String) {
        for statement in statements {
            match statement {
                Stmt::Say(say) => {
                    append_parts(&say.text.parts, output);
                    output.push('\n');
                }
                Stmt::Text(text) => {
                    append_parts(&text.parts, output);
                    if !text.glue {
                        output.push('\n');
                    }
                }
                Stmt::Choice(choice) => {
                    append_parts(&choice.label, output);
                    output.push('\n');
                    append(&choice.body, output);
                    output.push('\n');
                }
                Stmt::If(condition) => {
                    for (_, branch) in &condition.branches {
                        append(branch, output);
                        output.push('\n');
                    }
                }
                Stmt::Scene(scene) => append(&scene.body, output),
                // 声明、表达式、跳转、效果和其他控制语法不属于静态阅读文本。
                Stmt::Local(_)
                | Stmt::Call(_)
                | Stmt::Return(_)
                | Stmt::DynamicChange(_)
                | Stmt::Divert(_)
                | Stmt::Let(_)
                | Stmt::Set(_)
                | Stmt::Change(_)
                | Stmt::Anchor(_)
                | Stmt::Effect(_) => {}
            }
        }
    }
    let mut output = String::new();
    append(statements, &mut output);
    output
}

fn text_stats(text: &str) -> ManuscriptTextStats {
    let mut stats = ManuscriptTextStats::default();
    let mut in_non_han_word = false;
    for character in text.chars() {
        if is_han_ideograph(character) {
            stats.han_characters += 1;
            stats.words += 1;
            in_non_han_word = false;
        } else if character.is_alphanumeric() {
            if !in_non_han_word {
                stats.words += 1;
            }
            in_non_han_word = true;
        } else {
            in_non_han_word = false;
        }
    }
    stats
}

fn is_han_ideograph(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x2F800..=0x2FA1F
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EBEF
            | 0x2EBF0..=0x2EE5F
            | 0x30000..=0x3134F
            | 0x31350..=0x323AF
    )
}

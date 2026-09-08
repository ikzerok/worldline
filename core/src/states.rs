//! 状态声明、变更出处索引与结构编辑。时间线不隐式执行这些变更。
use crate::ast::{Change, ChangeKind, Expr, Loc, Stmt};
use crate::catalog::{Catalog, CatalogDecl, ReferenceInfo, TargetRef, TARGET_KINDS};
use crate::{Diagnostic, Program, Span};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct StateDecl {
    pub id: String,
    pub display: String,
    pub target: TargetRef,
    pub tags: Vec<String>,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct StateInfo {
    pub id: String,
    pub display: String,
    pub target: TargetRef,
    pub tags: Vec<String>,
    pub file: String,
    pub line: u32,
    pub changes: Vec<StateChangeSite>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StateChangeSite {
    pub kind: ChangeKind,
    pub event: String,
    pub node: String,
    pub timing: String,
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub contexts: Vec<String>,
    pub file: String,
    pub line: u32,
}

fn error(
    diags: &mut Vec<Diagnostic>,
    code: &'static str,
    file: &str,
    line: u32,
    message: impl Into<String>,
) {
    diags.push(Diagnostic::error(
        code,
        file,
        Span::new(line, 1, 5),
        message,
    ));
}

fn tags_and_note(tokens: &[(String, bool)], start: usize) -> Option<(Vec<String>, Option<String>)> {
    let tail = tokens.get(start..)?;
    let end = tail
        .iter()
        .position(|(s, quoted)| !quoted && s == "as")
        .unwrap_or(tail.len());
    let note = if end < tail.len() {
        if tail.len() != end + 2 || !tail[end + 1].1 {
            return None;
        }
        Some(tail[end + 1].0.clone())
    } else {
        None
    };
    let values = &tail[..end];
    if values.len() == 1 && values[0] == ("[]".into(), false) {
        return Some((Vec::new(), note));
    }
    if values.is_empty()
        || values
            .iter()
            .any(|(id, quoted)| *quoted || crate::authoring::identifier(id).is_err())
    {
        return None;
    }
    let mut tags: Vec<_> = values.iter().map(|t| t.0.clone()).collect();
    tags.sort();
    tags.dedup();
    Some((tags, note))
}

pub(crate) fn parse_declaration(
    src: &str,
    file: &str,
    line: u32,
    diags: &mut Vec<Diagnostic>,
) -> StateDecl {
    let tokens = crate::catalog_syntax::tokenize(src, file, line, diags);
    let get = |i| {
        tokens
            .get(i)
            .map(|t: &(String, bool)| t.0.as_str())
            .unwrap_or("")
    };
    let content = tags_and_note(&tokens, 5);
    if crate::authoring::identifier(get(0)).is_err()
        || tokens.first().is_some_and(|t| t.1)
        || get(1) != "on"
        || !TARGET_KINDS.contains(&get(2))
        || get(3).is_empty()
        || get(4) != "with"
        || content.is_none()
    {
        error(
            diags,
            "P004",
            file,
            line,
            "状态格式: state ID on 对象类型 对象ID with 标签列表 [as \"名称\"]",
        );
    }
    let (tags, display) = content.unwrap_or_default();
    StateDecl {
        id: get(0).into(),
        display: display.unwrap_or_else(|| get(0).into()),
        target: TargetRef::new(get(2), get(3)),
        tags,
        file: file.into(),
        line,
    }
}

pub(crate) fn parse_change(
    src: &str,
    file: &str,
    line: u32,
    diags: &mut Vec<Diagnostic>,
) -> Change {
    let tokens = crate::catalog_syntax::tokenize(src, file, line, diags);
    let id = tokens.first().map(|t| t.0.clone()).unwrap_or_default();
    let content = tags_and_note(&tokens, 2);
    if crate::authoring::identifier(&id).is_err()
        || tokens.first().is_some_and(|t| t.1)
        || !tokens
            .get(1)
            .is_some_and(|t| matches!(t.0.as_str(), "with" | "add" | "remove") && !t.1)
        || content.is_none()
    {
        error(
            diags,
            "P004",
            file,
            line,
            "状态变更格式: become 状态ID with/add/remove 标签列表 [as \"说明\"]；清空写 with []",
        );
    }
    let (tags, note) = content.unwrap_or_default();
    Change {
        kind: match tokens.get(1).map(|t| t.0.as_str()) {
            Some("add") => ChangeKind::AddTags,
            Some("remove") => ChangeKind::RemoveTags,
            _ => ChangeKind::Become,
        },
        id,
        tags,
        note,
        to_storyline: None,
        loc: Loc::new(line, 1),
    }
}

pub(crate) fn collect_declarations(
    program: &Program,
    catalog: &mut Catalog,
    diags: &mut Vec<Diagnostic>,
) {
    for decl in &program.catalog {
        let CatalogDecl::State(state) = decl else {
            continue;
        };
        if catalog.states.contains_key(&state.id) {
            error(
                diags,
                "A216",
                &state.file,
                state.line,
                format!("状态 `{}` 重复定义", state.id),
            );
            continue;
        }
        let mut target = state.target.clone();
        if target.kind == "file" {
            target.id = crate::catalog::resolved_asset(&state.file, &target.id)
                .to_string_lossy()
                .into_owned();
        }
        catalog.add_object("state", &state.id, &state.display, &state.file, state.line);
        catalog.states.insert(
            state.id.clone(),
            StateInfo {
                id: state.id.clone(),
                display: state.display.clone(),
                target,
                tags: state.tags.clone(),
                file: state.file.clone(),
                line: state.line,
                changes: Vec::new(),
            },
        );
    }
    for state in catalog.states.values() {
        if catalog.object(&state.target).is_none() {
            error(
                diags,
                "A216",
                &state.file,
                state.line,
                format!("状态目标 {} {} 不存在", state.target.kind, state.target.id),
            );
        }
        validate_tags(&state.tags, &state.file, state.line, catalog, diags);
    }
    for state in catalog.states.values() {
        for target in std::iter::once(state.target.clone())
            .chain(state.tags.iter().map(|id| TargetRef::new("tag", id)))
        {
            catalog.references.push(ReferenceInfo {
                source: TargetRef::new("state", &state.id),
                target,
                kind: "状态定义".into(),
                file: state.file.clone(),
                line: state.line,
            });
        }
    }
}

fn validate_tags(
    tags: &[String],
    file: &str,
    line: u32,
    catalog: &Catalog,
    diags: &mut Vec<Diagnostic>,
) {
    for tag in tags {
        if !catalog.tags.get(tag).is_some_and(|t| t.declared) {
            error(
                diags,
                "A216",
                file,
                line,
                format!("状态内容引用了未声明标签 `{tag}`"),
            );
        }
    }
}

fn add_change(
    change: &Change,
    mut site: StateChangeSite,
    catalog: &mut Catalog,
    diags: &mut Vec<Diagnostic>,
) {
    if !matches!(
        change.kind,
        ChangeKind::Become | ChangeKind::AddTags | ChangeKind::RemoveTags
    ) {
        return;
    }
    site.kind = change.kind;
    site.line = change.loc.line;
    site.tags = change.tags.clone();
    site.note = change.note.clone();
    validate_tags(&change.tags, &site.file, site.line, catalog, diags);
    if let Some(state) = catalog.states.get_mut(&change.id) {
        state.changes.push(site.clone());
    } else {
        error(
            diags,
            "A216",
            &site.file,
            site.line,
            format!("状态 `{}` 未定义", change.id),
        );
    }
    for target in std::iter::once(TargetRef::new("state", &change.id))
        .chain(change.tags.iter().map(|id| TargetRef::new("tag", id)))
    {
        catalog.references.push(ReferenceInfo {
            source: TargetRef::new(
                if site.node == site.event {
                    "event"
                } else {
                    "scene"
                },
                &site.node,
            ),
            target,
            kind: "状态变更".into(),
            file: site.file.clone(),
            line: site.line,
        });
    }
}

pub(crate) fn collect_changes(
    program: &Program,
    catalog: &mut Catalog,
    diags: &mut Vec<Diagnostic>,
) {
    fn walk(
        body: &[Stmt],
        site: &StateChangeSite,
        catalog: &mut Catalog,
        diags: &mut Vec<Diagnostic>,
    ) {
        for stmt in body {
            match stmt {
                Stmt::Change(c) => add_change(&c.change, site.clone(), catalog, diags),
                Stmt::Scene(s) => {
                    let mut inner = site.clone();
                    inner.node = format!("{}.{}", site.node, s.name);
                    walk(&s.body, &inner, catalog, diags);
                }
                Stmt::Choice(c) => {
                    let mut inner = site.clone();
                    inner
                        .contexts
                        .push(format!("选择：{}（第 {} 行）", c.label_raw, c.loc.line));
                    walk(&c.body, &inner, catalog, diags);
                }
                Stmt::If(i) => {
                    for (index, (_, body)) in i.branches.iter().enumerate() {
                        let mut inner = site.clone();
                        inner.contexts.push(format!(
                            "第 {} 行条件的分支 {}",
                            i.loc.line,
                            index + 1
                        ));
                        walk(body, &inner, catalog, diags);
                    }
                }
                _ => {}
            }
        }
    }
    for (event, file) in program.events.iter().zip(&program.event_files) {
        let site = StateChangeSite {
            kind: ChangeKind::Become,
            event: event.name.clone(),
            node: event.name.clone(),
            timing: "during".into(),
            tags: Vec::new(),
            note: None,
            contexts: Vec::new(),
            file: file.clone(),
            line: event.loc.line,
        };
        walk(&event.body, &site, catalog, diags);
        for effect in &event.effects {
            let mut site = site.clone();
            site.timing = match effect.when {
                crate::ast::EffectWhen::Enter => "enter",
                crate::ast::EffectWhen::Exit => "exit",
                crate::ast::EffectWhen::Done => "done",
            }
            .into();
            if effect.cond.is_some() {
                site.contexts
                    .push(format!("受第 {} 行效果条件约束", effect.loc.line));
            }
            for action in &effect.actions {
                add_change(action, site.clone(), catalog, diags);
            }
        }
    }
    for state in catalog.states.values_mut() {
        state
            .changes
            .sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    }
}

pub(crate) fn check_has(
    args: &[Expr],
    loc: Loc,
    file: &str,
    program: &Program,
    diags: &mut Vec<Diagnostic>,
) {
    let names: Vec<_> = args
        .iter()
        .filter_map(|e| match e {
            Expr::Var { name, .. } | Expr::Str(name) => Some(name),
            _ => None,
        })
        .collect();
    if names.len() != 2 || args.len() != 2 {
        error(
            diags,
            "A103",
            file,
            loc.line,
            "has 需要两个名称参数: has(状态ID, 标签ID)",
        );
        return;
    }
    if !program
        .catalog
        .iter()
        .any(|d| matches!(d, CatalogDecl::State(s) if &s.id == names[0]))
    {
        error(
            diags,
            "A216",
            file,
            loc.line,
            format!("has 引用的状态 `{}` 未定义", names[0]),
        );
    }
    if !program
        .catalog
        .iter()
        .any(|d| matches!(d, CatalogDecl::Tag(t) if &t.name == names[1]))
    {
        error(
            diags,
            "A216",
            file,
            loc.line,
            format!("has 引用的标签 `{}` 未声明", names[1]),
        );
    }
}

#[derive(Clone)]
pub struct StateDraft {
    pub id: String,
    pub display: String,
    pub target: TargetRef,
    pub tags: Vec<String>,
}

impl crate::project::Project {
    pub fn write_state(
        &mut self,
        original: Option<&str>,
        draft: &StateDraft,
    ) -> Result<(), String> {
        crate::authoring::identifier(&draft.id)?;
        if original.is_some_and(|id| id != draft.id) {
            return Err("状态 ID 是追踪身份，请保留 ID，修改显示名称".into());
        }
        for tag in &draft.tags {
            crate::authoring::identifier(tag)?;
        }
        let result = crate::compile_sources(&self.entry, &self.sources());
        if original.is_none() && result.analysis.catalog.states.contains_key(&draft.id) {
            return Err("状态 ID 已存在".into());
        }
        let existing = original.and_then(|id| result.analysis.catalog.states.get(id));
        if original.is_some() && existing.is_none() {
            return Err("待修改的状态不存在".into());
        }
        let path = existing
            .map(|s| std::path::PathBuf::from(&s.file))
            .unwrap_or_else(|| self.entry.clone());
        let target = if draft.target.kind == "file" {
            crate::authoring::quote(&crate::catalog_edit::relative_source_path(
                path.parent().unwrap_or(&self.root),
                std::path::Path::new(&draft.target.id),
            )?)
        } else {
            draft.target.id.clone()
        };
        let tags = if draft.tags.is_empty() {
            "[]".into()
        } else {
            draft.tags.join(", ")
        };
        let out = format!(
            "state {} on {} {} with {} as {}\n",
            draft.id,
            draft.target.kind,
            target,
            tags,
            crate::authoring::quote(&draft.display)
        );
        let mut text = self.document(&path)?.to_string();
        if let Some(state) = existing {
            let mut lines: Vec<_> = text.split_inclusive('\n').map(str::to_string).collect();
            let index = state.line as usize - 1;
            lines[index] = format!("{}{out}", crate::authoring::comments(&lines[index]));
            text = lines.concat();
        } else {
            text.push_str(&format!("\n{out}"));
        }
        self.set_text(&path, text)
    }
}

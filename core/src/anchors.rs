//! 独立叙事锚点只保存意义与对象引用；变化出处始终借用状态索引。
use crate::authoring::{comments, identifier, quote};
use crate::catalog::{Catalog, CatalogDecl, ReferenceInfo, TargetRef};
use crate::lexer::{lex_source, LineKind};
use crate::project::Project;
use crate::states::{StateChangeSite, StateInfo};
use crate::{Diagnostic, Program, Span};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::PathBuf;

pub const ANCHOR_TARGET_KINDS: &[&str] = &["character", "event", "state", "anchor"];

#[derive(Debug, Clone, Serialize)]
pub struct AnchorLink {
    pub anchor: String,
    pub target: TargetRef,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnchorInfo {
    pub id: String,
    pub display: String,
    pub description: String,
    pub file: String,
    pub line: u32,
    pub links: Vec<AnchorLink>,
}

#[derive(Debug, Clone, Default)]
pub struct AnchorDraft {
    pub id: String,
    pub display: String,
    pub description: String,
    pub targets: Vec<TargetRef>,
}

impl From<&AnchorInfo> for AnchorDraft {
    fn from(anchor: &AnchorInfo) -> Self {
        Self {
            id: anchor.id.clone(),
            display: anchor.display.clone(),
            description: anchor.description.clone(),
            targets: anchor.links.iter().map(|l| l.target.clone()).collect(),
        }
    }
}

impl Catalog {
    /// 按完整对象反查直接关联的锚点，包括角色与事件。
    pub fn anchors_for(&self, target: &TargetRef) -> Vec<&AnchorInfo> {
        self.anchors
            .values()
            .filter(|a| a.links.iter().any(|l| &l.target == target))
            .collect()
    }

    /// 关联状态与关联事件的交集；缺少任意一类关联时没有变化出处。
    /// 返回现有状态索引的借用，不复制内容，也不把行号作为持久身份。
    pub fn anchor_changes(&self, id: &str) -> Vec<(&StateInfo, &StateChangeSite)> {
        let Some(anchor) = self.anchors.get(id) else {
            return Vec::new();
        };
        let targets: BTreeSet<_> = anchor.links.iter().map(|l| &l.target).collect();
        self.states
            .values()
            .filter(|s| targets.contains(&TargetRef::new("state", &s.id)))
            .flat_map(|s| {
                s.changes
                    .iter()
                    .filter(|c| targets.contains(&TargetRef::new("event", &c.event)))
                    .map(move |c| (s, c))
            })
            .collect()
    }
}

pub(crate) fn collect_declarations(
    program: &Program,
    catalog: &mut Catalog,
    diags: &mut Vec<Diagnostic>,
) {
    for decl in &program.catalog {
        let CatalogDecl::Anchor(anchor) = decl else {
            continue;
        };
        if let Some(old) = catalog.anchors.get(&anchor.name) {
            diags.push(
                Diagnostic::error(
                    "A217",
                    &anchor.file,
                    Span::new(anchor.loc.line, 1, 10),
                    format!("独立锚点 `{}` 重复定义", anchor.name),
                )
                .with_related(&old.file, Span::new(old.line, 1, 10)),
            );
            continue;
        }
        let display = anchor.display.as_deref().unwrap_or(&anchor.name);
        catalog.add_object(
            "anchor",
            &anchor.name,
            display,
            &anchor.file,
            anchor.loc.line,
        );
        catalog.anchors.insert(
            anchor.name.clone(),
            AnchorInfo {
                id: anchor.name.clone(),
                display: display.into(),
                description: anchor.description.clone(),
                file: anchor.file.clone(),
                line: anchor.loc.line,
                links: Vec::new(),
            },
        );
    }
}

pub(crate) fn collect_links(program: &Program, catalog: &mut Catalog, diags: &mut Vec<Diagnostic>) {
    for decl in &program.catalog {
        let CatalogDecl::AnchorLink(link) = decl else {
            continue;
        };
        let source = TargetRef::new("anchor", &link.anchor);
        for target in [&source, &link.target] {
            if catalog.object(target).is_none() {
                diags.push(Diagnostic::error(
                    "A217",
                    &link.file,
                    Span::new(link.line, 1, 11),
                    format!("锚点关联的对象 {} {} 不存在", target.kind, target.id),
                ));
            }
        }
        if let Some(anchor) = catalog.anchors.get_mut(&link.anchor) {
            anchor.links.push(link.clone());
        }
        catalog.references.push(ReferenceInfo {
            source,
            target: link.target.clone(),
            kind: "锚点关联".into(),
            file: link.file.clone(),
            line: link.line,
        });
    }
}

fn validate_targets(targets: &[TargetRef]) -> Result<(), String> {
    for target in targets {
        if !ANCHOR_TARGET_KINDS.contains(&target.kind.as_str()) {
            return Err("锚点只能关联角色、事件、状态或独立锚点".into());
        }
        identifier(&target.id)?;
    }
    Ok(())
}

impl Project {
    /// 与其他结构编辑一样，由调用方用 Project.edit 校验并提交整次修改。
    pub fn write_anchor(
        &mut self,
        original: Option<&str>,
        draft: &AnchorDraft,
    ) -> Result<(), String> {
        identifier(&draft.id)?;
        validate_targets(&draft.targets)?;
        if original.is_some_and(|id| id != draft.id) {
            return Err("锚点 ID 是引用身份，请保留 ID，修改显示名称与叙事意义".into());
        }
        let result = self.compile();
        let existing = result.analysis.catalog.anchors.get(&draft.id);
        if original.is_none() && existing.is_some() {
            return Err("锚点 ID 已存在".into());
        }
        if original.is_some() && existing.is_none() {
            return Err("待修改的锚点不存在".into());
        }
        let path = existing
            .map(|a| PathBuf::from(&a.file))
            .unwrap_or_else(|| self.entry.clone());
        let mut text = self.document(&path)?.to_string();
        let out = format!(
            "anchor_def {} as {}\n  description {}\n",
            draft.id,
            quote(&draft.display),
            quote(&draft.description)
        );
        if let Some(anchor) = existing {
            let parsed = lex_source(&path.to_string_lossy(), &text, &mut Vec::new());
            let index = parsed
                .iter()
                .position(|l| l.no == anchor.line)
                .ok_or("锚点声明源位置不存在")?;
            let offset = |line: u32| {
                text.split_inclusive('\n')
                    .take(line.saturating_sub(1) as usize)
                    .map(str::len)
                    .sum::<usize>()
            };
            let start = offset(anchor.line);
            let end = parsed
                .iter()
                .skip(index + 1)
                .find(|l| l.indent == 0)
                .map(|l| offset(l.no))
                .unwrap_or(text.len());
            let retained = comments(&text[start..end]);
            text.replace_range(start..end, &format!("{retained}{out}"));
        } else {
            text = format!("{out}\n{text}");
        }
        self.set_text(&path, text)?;
        self.set_anchor_links(&draft.id, &draft.targets)
    }

    /// 替换此锚点在全部工程缓冲中的关联；注释与其他锚点保持存在。
    pub fn set_anchor_links(&mut self, id: &str, targets: &[TargetRef]) -> Result<(), String> {
        identifier(id)?;
        validate_targets(targets)?;
        let result = self.compile();
        let anchor = result
            .analysis
            .catalog
            .anchors
            .get(id)
            .ok_or("锚点不存在")?;
        let destination = PathBuf::from(&anchor.file);
        self.document(&destination)?;
        for (path, document) in &mut self.documents {
            let parsed = lex_source(&path.to_string_lossy(), &document.text, &mut Vec::new());
            let remove: BTreeSet<_> = parsed
                .iter()
                .filter_map(|line| match &line.kind {
                    LineKind::Catalog(CatalogDecl::AnchorLink(link)) if link.anchor == id => {
                        Some(line.no)
                    }
                    _ => None,
                })
                .collect();
            if remove.is_empty() {
                continue;
            }
            // 整文件剥离注释用于定位，保留跨行块注释的边界。
            let cleaned = crate::lexer::strip_comments(&document.text);
            document.text = document
                .text
                .split_inclusive('\n')
                .zip(cleaned.split_inclusive('\n'))
                .enumerate()
                .map(|(index, (raw, clean))| {
                    if remove.contains(&(index as u32 + 1)) {
                        let retained = raw
                            .chars()
                            .zip(clean.chars())
                            .map(|(raw, clean)| {
                                if raw != clean || raw.is_whitespace() {
                                    raw
                                } else {
                                    ' '
                                }
                            })
                            .collect::<String>();
                        if retained.trim().is_empty() {
                            String::new()
                        } else {
                            retained
                        }
                    } else {
                        raw.to_string()
                    }
                })
                .collect();
        }
        let mut links = String::new();
        for target in targets.iter().collect::<BTreeSet<_>>() {
            links.push_str(&format!("anchor_link {id} {} {}\n", target.kind, target.id));
        }
        let text = format!("{links}{}", self.document(&destination)?);
        self.set_text(&destination, text)
    }
}

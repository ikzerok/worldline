use super::{source_relative, QuerySource};
use crate::catalog::TargetRef;
use crate::collaboration::{AnchorStatus, CommentAnchor, ProposalStatus};
use crate::project::Project;
use crate::Diagnostic;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoKind {
    BrokenLink,
    EntryToCreate,
    DetachedComment,
    OpenProposal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TodoItem {
    pub id: String,
    pub kind: TodoKind,
    /// 被检查的缺失对象，或产生待办的批注/提案身份。
    pub target: TargetRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_target: Option<TargetRef>,
    pub source: QuerySource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TodoProjection {
    pub schema_version: u32,
    pub snapshot: String,
    pub items: Vec<TodoItem>,
    pub diagnostics: Vec<Diagnostic>,
}
impl Project {
    /// 汇总当前缓冲中的明确未完成事项；只读，不为投影创建持久身份或修改来源文档。
    pub fn todo_projection(&self) -> TodoProjection {
        let content = self.compile_current();
        let maps = crate::presentation_commands::map_index_with_content(self, &content);
        let comments = crate::collaboration::build_comment_index(self, &content, &maps);
        let proposals = crate::collaboration::build_proposal_index(self);
        let mut diagnostics = content.diagnostics.clone();
        diagnostics.extend(maps.diagnostics.iter().cloned());
        diagnostics.extend(comments.diagnostics.iter().cloned());
        diagnostics.extend(proposals.diagnostics.iter().cloned());

        let mut items = Vec::new();
        let mut missing_targets: BTreeMap<TargetRef, Vec<&crate::navigation::TextLinkInfo>> =
            BTreeMap::new();
        for link in &content.analysis.catalog.text_links {
            if content.analysis.catalog.object(&link.target).is_some() {
                continue;
            }
            let source = QuerySource {
                file: link.file.clone(),
                line: link.line,
            };
            let source_path = source_relative(&self.root, &link.file);
            items.push(TodoItem {
                id: todo_id(
                    TodoKind::BrokenLink,
                    &source_path,
                    link.line,
                    Some(link.column),
                    &link.target,
                    "",
                ),
                kind: TodoKind::BrokenLink,
                target: link.target.clone(),
                related_target: Some(link.source.clone()),
                source,
                column: Some(link.column),
                reason: format!(
                    "正文链接指向不存在的 {} `{}`",
                    link.target.kind, link.target.id
                ),
            });
            missing_targets
                .entry(link.target.clone())
                .or_default()
                .push(link);
        }
        for (target, mut links) in missing_targets {
            links.sort_by(|left, right| {
                (&left.file, left.line, left.column).cmp(&(&right.file, right.line, right.column))
            });
            let first = links[0];
            let source_path = source_relative(&self.root, &first.file);
            items.push(TodoItem {
                id: todo_id(
                    TodoKind::EntryToCreate,
                    &source_path,
                    first.line,
                    Some(first.column),
                    &target,
                    "",
                ),
                kind: TodoKind::EntryToCreate,
                target: target.clone(),
                related_target: None,
                source: QuerySource {
                    file: first.file.clone(),
                    line: first.line,
                },
                column: Some(first.column),
                reason: format!(
                    "缺失条目 {} `{}` 被 {} 处正文链接引用",
                    target.kind,
                    target.id,
                    links.len()
                ),
            });
        }

        for comment in comments.comments.values().filter(|comment| {
            !comment.draft.resolved && comment.anchor_status == AnchorStatus::Detached
        }) {
            let related_target = match &comment.draft.anchor {
                CommentAnchor::Object { target } => Some(target.clone()),
                CommentAnchor::MapPlacement { map_id, .. } => Some(TargetRef::new("map", map_id)),
                CommentAnchor::TextRange { path, .. } => Some(TargetRef::new("file", path)),
            };
            let source_file = comment.path.to_string_lossy().into_owned();
            let line = authoring_document_line(self, &comment.path, &comment.draft.id);
            let source_path = source_relative(&self.root, &source_file);
            let target = TargetRef::new("comment", &comment.draft.id);
            items.push(TodoItem {
                id: todo_id(
                    TodoKind::DetachedComment,
                    &source_path,
                    line,
                    None,
                    &target,
                    &comment.draft.id,
                ),
                kind: TodoKind::DetachedComment,
                target,
                related_target,
                source: QuerySource {
                    file: source_file,
                    line,
                },
                column: None,
                reason: format!("未解决批注 `{}` 的锚点已失效", comment.draft.id),
            });
        }

        for proposal in proposals
            .proposals
            .values()
            .filter(|proposal| proposal.draft.status == ProposalStatus::Open)
        {
            let source_file = proposal.path.to_string_lossy().into_owned();
            let line = authoring_document_line(self, &proposal.path, &proposal.draft.id);
            let source_path = source_relative(&self.root, &source_file);
            let target = TargetRef::new("proposal", &proposal.draft.id);
            items.push(TodoItem {
                id: todo_id(
                    TodoKind::OpenProposal,
                    &source_path,
                    line,
                    None,
                    &target,
                    &proposal.draft.id,
                ),
                kind: TodoKind::OpenProposal,
                target,
                related_target: None,
                source: QuerySource {
                    file: source_file,
                    line,
                },
                column: None,
                reason: format!("提案 `{}` 等待审阅", proposal.draft.id),
            });
        }
        items.sort_by(|left, right| {
            (
                left.kind,
                &left.target,
                &left.source.file,
                left.source.line,
                left.column,
                &left.id,
            )
                .cmp(&(
                    right.kind,
                    &right.target,
                    &right.source.file,
                    right.source.line,
                    right.column,
                    &right.id,
                ))
        });
        crate::sort_diagnostics(&mut diagnostics);
        TodoProjection {
            schema_version: 1,
            snapshot: self.content_baseline(),
            items,
            diagnostics,
        }
    }
}
fn todo_id(
    kind: TodoKind,
    source_path: &str,
    line: u32,
    column: Option<u32>,
    target: &TargetRef,
    extra: &str,
) -> String {
    let identity = format!(
        "{kind:?}\0{source_path}\0{line}\0{column:?}\0{}\0{}\0{extra}",
        target.kind, target.id
    );
    let mut hash = 0xcbf29ce484222325u64;
    for byte in identity.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("todo_{hash:016x}")
}

fn authoring_document_line(project: &Project, path: &Path, id: &str) -> u32 {
    let Ok(document) = project.authoring_document(path) else {
        return 1;
    };
    let marker = format!("\"id\": \"{id}\"");
    String::from_utf8_lossy(document.bytes())
        .lines()
        .position(|line| line.contains(&marker))
        .map(|line| line as u32 + 1)
        .unwrap_or(1)
}

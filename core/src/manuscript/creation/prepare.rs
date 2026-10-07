use super::*;
use crate::authoring::EventDraft;
use crate::source_lifecycle::safety;
use crate::workspace_documents::{manifest_path, parse_registry};
use std::path::Path;

pub(super) fn prepare(
    project: &Project,
    revision: Revision,
    request: &ManuscriptChapterCreateRequest,
) -> Result<(Project, ManuscriptChapterCreatePlan), Failure> {
    guards::request(request)?;
    if request.expected_baseline != project.content_baseline()
        || request.expected_revision != revision
    {
        return Err(Failure::new(
            Code::StaleBaseline,
            "新章基线或修订已过期；请保留输入并重新预览",
        ));
    }
    let guard = guards::workspace(project)?;
    let before = project.compile_current();
    if before.has_errors() {
        return Err(Failure::new(
            Code::SourceUnavailable,
            "新章创建需要完整可分析的当前工程；请先修复源码诊断，输入已保留",
        ));
    }
    let (mut draft, original) = book(project, request)?;
    if draft
        .entries
        .iter()
        .any(|entry| entry.id == request.chapter.id)
    {
        return Err(Failure::new(
            Code::IdConflict,
            format!("章节 ID `{}` 已存在", request.chapter.id),
        ));
    }
    let at = insertion_index(&draft, &request.chapter)?;
    guards::destinations(project, request)?;
    let mut candidate = project.clone();
    let (target, source_path, new_source) = source(project, &mut candidate, request, &before)?;
    draft.entries.insert(
        at,
        ManuscriptEntryDraft {
            id: request.chapter.id.clone(),
            title: request.chapter.title.clone(),
            kind: ManuscriptEntryKind::Chapter,
            parent_id: request.chapter.parent_section_id.clone(),
            summary: None,
            pov: None,
            status: None,
            goal: None,
            target_ref: Some(target.clone()),
        },
    );
    let book_id = draft.id.clone();
    let command = ManuscriptCommand {
        expected_revision: revision,
        expected_baseline: candidate.content_baseline(),
        original,
        draft,
    };
    let (candidate, index, _) = candidate
        .prepare_manuscript(revision, &command)
        .map_err(|m| Failure::new(Code::InvalidChapter, m))?;
    if index
        .entries
        .iter()
        .find(|entry| entry.id == request.chapter.id)
        .and_then(|entry| entry.source.as_ref())
        .is_none_or(|source| source.status != ManuscriptReferenceStatus::Resolved)
    {
        return Err(Failure::new(
            Code::SourceUnavailable,
            "新章节正式来源无法确认；整笔未提交",
        ));
    }
    let after = candidate.compile_current();
    if after.has_errors() || before.program.entry != after.program.entry {
        return Err(Failure::new(
            Code::SourceUnavailable,
            "新章候选有源码错误或改变了入口；整笔未提交",
        ));
    }
    if matches!(request.source, ManuscriptChapterSource::Existing { .. })
        && before.analysis.fingerprint != after.analysis.fingerprint
    {
        return Err(Failure::new(
            Code::SourceUnavailable,
            "复用来源的书稿编排意外改变运行指纹；整笔未提交",
        ));
    }
    guards::budget(&candidate)?;
    let changes = changes(project, &candidate)?;
    let changed_files = changes
        .iter()
        .map(|change| change.path.clone())
        .collect::<Vec<_>>();
    for path in &changed_files {
        safety::writable_path(&project.root.join(path))
            .map_err(|m| Failure::new(Code::ReadOnly, m))?;
    }
    let registry = parse_registry(
        &project.root,
        candidate.authoring_documents[&manifest_path(&project.root)].bytes(),
    );
    let manuscript_path = relative(project, &registry.manuscripts[&book_id])?;
    let mut plan = ManuscriptChapterCreatePlan {
        schema_version: 1,
        workspace: project.root.clone(),
        baseline: project.content_baseline(),
        revision,
        book_id,
        chapter_id: request.chapter.id.clone(),
        target,
        source_path,
        new_source,
        manuscript_path,
        changed_files,
        changes,
        diagnostics: after.diagnostics,
        entry_before: before.program.entry,
        entry_after: after.program.entry,
        runtime_fingerprint_before: before.analysis.fingerprint,
        runtime_fingerprint_after: after.analysis.fingerprint,
        can_apply: true,
        plan_digest: String::new(),
        guard,
    };
    let payload = serde_json::to_vec(&(request, &plan, &plan.guard, candidate.content_baseline()))
        .map_err(|e| Failure::new(Code::InvalidChapter, e.to_string()))?;
    plan.plan_digest = crate::presentation_commands::document_hash(&payload);
    Ok((candidate, plan))
}

fn book(
    project: &Project,
    request: &ManuscriptChapterCreateRequest,
) -> Result<(ManuscriptDraft, Option<String>), Failure> {
    let indices = project.manuscript_indices();
    match &request.book {
        ManuscriptBookDestination::Existing { id } => {
            let index = indices
                .get(id)
                .ok_or_else(|| Failure::new(Code::InvalidChapter, format!("书稿 `{id}` 未注册")))?;
            if index.read_only {
                return Err(Failure::new(Code::ReadOnly, format!("书稿 `{id}` 为只读")));
            }
            Ok((ManuscriptDraft::from_index(index), Some(id.clone())))
        }
        ManuscriptBookDestination::New { id, title } => {
            if indices.contains_key(id) {
                return Err(Failure::new(
                    Code::IdConflict,
                    format!("书稿 ID `{id}` 已注册"),
                ));
            }
            Ok((
                ManuscriptDraft {
                    id: id.clone(),
                    title: title.clone(),
                    entries: Vec::new(),
                },
                None,
            ))
        }
    }
}

fn insertion_index(
    draft: &ManuscriptDraft,
    chapter: &ManuscriptChapterDraft,
) -> Result<usize, Failure> {
    if let Some(parent) = &chapter.parent_section_id {
        if !draft
            .entries
            .iter()
            .any(|entry| &entry.id == parent && entry.kind == ManuscriptEntryKind::Section)
        {
            return Err(Failure::new(
                Code::InvalidChapter,
                "新章父项必须是当前书稿中存在的 section",
            ));
        }
    }
    if let Some(after) = &chapter.after_sibling_id {
        return draft
            .entries
            .iter()
            .position(|entry| &entry.id == after && entry.parent_id == chapter.parent_section_id)
            .map(|at| at + 1)
            .ok_or_else(|| {
                Failure::new(Code::InvalidChapter, "插入位置必须是同一父项下的已有兄弟")
            });
    }
    Ok(draft
        .entries
        .iter()
        .rposition(|entry| entry.parent_id == chapter.parent_section_id)
        .map_or(draft.entries.len(), |at| at + 1))
}

fn source(
    project: &Project,
    candidate: &mut Project,
    request: &ManuscriptChapterCreateRequest,
    before: &crate::CompileResult,
) -> Result<(TargetRef, PathBuf, bool), Failure> {
    match &request.source {
        ManuscriptChapterSource::Existing { target } => {
            if !matches!(
                target.kind.as_str(),
                "event" | "scene" | "entity" | "fragment"
            ) {
                return Err(Failure::new(
                    Code::SourceUnavailable,
                    "章节来源只支持 event、scene、entity 或 fragment",
                ));
            }
            let object = before.analysis.catalog.object(target).ok_or_else(|| {
                Failure::new(
                    Code::SourceUnavailable,
                    format!(
                        "正式来源 {}:{} 不存在或不受当前语言支持",
                        target.kind, target.id
                    ),
                )
            })?;
            let path = Path::new(&object.file);
            let path = if path.is_absolute() {
                path.to_owned()
            } else {
                project.root.join(path)
            };
            if !project.sources().contains_key(&path) {
                return Err(Failure::new(
                    Code::SourceUnavailable,
                    "章节来源不是当前活动源码",
                ));
            }
            Ok((target.clone(), relative(project, &path)?, false))
        }
        ManuscriptChapterSource::NewEvent {
            id,
            destination,
            storyline,
        } => {
            if before.program.event_index(id).is_some() {
                return Err(Failure::new(
                    Code::IdConflict,
                    format!("事件 ID `{id}` 已存在"),
                ));
            }
            let (path, new_source) = match destination {
                ManuscriptSourceDestination::ExistingActiveSource { relative_path } => {
                    (project.root.join(relative_path), false)
                }
                ManuscriptSourceDestination::NewActiveSource { relative_path } => {
                    let path =
                        safety::destination(project, relative_path).map_err(guards::failure)?;
                    candidate
                        .add_file_candidate(relative_path, &path)
                        .map_err(|m| Failure::new(Code::InvalidDestination, m))?;
                    candidate
                        .set_text(&path, String::new())
                        .map_err(|m| Failure::new(Code::SourceUnavailable, m))?;
                    (path, true)
                }
            };
            let draft = EventDraft {
                id: id.clone(),
                storyline: storyline.clone(),
                body: "-> END\n".into(),
                ..EventDraft::default()
            };
            candidate
                .write_event_candidate(&path, &draft)
                .map_err(|m| Failure::new(Code::SourceUnavailable, m))?;
            Ok((
                TargetRef::new("event", id),
                relative(project, &path)?,
                new_source,
            ))
        }
    }
}

fn relative(project: &Project, path: &Path) -> Result<PathBuf, Failure> {
    path.strip_prefix(&project.root)
        .map(Path::to_path_buf)
        .map_err(|_| Failure::new(Code::InvalidDestination, "来源路径不在工作区内"))
}

fn changes(before: &Project, after: &Project) -> Result<Vec<ManuscriptChapterChange>, Failure> {
    let mut paths = std::collections::BTreeSet::new();
    paths.extend(after.documents.keys());
    paths.extend(after.authoring_documents.keys());
    let mut changes = Vec::new();
    let mut bytes = 0usize;
    for path in paths {
        let old = before.tracked_file_state(path).and_then(|s| s.current);
        let new = after.tracked_file_state(path).and_then(|s| s.current);
        if old == new {
            continue;
        }
        let new =
            new.ok_or_else(|| Failure::new(Code::InvalidChapter, "创建新章不能删除既有文件"))?;
        bytes = bytes
            .saturating_add(old.as_ref().map_or(0, Vec::len))
            .saturating_add(new.len());
        if bytes > 8 * 1024 * 1024 {
            return Err(Failure::new(
                Code::BudgetExceeded,
                "新章完整变更预览超过 8 MiB，整笔未提交",
            ));
        }
        let decode = |bytes| {
            String::from_utf8(bytes)
                .map_err(|_| Failure::new(Code::InvalidChapter, "变更文档必须为完整 UTF-8"))
        };
        changes.push(ManuscriptChapterChange {
            path: relative(before, path)?,
            before: old.map(decode).transpose()?,
            after: decode(new)?,
        });
    }
    Ok(changes)
}

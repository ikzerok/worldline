use super::*;
use crate::problems::{ProblemCoverageState, ProblemDomain, ProblemsOptions, ProblemsReport};
use crate::workspace_documents::{document_read_only, manifest_path, parse_registry};
use std::collections::BTreeMap;

impl Project {
    pub fn preview_reconciliation(
        &self,
        session: &ReconciliationSession,
        request: &ReconciliationRequest,
    ) -> Result<ReconciliationPlan, String> {
        self.preview_reconciliation_with_progress(session, request, &mut |_| true)
    }

    pub fn preview_reconciliation_with_progress(
        &self,
        session: &ReconciliationSession,
        request: &ReconciliationRequest,
        progress: Progress<'_>,
    ) -> Result<ReconciliationPlan, String> {
        let current = self.capture_reconciliation_with_progress(progress)?;
        if &current != session {
            return Err("外部改稿会话已过期，原候选已保留；请重新捕获后核对".into());
        }
        let (candidate, files, mut blockers) = build_candidate(self, session, request, progress)?;
        let unresolved = files.iter().filter(|file| !file.resolved).count();
        checkpoint(progress, ReconciliationStage::Validate)?;
        let before = self.compile_problems_snapshot();
        let after = candidate.compile_problems_snapshot();
        let problems = candidate
            .problems_report_with_progress(&ProblemsOptions::default(), &mut |_| {
                progress(ReconciliationStage::Validate)
            })
            .map_err(|error| error.to_string())?;
        report_blockers(&problems, request.allow_incomplete_source, &mut blockers);
        if unresolved > 0 {
            blockers.push(format!("还有 {unresolved} 个文件未明确选择候选"));
        }
        if files.is_empty() {
            blockers.push("没有需要采纳的普通外改冲突".into());
        }
        checkpoint(progress, ReconciliationStage::Revalidate)?;
        if capture::capture_guard(self, progress)? != session.guard {
            return Err("预览期间工程或磁盘再次变化；原候选已保留，请重新捕获".into());
        }
        let mut plan = ReconciliationPlan {
            schema_version: 1,
            session_digest: session.session_digest.clone(),
            plan_digest: String::new(),
            expected_baseline: self.content_baseline(),
            candidate_baseline: candidate.content_baseline(),
            request: request.clone(),
            files,
            unresolved,
            can_apply: blockers.is_empty(),
            blockers,
            problems,
            source_has_errors: after.has_errors(),
            runtime_fingerprint_before: before.analysis.fingerprint,
            runtime_fingerprint_after: after.analysis.fingerprint,
            session: session.clone(),
        };
        plan.plan_digest = digest(&plan)?;
        Ok(plan)
    }
}

pub(super) fn build_candidate(
    project: &Project,
    session: &ReconciliationSession,
    request: &ReconciliationRequest,
    progress: Progress<'_>,
) -> Result<(Project, Vec<ReconciliationCandidate>, Vec<String>), String> {
    if request.choices.len() > session.files.len() {
        return Err("候选选择超过本会话冲突文件数".into());
    }
    let mut choices = BTreeMap::new();
    let mut bytes = 0;
    for decision in &request.choices {
        if relative(&project.root, &project.root.join(&decision.path))? != decision.path {
            return Err("候选须使用本会话准确相对路径".into());
        }
        if !session.files.iter().any(|file| file.path == decision.path)
            || choices.insert(&decision.path, &decision.choice).is_some()
        {
            return Err("候选包含重复或不属于本会话的文件".into());
        }
        if let ReconciliationChoice::Manual { text } = &decision.choice {
            capture::budget(&mut bytes, text.len())?;
        }
    }
    let mut candidate = project.clone();
    let mut files = Vec::new();
    let mut blockers = session.blockers.clone();
    for file in &session.files {
        checkpoint(progress, ReconciliationStage::Candidate)?;
        let choice = choices.get(&file.path).copied();
        let result = match choice {
            Some(ReconciliationChoice::Baseline) => file.baseline.clone(),
            Some(ReconciliationChoice::Local) | None => file.local.clone(),
            Some(ReconciliationChoice::Disk) => file.disk.clone(),
            Some(ReconciliationChoice::Manual { text }) => Some(text.as_bytes().to_vec()),
            Some(ReconciliationChoice::Delete) => None,
        };
        if let Some(reason) = &file.protected_reason {
            blockers.push(format!("{}：{reason}", file.path.display()));
        } else if let Err(error) = set_candidate(&mut candidate, file, result.as_deref()) {
            blockers.push(format!("{}：{error}", file.path.display()));
        }
        files.push(ReconciliationCandidate {
            path: file.path.clone(),
            authoring: file.authoring,
            result,
            resolved: choice.is_some(),
        });
    }
    if let Err(error) = reconcile_registry(&mut candidate, session) {
        blockers.push(error);
    }
    if candidate
        .documents
        .get(&candidate.entry)
        .is_none_or(|document| document.deleted)
    {
        blockers.push("不能删除工程入口；请保留入口文件再处理正文".into());
    }
    crate::source_lifecycle::safety::buffer_budget(&candidate)?;
    Ok((candidate, files, blockers))
}

fn set_candidate(
    project: &mut Project,
    file: &ReconciliationFile,
    result: Option<&[u8]>,
) -> Result<(), String> {
    let path = project.root.join(&file.path);
    if file.authoring {
        if let Some(bytes) = result {
            capture::supported_json(bytes)?;
        }
        let document = project
            .authoring_documents
            .get_mut(&path)
            .ok_or("已注册文件身份消失")?;
        if let Some(bytes) = result {
            document.bytes = bytes.to_vec();
        }
        document.deleted = result.is_none();
        document.saved = file.disk.clone();
    } else {
        let document = project.documents.get_mut(&path).ok_or("源码文件身份消失")?;
        if let Some(bytes) = result {
            document.text = std::str::from_utf8(bytes)
                .map_err(|_| "候选源码不是UTF-8")?
                .into();
        }
        document.deleted = result.is_none();
        document.saved = file
            .disk
            .as_ref()
            .map(|bytes| String::from_utf8(bytes.clone()))
            .transpose()
            .map_err(|_| "磁盘源码不是UTF-8，不能推进保存基线")?;
    }
    Ok(())
}

fn reconcile_registry(
    project: &mut Project,
    session: &ReconciliationSession,
) -> Result<(), String> {
    let manifest = manifest_path(&project.root);
    let registry = project
        .authoring_documents
        .get(&manifest)
        .filter(|doc| !doc.deleted)
        .map(|doc| parse_registry(&project.root, &doc.bytes))
        .unwrap_or_default();
    if !registry.diagnostics.is_empty() {
        return Err("候选清单注册、语言版本或必需能力无效，不能采纳".into());
    }
    let mut remove = Vec::new();
    for (path, document) in &project.authoring_documents {
        if !registry.is_registered(path) && path != &manifest && !document.deleted {
            if document.is_dirty() {
                return Err(format!(
                    "候选清单移除了仍有本地稿的登记文件：{}",
                    relative(&project.root, path)?.display()
                ));
            }
            remove.push(path.clone());
        }
    }
    for path in remove {
        project.authoring_documents.remove(&path);
    }
    for (path, inherited) in &registry.documents {
        if !project.authoring_documents.contains_key(path) {
            let document = match session.guard.disk.get(path) {
                Some(bytes) => AuthoringDocument::from_disk(bytes.clone(), *inherited),
                None => AuthoringDocument::missing(*inherited),
            };
            project.authoring_documents.insert(path.clone(), document);
        }
        let document = project
            .authoring_documents
            .get_mut(path)
            .ok_or("候选注册文件消失")?;
        if !document.deleted {
            capture::supported_json(&document.bytes)?;
            document.read_only = document_read_only(&document.bytes, *inherited);
            if document.read_only {
                return Err("候选注册文档为只读，不能采纳".into());
            }
        }
    }
    project.language_version = registry.language_version;
    project.source_selection = registry.source_selection;
    project.authoring_diagnostics = registry.diagnostics;
    Ok(())
}

fn report_blockers(report: &ProblemsReport, allow_source_errors: bool, blockers: &mut Vec<String>) {
    if report.truncated || report.read_only {
        blockers.push("完整候选报告被截断或工作区只读，不能采纳".into());
    }
    let source_errors = report.content_has_errors;
    if source_errors && !allow_source_errors {
        blockers.push("候选源码有错误；可继续修稿，或明确选择“保留为未完成源码”".into());
    }
    if report.entries.iter().any(|entry| {
        entry.severity == crate::Severity::Error && entry.domain != ProblemDomain::Content
    }) {
        blockers.push("已注册创作文档存在错误；正文未完成确认不豁免注册文档保护".into());
    }
    let only_source_incomplete = source_errors
        && allow_source_errors
        && report
            .reasons
            .iter()
            .all(|reason| reason == "coverage_incomplete")
        && report.coverage.iter().all(|coverage| {
            !matches!(
                coverage.state,
                ProblemCoverageState::Partial | ProblemCoverageState::Unavailable
            ) || (coverage.state == ProblemCoverageState::Partial
                && !coverage.reasons.is_empty()
                && coverage.reasons.iter().all(|reason| {
                    matches!(
                        reason.as_str(),
                        "content_incomplete" | "documents_incomplete"
                    )
                }))
        });
    if !report.complete && !only_source_incomplete {
        blockers.push("完整候选尚有不能确认的覆盖范围，不能采纳".into());
    }
}

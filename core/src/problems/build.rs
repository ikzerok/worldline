use super::*;
use crate::{
    project::Project,
    workspace_documents::{manifest_path, parse_registry},
    CompileResult, Diagnostic,
};
use std::collections::BTreeMap;

impl Project {
    pub fn problems_report(
        &self,
        options: &ProblemsOptions,
    ) -> Result<ProblemsReport, ProblemsError> {
        self.problems_report_with_progress(options, &mut |_| true)
    }
    pub fn problems_report_with_progress(
        &self,
        options: &ProblemsOptions,
        progress: &mut dyn FnMut(ProblemDomain) -> bool,
    ) -> Result<ProblemsReport, ProblemsError> {
        options.validate()?;
        checkpoint(progress, ProblemDomain::Content)?;
        let content = self.compile_problems_snapshot();
        build(self, &content, options, progress, 1)
    }
    pub fn problems_report_with_content(
        &self,
        content: &CompileResult,
        expected_baseline: &str,
        options: &ProblemsOptions,
    ) -> Result<ProblemsReport, ProblemsError> {
        options.validate()?;
        let sources = self.sources();
        let entry = self.entry.to_string_lossy();
        if self.content_baseline() != expected_baseline
            || content.sources != sources
            || (sources.contains_key(&self.entry)
                && content.program.files.first().map(String::as_str) != Some(entry.as_ref()))
            || content.options != self.compile_options()
            || !super::observation::assets_current(self, content)
        {
            return Err(ProblemsError::new(
                "STALE_REPORT",
                "活动内容快照不属于当前已应用缓冲",
            ));
        }
        build(self, content, options, &mut |_| true, 0)
    }
}
fn checkpoint(
    progress: &mut dyn FnMut(ProblemDomain) -> bool,
    domain: ProblemDomain,
) -> Result<(), ProblemsError> {
    if !progress(domain) {
        return Err(ProblemsError::new("CANCELLED", "工程问题检查已取消"));
    }
    Ok(())
}
fn build(
    project: &Project,
    content: &CompileResult,
    options: &ProblemsOptions,
    progress: &mut dyn FnMut(ProblemDomain) -> bool,
    compile_count: u32,
) -> Result<ProblemsReport, ProblemsError> {
    let manifest = manifest_path(&project.root);
    let registry = project
        .authoring_documents
        .get(&manifest)
        .filter(|d| !d.is_deleted())
        .map(|d| parse_registry(&project.root, d.bytes()))
        .unwrap_or_default();
    let mut report = ProblemsReport {
        schema_version: 1,
        report_version: "0000000000000000".into(),
        content_baseline: project.content_baseline(),
        source_observation: String::new(),
        language_version: project.language_version().into(),
        content_has_errors: content.has_errors(),
        read_only: !project.authoring_diagnostics().is_empty(),
        complete: true,
        truncated: false,
        reasons: Vec::new(),
        coverage: super::coverage::build(project, content, &registry),
        entries: Vec::new(),
        related: BTreeMap::new(),
        limits: options.clone(),
        compile_count,
    };
    match project.problems_observation_key() {
        Ok(key) => report.source_observation = key,
        Err(_) => report
            .reasons
            .push("external_observation_unavailable".into()),
    }
    let mut diagnostics = Vec::<(ProblemDomain, Diagnostic)>::new();
    let mut add = |domain, values: Vec<Diagnostic>| {
        diagnostics.extend(values.into_iter().map(|d| {
            (
                if d.code.starts_with("WS") {
                    ProblemDomain::Workspace
                } else {
                    domain
                },
                d,
            )
        }));
    };
    add(ProblemDomain::Content, content.diagnostics.clone());
    checkpoint(progress, ProblemDomain::Workspace)?;
    add(
        ProblemDomain::Workspace,
        project.authoring_diagnostics().to_vec(),
    );
    add(ProblemDomain::Workspace, registry.diagnostics.clone());
    checkpoint(progress, ProblemDomain::Maps)?;
    let maps = crate::presentation_commands::map_index_with_content(project, content);
    add(ProblemDomain::Maps, maps.diagnostics.clone());
    checkpoint(progress, ProblemDomain::GraphViews)?;
    let graphs = crate::graph_views::build_graph_view_index(project, content);
    add(ProblemDomain::GraphViews, graphs.diagnostics.clone());
    checkpoint(progress, ProblemDomain::Presets)?;
    add(
        ProblemDomain::Presets,
        crate::presentation_presets::build_preset_index(project, content, &maps, &graphs)
            .diagnostics,
    );
    checkpoint(progress, ProblemDomain::Comments)?;
    add(
        ProblemDomain::Comments,
        crate::collaboration::build_comment_index(project, content, &maps).diagnostics,
    );
    checkpoint(progress, ProblemDomain::Proposals)?;
    add(
        ProblemDomain::Proposals,
        crate::collaboration::build_proposal_index(project).diagnostics,
    );
    checkpoint(progress, ProblemDomain::Templates)?;
    add(
        ProblemDomain::Templates,
        project.template_index_with_content(content).diagnostics,
    );
    checkpoint(progress, ProblemDomain::SavedQueries)?;
    add(
        ProblemDomain::SavedQueries,
        project.saved_query_index().diagnostics,
    );
    checkpoint(progress, ProblemDomain::Manuscripts)?;
    for index in project
        .manuscript_indices_with_content(content)
        .into_values()
    {
        add(ProblemDomain::Manuscripts, index.diagnostics);
    }
    checkpoint(progress, ProblemDomain::ReaderProfiles)?;
    for (id, path) in &registry.reader_profiles {
        if let Err(error) = project.read_reader_profile_document(id, path) {
            add(
                ProblemDomain::ReaderProfiles,
                vec![Diagnostic::error(
                    "READER001",
                    &path.to_string_lossy(),
                    crate::Span::new(1, 1, 1),
                    error,
                )],
            );
        }
    }
    checkpoint(progress, ProblemDomain::Localizations)?;
    for (id, path) in &registry.localizations {
        if let Err(error) = project.read_localization_document(id, path) {
            add(
                ProblemDomain::Localizations,
                vec![Diagnostic::error(
                    "LOC001",
                    &path.to_string_lossy(),
                    crate::Span::new(1, 1, 1),
                    error,
                )],
            );
        }
    }
    // Retain conflict information without changing Project or claiming a disk snapshot.
    match project.conflict_snapshots() {
        Ok(conflicts) if conflicts.is_empty() && project.recovery_conflicts().is_empty() => {}
        Ok(_) => report.reasons.push("source_conflict".into()),
        Err(_) => report
            .reasons
            .push("external_observation_unavailable".into()),
    }
    if report.coverage.iter().any(|v| {
        matches!(
            v.state,
            ProblemCoverageState::Partial | ProblemCoverageState::Unavailable
        )
    }) {
        report.reasons.push("coverage_incomplete".into());
    }
    if project.problems_observation_key().ok().as_deref()
        != Some(report.source_observation.as_str())
        || !super::observation::assets_current(project, content)
    {
        report.reasons.push("external_observation_changed".into());
    }
    report.complete = report.reasons.is_empty();
    assemble(project, diagnostics, report)
}

fn assemble(
    project: &Project,
    diagnostics: Vec<(ProblemDomain, Diagnostic)>,
    mut report: ProblemsReport,
) -> Result<ProblemsReport, ProblemsError> {
    // Sort full raw facts before truncating; clipping never merges distinct issues.
    let mut rows: Vec<_> = diagnostics
        .into_iter()
        .map(|(domain, diagnostic)| {
            let path = super::location::relative(project, std::path::Path::new(&diagnostic.file));
            let facts = serde_json::to_string(&(domain, &diagnostic)).expect("诊断可序列化");
            (
                (
                    diagnostic.severity,
                    path.is_none(),
                    path,
                    diagnostic.span.line,
                    diagnostic.span.column,
                    diagnostic.code,
                    domain,
                    diagnostic.message.clone(),
                    diagnostic.note.clone(),
                    diagnostic.suggestion.clone(),
                    facts,
                ),
                domain,
                diagnostic,
            )
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.dedup_by(|a, b| a.0 == b.0);
    let mut used_bytes = serde_json::to_vec(&report).expect("报告可序列化").len()
        + 4096
        + report.coverage.len().saturating_mul(96);
    let mut related_used = 0;
    for (_, domain, diagnostic) in rows {
        if report.entries.len() >= report.limits.max_entries {
            truncate(&mut report, "entry_limit");
            break;
        }
        let id = format!("0000000000000000:p{}", report.entries.len() + 1);
        let location = |file: &str, span, role| {
            super::location::project_location(
                project,
                file,
                span,
                role,
                report.limits.max_excerpt_bytes,
            )
        };
        let primary = location(&diagnostic.file, diagnostic.span, diagnostic.source_role());
        let mut related = Vec::new();
        let available = report
            .limits
            .max_related_locations
            .saturating_sub(related_used);
        for (index, (file, span)) in diagnostic.related.iter().take(available).enumerate() {
            related.push(location(file, *span, diagnostic.related_source_role(index)));
        }
        let related_truncated = related.len() < diagnostic.related.len();
        let (message, mut text_truncated) =
            clipped(&diagnostic.message, report.limits.max_text_bytes);
        let mut optional = |value: Option<String>| {
            value.map(|text| {
                let (text, clipped) = clipped(&text, report.limits.max_text_bytes);
                text_truncated |= clipped;
                text
            })
        };
        let note = optional(diagnostic.note);
        let suggestion = optional(diagnostic.suggestion);
        let entry = ProblemEntry {
            id: id.clone(),
            domain,
            severity: diagnostic.severity,
            code: diagnostic.code.into(),
            message,
            note,
            suggestion,
            primary,
            related_count: diagnostic.related.len(),
            text_truncated,
        };
        let cost = serde_json::to_vec(&entry).expect("问题可序列化").len()
            + serde_json::to_vec(&related).expect("位置可序列化").len()
            + id.len()
            + 16;
        if used_bytes.saturating_add(cost) > report.limits.max_report_bytes {
            truncate(&mut report, "report_bytes");
            break;
        }
        used_bytes += cost;
        related_used += related.len();
        if related_truncated {
            truncate(&mut report, "related_limit");
        }
        if text_truncated {
            truncate(&mut report, "text_limit");
        }
        if !related.is_empty() {
            report.related.insert(id, related);
        }
        report.entries.push(entry);
    }
    if report.truncated {
        for coverage in &mut report.coverage {
            if coverage.state == ProblemCoverageState::Checked {
                coverage.state = ProblemCoverageState::Partial;
                coverage.reasons.push("report_truncated".into());
            }
        }
    }
    report.reasons.sort();
    report.reasons.dedup();
    report.report_version = super::version::of(&report);
    for entry in &mut report.entries {
        let old = entry.id.clone();
        entry.id.replace_range(..16, &report.report_version);
        if let Some(related) = report.related.remove(&old) {
            report.related.insert(entry.id.clone(), related);
        }
    }
    if serde_json::to_vec(&report).expect("报告可序列化").len() > report.limits.max_report_bytes
    {
        return Err(ProblemsError::new(
            "BUDGET_EXCEEDED",
            "报告元数据超过字节预算，请缩小工作区或提高预算",
        ));
    }
    Ok(report)
}
fn truncate(report: &mut ProblemsReport, reason: &str) {
    report.truncated = true;
    report.complete = false;
    if !report.reasons.iter().any(|r| r == reason) {
        report.reasons.push(reason.into());
    }
}

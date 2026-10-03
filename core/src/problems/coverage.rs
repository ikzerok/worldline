use super::*;
use crate::{project::Project, workspace_documents::Registry, CompileResult};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) fn paths(registry: &Registry, domain: ProblemDomain) -> &BTreeMap<String, PathBuf> {
    match domain {
        ProblemDomain::Maps => &registry.maps,
        ProblemDomain::GraphViews => &registry.graph_views,
        ProblemDomain::Presets => &registry.presets,
        ProblemDomain::Comments => &registry.comments,
        ProblemDomain::Proposals => &registry.proposals,
        ProblemDomain::Templates => &registry.templates,
        ProblemDomain::SavedQueries => &registry.saved_queries,
        ProblemDomain::Manuscripts => &registry.manuscripts,
        ProblemDomain::ReaderProfiles => &registry.reader_profiles,
        ProblemDomain::Localizations => &registry.localizations,
        _ => unreachable!("only registered domains have paths"),
    }
}
fn coverage(
    domain: ProblemDomain,
    path: Option<String>,
    state: ProblemCoverageState,
    reasons: Vec<String>,
) -> ProblemCoverage {
    ProblemCoverage {
        domain,
        path,
        state,
        reasons,
    }
}

pub(crate) fn build(
    project: &Project,
    content: &CompileResult,
    registry: &Registry,
) -> Vec<ProblemCoverage> {
    use ProblemCoverageState::*;
    let mut out = vec![coverage(ProblemDomain::Content, None, Checked, Vec::new())];
    out.extend(content.sources.keys().map(|path| {
        coverage(
            ProblemDomain::Content,
            super::location::relative(project, path),
            Checked,
            Vec::new(),
        )
    }));
    let manifest = crate::workspace_documents::manifest_path(&project.root);
    let has_manifest = project
        .authoring_documents
        .get(&manifest)
        .is_some_and(|d| !d.is_deleted());
    let registry_bad =
        !registry.diagnostics.is_empty() || !project.authoring_diagnostics().is_empty();
    let workspace = if registry_bad {
        Partial
    } else if has_manifest {
        Checked
    } else {
        NotApplicable
    };
    let reasons = if registry_bad {
        vec!["registry_incomplete".into()]
    } else {
        Vec::new()
    };
    out.push(coverage(
        ProblemDomain::Workspace,
        None,
        workspace,
        reasons.clone(),
    ));
    if has_manifest {
        out.push(coverage(
            ProblemDomain::Workspace,
            super::location::relative(project, &manifest),
            workspace,
            reasons,
        ));
    }
    for domain in ProblemDomain::ALL.into_iter().skip(2) {
        let registrations = paths(registry, domain);
        let dependent = matches!(
            domain,
            ProblemDomain::Maps
                | ProblemDomain::GraphViews
                | ProblemDomain::Presets
                | ProblemDomain::Comments
                | ProblemDomain::Templates
                | ProblemDomain::Manuscripts
        );
        let mut reasons = Vec::<String>::new();
        if registry_bad {
            reasons.push("registry_incomplete".into());
        }
        if dependent && content.has_errors() && !registrations.is_empty() {
            reasons.push("content_incomplete".into());
        }
        let first = out.len();
        for path in registrations.values() {
            let mut doc_reasons = reasons.clone();
            let mut state = if doc_reasons.is_empty() {
                Checked
            } else {
                Partial
            };
            match project
                .authoring_documents
                .get(path)
                .filter(|d| !d.is_deleted())
            {
                None => {
                    state = Unavailable;
                    doc_reasons.push("source_unavailable".into());
                }
                Some(document) => {
                    if std::str::from_utf8(document.bytes()).is_err() {
                        state = Unavailable;
                        doc_reasons.push("invalid_utf8".into());
                    } else if crate::parse_unique_json(document.bytes()).is_err() {
                        state = Partial;
                        doc_reasons.push("invalid_document".into());
                    } else if document.is_read_only() {
                        state = Partial;
                        doc_reasons.push("unsupported_document_or_registry".into());
                    }
                }
            }
            out.push(coverage(
                domain,
                super::location::relative(project, path),
                state,
                doc_reasons,
            ));
        }
        let state = if !reasons.is_empty()
            || out[first..]
                .iter()
                .any(|v| matches!(v.state, Partial | Unavailable))
        {
            Partial
        } else if registrations.is_empty() {
            NotApplicable
        } else {
            Checked
        };
        if out[first..]
            .iter()
            .any(|v| matches!(v.state, Partial | Unavailable))
        {
            reasons.push("documents_incomplete".into());
        }
        if matches!(
            domain,
            ProblemDomain::ReaderProfiles | ProblemDomain::Localizations
        ) {
            reasons.push("operation_checks_excluded".into());
        }
        out.insert(first, coverage(domain, None, state, reasons));
    }
    out
}

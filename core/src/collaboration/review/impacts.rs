use super::super::{
    absolute_path, ProposalDraft, ProposalReferenceImpact, ProposalReferenceLocation,
};
use super::MAX_REVIEW_DIFFERENCES;
use crate::project::Project;
use crate::{CompileResult, TargetRef};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

type LimitedReferenceLocations = (Vec<ProposalReferenceLocation>, bool);

fn index_reference_locations(
    result: &CompileResult,
    targets: &BTreeSet<TargetRef>,
) -> BTreeMap<TargetRef, LimitedReferenceLocations> {
    let mut index = targets
        .iter()
        .cloned()
        .map(|target| (target, (Vec::new(), true)))
        .collect::<BTreeMap<_, _>>();
    for reference in &result.analysis.catalog.references {
        let Some((locations, complete)) = index.get_mut(&reference.target) else {
            continue;
        };
        if locations.len() == MAX_REVIEW_DIFFERENCES {
            *complete = false;
        } else {
            locations.push(ProposalReferenceLocation {
                source: reference.source.clone(),
                kind: reference.kind.clone(),
                file: reference.file.clone(),
                line: reference.line,
            });
        }
    }
    index
}

fn changed_file_targets(
    result: Option<&CompileResult>,
    path: &Path,
) -> (BTreeSet<TargetRef>, bool) {
    let mut targets = BTreeSet::new();
    let Some(result) = result else {
        return (targets, false);
    };
    let mut complete = true;
    for object in &result.analysis.catalog.objects {
        let object_file = Path::new(&object.file);
        if object_file != path {
            continue;
        }
        if targets.contains(&object.target) {
            continue;
        }
        if targets.len() == MAX_REVIEW_DIFFERENCES {
            complete = false;
        } else {
            targets.insert(object.target.clone());
        }
    }
    (targets, complete)
}

pub(in crate::collaboration) fn proposal_reference_impacts(
    project: &Project,
    proposal: &ProposalDraft,
) -> BTreeMap<String, (Vec<ProposalReferenceImpact>, bool)> {
    let content_changes = proposal
        .changes
        .iter()
        .filter(|change| change.domain == "content")
        .collect::<Vec<_>>();
    if content_changes.is_empty() {
        return BTreeMap::new();
    }

    // Compile one complete candidate for the whole proposal. Per-file candidates
    // can hide cross-file errors and omit reference edits made by sibling files.
    let current = project.compile_current();
    let mut candidate = project.clone();
    let mut candidate_edit_complete = true;
    for change in &content_changes {
        let Ok(path) = absolute_path(project, &change.path) else {
            candidate_edit_complete = false;
            break;
        };
        let edit = match &change.proposed {
            Some(text) => candidate.set_text(&path, text.clone()),
            None => candidate.delete_document(&path),
        };
        if edit.is_err() {
            candidate_edit_complete = false;
            break;
        }
    }
    let proposed = candidate_edit_complete.then(|| candidate.compile_current());
    let compile_complete = candidate_edit_complete
        && !current.has_errors()
        && proposed.as_ref().is_some_and(|result| !result.has_errors());

    let mut targets_by_file = BTreeMap::new();
    let mut all_targets = BTreeSet::new();
    for change in &content_changes {
        let Ok(path) = absolute_path(project, &change.path) else {
            targets_by_file.insert(change.path.clone(), (BTreeSet::new(), false));
            continue;
        };
        let (mut targets, targets_complete) = changed_file_targets(Some(&current), &path);
        let (proposed_targets, proposed_targets_complete) =
            changed_file_targets(proposed.as_ref(), &path);
        let mut complete = compile_complete && targets_complete && proposed_targets_complete;
        for target in proposed_targets {
            if targets.contains(&target) {
                continue;
            }
            if targets.len() == MAX_REVIEW_DIFFERENCES {
                complete = false;
            } else {
                targets.insert(target);
            }
        }
        all_targets.extend(targets.iter().cloned());
        targets_by_file.insert(change.path.clone(), (targets, complete));
    }

    // Visit each catalog once and retain at most one sentinel beyond the public
    // limit, so a high-fanout target cannot allocate an unbounded DTO buffer.
    let current_references = index_reference_locations(&current, &all_targets);
    let proposed_references = proposed
        .as_ref()
        .map(|result| index_reference_locations(result, &all_targets))
        .unwrap_or_default();

    content_changes
        .into_iter()
        .map(|change| {
            let (targets, mut complete) = targets_by_file.remove(&change.path).unwrap_or_default();
            let mut impacts = Vec::with_capacity(targets.len());
            for target in targets {
                let (mut current, current_complete) =
                    current_references.get(&target).cloned().unwrap_or_default();
                let (mut proposed, proposed_complete) = proposed_references
                    .get(&target)
                    .cloned()
                    .unwrap_or_default();
                complete &= current_complete && proposed_complete;
                current.truncate(MAX_REVIEW_DIFFERENCES);
                proposed.truncate(MAX_REVIEW_DIFFERENCES);
                impacts.push(ProposalReferenceImpact {
                    target,
                    current,
                    proposed,
                });
            }
            (change.path.clone(), (impacts, complete))
        })
        .collect()
}

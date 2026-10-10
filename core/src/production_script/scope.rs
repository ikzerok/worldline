use super::*;
use crate::manuscript::{ManuscriptDeliveryRequest, ManuscriptDeliverySnapshot};
use std::collections::BTreeSet;

pub(super) fn roots(
    query: &Arc<ManuscriptQuerySnapshot>,
    request: &ProductionScriptRequest,
) -> Result<(BTreeSet<TargetRef>, Vec<ProductionChapterOccurrence>), ProductionError> {
    let mut roots = BTreeSet::new();
    let mut occurrences = Vec::new();
    match &request.scope {
        ProductionScope::CurrentTarget { target } => {
            roots.insert(target.clone());
        }
        ProductionScope::Project => {
            roots.extend(
                query
                    .compiled()
                    .program
                    .events
                    .iter()
                    .map(|event| TargetRef::new("event", &event.name)),
            );
            roots.extend(
                query
                    .compiled()
                    .program
                    .fragments
                    .iter()
                    .map(|fragment| TargetRef::new("fragment", &fragment.name)),
            );
        }
        ProductionScope::Manuscript {
            query: input,
            chapter_ids,
            expected_query_key,
        } => {
            let mut delivery = ManuscriptDeliveryRequest::new(input.as_ref().clone());
            delivery.chapter_ids = chapter_ids.clone();
            delivery.expected_snapshot_key = expected_query_key.clone();
            delivery.limits.chapters = request.limits.chapters;
            let selected = ManuscriptDeliverySnapshot::new(query.clone(), &delivery)
                .map_err(|e| ProductionError::new(&e.code, e.message))?;
            if !selected.scope().complete {
                return Err(ProductionError::new(
                    "INCOMPLETE_SCOPE",
                    "所选书稿范围不能确认完整",
                ));
            }
            for row in &selected.scope().chapters {
                let target = row
                    .entry
                    .target_ref
                    .clone()
                    .ok_or_else(ProductionError::source)?;
                roots.insert(target.clone());
                occurrences.push(ProductionChapterOccurrence {
                    manuscript_id: input.manuscript_id.clone(),
                    chapter_id: row.entry.id.clone(),
                    target,
                });
            }
        }
    }
    if roots.len() > request.limits.definitions || occurrences.len() > request.limits.chapters {
        return Err(ProductionError::budget());
    }
    for target in &roots {
        if !matches!(
            target.kind.as_str(),
            "event" | "scene" | "fragment" | "entity"
        ) || query.compiled().analysis.catalog.object(target).is_none()
        {
            return Err(ProductionError::new(
                "INVALID_SCOPE",
                "制作台本来源必须是现存 event、scene、fragment 或 entity",
            ));
        }
    }
    bounded_size(&(&roots, &occurrences), request.limits.result_bytes)?;
    Ok((roots, occurrences))
}

pub(super) fn normalize(request: &ProductionScriptRequest) -> ProductionScriptRequest {
    let mut request = request.clone();
    request.expected_snapshot_key = None;
    if let ProductionScope::Manuscript {
        query,
        expected_query_key,
        chapter_ids,
    } = &mut request.scope
    {
        *expected_query_key = None;
        query.view = crate::manuscript::ManuscriptQueryView::Chapters;
        query.collapsed.clear();
        query.selected_id = None;
        query.offset = 0;
        query.limit = 100;
        query.cursor = None;
        if let Some(ids) = chapter_ids {
            ids.sort();
        }
    }
    request.statuses.sort();
    request
}

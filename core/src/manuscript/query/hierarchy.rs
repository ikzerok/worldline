use super::*;
use crate::catalog::TargetRef;
use crate::manuscript::{ManuscriptEntryKind, ManuscriptReferenceStatus};

pub(super) fn build_book(
    index: &ManuscriptIndex,
    displays: &BTreeMap<TargetRef, String>,
) -> SearchBook {
    let mut identities: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (position, entry) in index.entries.iter().enumerate() {
        identities.entry(&entry.id).or_default().push(position);
    }
    let mut valid = vec![true; index.entries.len()];
    let mut parents = vec![None; index.entries.len()];
    for (position, raw_parent) in index.original_parents.iter().enumerate() {
        valid[position] = index.original_id_counts.get(&index.entries[position].id) == Some(&1)
            && index.original_parent_valid[position];
        let Some(id) = raw_parent else { continue };
        let parent = identities.get(id.as_str()).and_then(|values| {
            (values.len() == 1 && index.original_id_counts.get(id) == Some(&1)).then_some(values[0])
        });
        match parent {
            Some(parent)
                if parent != position
                    && index.entries[parent].kind == ManuscriptEntryKind::Section =>
            {
                parents[position] = Some(parent);
            }
            _ => valid[position] = false,
        }
    }
    // 每个节点最多沿父链访问一次；切开恢复边，原JSON与原parent_id均不改。
    let mut settled = vec![false; parents.len()];
    let mut visiting = vec![usize::MAX; parents.len()];
    for start in 0..parents.len() {
        if settled[start] {
            continue;
        }
        let mut path = Vec::new();
        let mut current = Some(start);
        while let Some(node) = current {
            if settled[node] {
                break;
            }
            if visiting[node] != usize::MAX {
                for &cycle_node in &path[visiting[node]..] {
                    valid[cycle_node] = false;
                }
                if let Some(&last) = path.last() {
                    parents[last] = None;
                }
                break;
            }
            visiting[node] = path.len();
            path.push(node);
            current = parents[node];
        }
        for node in path {
            settled[node] = true;
            visiting[node] = usize::MAX;
        }
    }
    let mut children = vec![Vec::new(); parents.len()];
    let mut roots = Vec::new();
    for (node, parent) in parents.into_iter().enumerate() {
        if let Some(parent) = parent {
            children[parent].push(node);
        } else {
            roots.push(node);
        }
    }
    let mut rows: Vec<SearchRow> = Vec::with_capacity(index.entries.len());
    let mut stack: Vec<_> = roots
        .into_iter()
        .rev()
        .map(|node| (node, None, false))
        .collect();
    while let Some((node, parent, leaving)) = stack.pop() {
        if leaving {
            rows[node].subtree_end = rows.len();
            continue;
        }
        let mut entry = index.entries[node].clone();
        entry.parent_id.clone_from(&index.original_parents[node]);
        let perspective_display = entry
            .perspective
            .as_ref()
            .filter(|_| entry.perspective_status == Some(ManuscriptReferenceStatus::Resolved))
            .and_then(|target| displays.get(target))
            .cloned();
        let text = format!(
            "{}\n{}\n{}\n{}",
            entry.id,
            entry.title,
            entry.summary.as_deref().unwrap_or_default(),
            entry.goal.as_deref().unwrap_or_default()
        )
        .to_lowercase();
        let status = entry.status.as_deref().unwrap_or_default().to_lowercase();
        let pov = entry
            .perspective
            .as_ref()
            .map(|target| {
                format!(
                    "{}:{}\n{}",
                    target.kind,
                    target.id,
                    perspective_display.as_deref().unwrap_or_default()
                )
                .to_lowercase()
            })
            .unwrap_or_default();
        let ordinal = rows.len();
        let identity_ambiguous = index.original_id_counts.get(&entry.id) != Some(&1);
        let path_complete =
            valid[node] && parent.is_none_or(|parent: usize| rows[parent].row.path_complete);
        rows.push(SearchRow {
            row: ManuscriptQueryRow {
                entry,
                section_path: Vec::new(),
                perspective_display,
                ordinal,
                identity_ambiguous,
                context_only: false,
                path_complete,
            },
            parent,
            subtree_end: 0,
            text,
            status,
            pov,
        });
        stack.push((ordinal, None, true));
        for &child in children[node].iter().rev() {
            stack.push((child, Some(ordinal), false));
        }
    }
    let mut identities: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (position, row) in rows.iter().enumerate() {
        identities
            .entry(row.row.entry.id.clone())
            .or_default()
            .push(position);
    }
    SearchBook { rows, identities }
}

impl SearchBook {
    pub(super) fn materialize(&self, position: usize, context_only: bool) -> ManuscriptQueryRow {
        let mut row = self.rows[position].row.clone();
        row.context_only = context_only;
        let mut current = self.rows[position].parent;
        while let Some(parent) = current {
            let entry = &self.rows[parent].row.entry;
            row.section_path.push(ManuscriptSectionPath {
                id: entry.id.clone(),
                title: entry.title.clone(),
            });
            current = self.rows[parent].parent;
        }
        row.section_path.reverse();
        row
    }
}

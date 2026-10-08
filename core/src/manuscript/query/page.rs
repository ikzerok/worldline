use super::*;
use crate::manuscript::ManuscriptEntryKind;

impl ManuscriptQuerySnapshot {
    pub fn query(
        &self,
        request: &ManuscriptQueryRequest,
    ) -> Result<ManuscriptQueryPage, ManuscriptQueryError> {
        request.validate()?;
        let book = self.books.get(&request.manuscript_id).ok_or_else(|| {
            ManuscriptQueryError::new("MANUSCRIPT_NOT_FOUND", "书稿未在当前缓冲清单中注册")
        })?;
        let index = &self.indices[&request.manuscript_id];
        let (start, end) = if let Some(id) = &request.section_id {
            let positions = book
                .identities
                .get(id)
                .ok_or_else(|| ManuscriptQueryError::new("INVALID_SECTION", "查询分节不存在"))?;
            if positions.len() != 1
                || self.entry_is_ambiguous(&request.manuscript_id, id)
                || book.rows[positions[0]].row.entry.kind != ManuscriptEntryKind::Section
            {
                return Err(ManuscriptQueryError::new(
                    "INVALID_SECTION",
                    "查询范围必须是身份明确的分节",
                ));
            }
            let start = positions[0];
            (start, book.rows[start].subtree_end)
        } else {
            (0, book.rows.len())
        };
        let query_key = query_key(&self.key, request);
        let requested_offset = match request.cursor.as_deref() {
            Some(cursor) => decode_cursor(cursor, &query_key)?,
            None => request.offset,
        };
        let words: Vec<_> = request
            .text
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let status = request.status.trim().to_lowercase();
        let pov = request.pov.trim().to_lowercase();
        let filtered = !words.is_empty() || !status.is_empty() || !pov.is_empty();
        let mut recognized_chapters = 0;
        let mut matching_chapters = 0;
        let mut matches = vec![false; book.rows.len()];
        for (position, row) in book.rows.iter().enumerate().take(end).skip(start) {
            if row.row.entry.kind != ManuscriptEntryKind::Chapter {
                continue;
            }
            recognized_chapters += 1;
            if words.iter().all(|word| row.text.contains(word))
                && row.status.contains(&status)
                && row.pov.contains(&pov)
            {
                matches[position] = true;
                matching_chapters += 1;
            }
        }
        let mut included = matches.clone();
        if request.view == ManuscriptQueryView::Tree {
            if filtered {
                // 逆前序向父项传播一次；深树不对每个命中重走全部父链。
                for position in (start..end).rev() {
                    if included[position] {
                        if let Some(parent) =
                            book.rows[position].parent.filter(|parent| *parent >= start)
                        {
                            included[parent] = true;
                        }
                    }
                }
            } else {
                included[start..end].fill(true);
            }
        }
        let collapsed: BTreeSet<&str> = request.collapsed.iter().map(String::as_str).collect();
        let mut hidden = vec![false; book.rows.len()];
        let mut visible = Vec::new();
        for position in start..end {
            if !filtered && request.view == ManuscriptQueryView::Tree {
                hidden[position] = book.rows[position]
                    .parent
                    .filter(|parent| *parent >= start)
                    .is_some_and(|parent| {
                        hidden[parent]
                            || (!book.rows[parent].row.identity_ambiguous
                                && collapsed.contains(book.rows[parent].row.entry.id.as_str()))
                    });
            }
            if included[position] && !hidden[position] {
                visible.push(position);
            }
        }
        let selection_ambiguous = request
            .selected_id
            .as_ref()
            .is_some_and(|id| self.entry_is_ambiguous(&request.manuscript_id, id));
        let selected = request
            .selected_id
            .as_ref()
            .and_then(|id| book.identities.get(id))
            .filter(|positions| positions.len() == 1 && !selection_ambiguous)
            .map(|positions| positions[0]);
        let selection_matches = request.selected_id.as_ref().map(|_| {
            selected.is_some_and(|position| {
                position >= start
                    && position < end
                    && if book.rows[position].row.entry.kind == ManuscriptEntryKind::Chapter {
                        matches[position]
                    } else {
                        included[position]
                    }
            })
        });
        let selected_offset =
            selected.and_then(|position| visible.iter().position(|row| *row == position));
        let limit = request.limit;
        let total_rows = visible.len();
        let last_page = total_rows.saturating_sub(1) / limit * limit;
        let offset = if requested_offset >= total_rows {
            last_page
        } else {
            requested_offset
        };
        let page_end = offset.saturating_add(limit).min(total_rows);
        let rows = visible[offset..page_end]
            .iter()
            .map(|&position| book.materialize(position, filtered && !matches[position]))
            .collect();
        let mut diagnostics = self.diagnostics.clone();
        diagnostics.extend(index.diagnostics.iter().cloned());
        crate::diagnostic::sort_diagnostics(&mut diagnostics);
        Ok(ManuscriptQueryPage {
            schema_version: MANUSCRIPT_QUERY_SCHEMA_VERSION,
            manuscript_id: request.manuscript_id.clone(),
            snapshot_key: self.key.clone(),
            source: if self.writing_draft || self.draft_ids.contains(&request.manuscript_id) {
                ManuscriptQuerySource::Draft
            } else {
                ManuscriptQuerySource::Applied
            },
            complete: self.complete
                && !index.read_only
                && index
                    .diagnostics
                    .iter()
                    .all(|item| item.severity != crate::Severity::Error)
                && book.rows.iter().all(|row| row.row.path_complete),
            diagnostics,
            recognized_chapters,
            matching_chapters,
            total_rows,
            offset,
            limit,
            next_cursor: (page_end < total_rows).then(|| encode_cursor(&query_key, page_end)),
            rows,
            selection_matches,
            selection_ambiguous,
            selected_offset,
        })
    }
}

fn query_key(snapshot: &str, request: &ManuscriptQueryRequest) -> String {
    let mut query = request.clone();
    query.offset = 0;
    query.cursor = None;
    let mut hash = key::Key::new("worldline-manuscript-cursor-v1");
    hash.bytes(snapshot.as_bytes());
    hash.bytes(&serde_json::to_vec(&query).expect("书稿查询可序列化"));
    hash.finish()
}
fn encode_cursor(query: &str, offset: usize) -> String {
    let mut hash = key::Key::new("worldline-manuscript-cursor-offset-v1");
    hash.bytes(query.as_bytes());
    hash.bytes(offset.to_string().as_bytes());
    format!("mq1:{query}:{offset}:{}", hash.finish())
}
fn decode_cursor(cursor: &str, query: &str) -> Result<usize, ManuscriptQueryError> {
    let parts: Vec<_> = cursor.split(':').collect();
    let offset = parts.get(2).and_then(|offset| offset.parse::<usize>().ok());
    if let Some(offset) = offset {
        if parts.len() == 4 && cursor == encode_cursor(query, offset) {
            return Ok(offset);
        }
    }
    Err(ManuscriptQueryError::new(
        "STALE_CURSOR",
        "书稿游标已过期或被修改，请重新查询",
    ))
}

use super::super::{
    pointer_child, ProposalDifference, ProposalFileChange, ProposalRawSources, ProposalSourceRange,
};
use super::MAX_REVIEW_DIFFERENCES;
use crate::workspace_documents::parse_unique_json;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

const MAX_REVIEW_TEXT_BYTES: usize = 16 * 1024;

fn review_text(value: &str, truncated: &mut bool) -> String {
    if value.len() <= MAX_REVIEW_TEXT_BYTES {
        return value.to_owned();
    }
    *truncated = true;
    let mut end = MAX_REVIEW_TEXT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn review_value(value: Option<&Value>, truncated: &mut bool) -> Option<String> {
    let value = value?;
    let mut writer = ReviewValueWriter::default();
    if serde_json::to_writer(&mut writer, value).is_err() {
        *truncated = true;
    }
    let valid_len = std::str::from_utf8(&writer.bytes)
        .map_or_else(|error| error.valid_up_to(), |_| writer.bytes.len());
    Some(String::from_utf8(writer.bytes[..valid_len].to_vec()).unwrap())
}

#[derive(Default)]
struct ReviewValueWriter {
    bytes: Vec<u8>,
}

impl Write for ReviewValueWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = MAX_REVIEW_TEXT_BYTES - self.bytes.len();
        let written = bytes.len().min(remaining);
        self.bytes.extend_from_slice(&bytes[..written]);
        if written < bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "proposal review text limit reached",
            ));
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn paragraphs_with_ranges(source: &str) -> Vec<(&str, ProposalSourceRange)> {
    if source.is_empty() {
        return vec![(
            "",
            ProposalSourceRange {
                start_byte: 0,
                end_byte: 0,
            },
        )];
    }

    let bytes = source.as_bytes();
    let mut paragraphs = Vec::new();
    let mut paragraph_start = 0;
    let mut line_start = 0;
    let mut has_text = false;
    for (newline, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        let content_end = if newline > line_start && bytes[newline - 1] == b'\r' {
            newline - 1
        } else {
            newline
        };
        let blank_line = source[line_start..content_end].trim().is_empty();
        let line_end = newline + 1;
        if blank_line {
            if has_text {
                paragraphs.push((
                    &source[paragraph_start..line_end],
                    ProposalSourceRange {
                        start_byte: paragraph_start,
                        end_byte: line_end,
                    },
                ));
                paragraph_start = line_end;
                has_text = false;
            } else if let Some((text, range)) = paragraphs.last_mut() {
                range.end_byte = line_end;
                *text = &source[range.start_byte..line_end];
                paragraph_start = line_end;
            } else {
                paragraph_start = line_end;
            }
        } else {
            has_text = true;
        }
        line_start = line_end;
    }

    if line_start < bytes.len() {
        has_text |= !source[line_start..].trim().is_empty();
    }
    if paragraph_start < bytes.len() {
        if has_text || paragraphs.is_empty() {
            paragraphs.push((
                &source[paragraph_start..],
                ProposalSourceRange {
                    start_byte: paragraph_start,
                    end_byte: bytes.len(),
                },
            ));
        } else if let Some((text, range)) = paragraphs.last_mut() {
            range.end_byte = bytes.len();
            *text = &source[range.start_byte..];
        }
    }
    if paragraphs.is_empty() {
        paragraphs.push((
            source,
            ProposalSourceRange {
                start_byte: 0,
                end_byte: bytes.len(),
            },
        ));
    }
    paragraphs
}

#[derive(Clone, Debug)]
struct ParagraphAlignment {
    /// 对应每个 base 段落的 current/proposed 段落索引；None 表示删除。
    matched: Vec<Option<usize>>,
    /// 以 base 段落边界为键的插入段落范围。
    inserted: Vec<Option<std::ops::Range<usize>>>,
}

fn paragraph_identity_alignment(count: usize) -> ParagraphAlignment {
    ParagraphAlignment {
        matched: (0..count).map(Some).collect(),
        inserted: vec![None; count + 1],
    }
}

fn paragraph_unique_positions<'a>(
    paragraphs: &[(&'a str, ProposalSourceRange)],
) -> BTreeMap<&'a str, Option<usize>> {
    let mut positions = BTreeMap::new();
    for (index, (text, _)) in paragraphs.iter().enumerate() {
        positions
            .entry(*text)
            .and_modify(|position| *position = None)
            .or_insert(Some(index));
    }
    positions
}

fn paragraph_gap_is_unambiguous(
    base: &[(&str, ProposalSourceRange)],
    changed: &[(&str, ProposalSourceRange)],
) -> bool {
    let mut seen = BTreeSet::new();
    base.iter()
        .chain(changed)
        .all(|(text, _)| seen.insert(*text))
}

fn align_paragraphs(
    base: &[(&str, ProposalSourceRange)],
    changed: &[(&str, ProposalSourceRange)],
) -> Option<ParagraphAlignment> {
    if base.len() == changed.len()
        && base
            .iter()
            .zip(changed)
            .all(|((left, _), (right, _))| left == right)
    {
        return Some(paragraph_identity_alignment(base.len()));
    }

    let base_positions = paragraph_unique_positions(base);
    let changed_positions = paragraph_unique_positions(changed);
    let mut anchors = Vec::new();
    let mut last_changed = None;
    for (base_index, (text, _)) in base.iter().enumerate() {
        let (Some(Some(unique_base)), Some(Some(unique_changed))) =
            (base_positions.get(text), changed_positions.get(text))
        else {
            continue;
        };
        if *unique_base != base_index
            || last_changed.is_some_and(|previous| *unique_changed <= previous)
        {
            return None;
        }
        anchors.push((base_index, *unique_changed));
        last_changed = Some(*unique_changed);
    }

    let mut alignment = ParagraphAlignment {
        matched: vec![None; base.len()],
        inserted: vec![None; base.len() + 1],
    };
    let mut base_start = 0;
    let mut changed_start = 0;
    for (base_end, changed_end) in anchors
        .into_iter()
        .chain(std::iter::once((base.len(), changed.len())))
    {
        let base_gap = &base[base_start..base_end];
        let changed_gap = &changed[changed_start..changed_end];
        match (base_gap.len(), changed_gap.len()) {
            (0, 0) => {}
            (0, _) => {
                alignment.inserted[base_start] = Some(changed_start..changed_end);
            }
            (_, 0) => {}
            (base_count, changed_count)
                if base_count == changed_count
                    && paragraph_gap_is_unambiguous(base_gap, changed_gap) =>
            {
                for offset in 0..base_count {
                    alignment.matched[base_start + offset] = Some(changed_start + offset);
                }
            }
            _ => return None,
        }

        if base_end < base.len() {
            alignment.matched[base_end] = Some(changed_end);
            base_start = base_end + 1;
            changed_start = changed_end + 1;
        }
    }
    Some(alignment)
}

fn paragraph_span<'a>(
    source: &'a str,
    paragraphs: &[(&str, ProposalSourceRange)],
    span: std::ops::Range<usize>,
) -> Option<(&'a str, ProposalSourceRange)> {
    if span.is_empty() {
        return None;
    }
    let start_byte = paragraphs.get(span.start)?.1.start_byte;
    let end_byte = paragraphs.get(span.end - 1)?.1.end_byte;
    Some((
        &source[start_byte..end_byte],
        ProposalSourceRange {
            start_byte,
            end_byte,
        },
    ))
}

fn push_review_difference(
    result: &mut Vec<ProposalDifference>,
    truncated: &mut bool,
    path: String,
    base: Option<String>,
    current: Option<String>,
    proposed: Option<String>,
    ranges: [Option<ProposalSourceRange>; 3],
) {
    if result.len() == MAX_REVIEW_DIFFERENCES {
        *truncated = true;
    } else if result.len() < MAX_REVIEW_DIFFERENCES {
        let [base_range, current_range, proposed_range] = ranges;
        result.push(ProposalDifference {
            path,
            base,
            current,
            proposed,
            base_range,
            current_range,
            proposed_range,
        });
    }
}

fn json_review_differences(
    path: &str,
    base: Option<&Value>,
    current: Option<&Value>,
    proposed: Option<&Value>,
    result: &mut Vec<ProposalDifference>,
    truncated: &mut bool,
    ranges: &[Option<ProposalSourceRange>; 3],
) {
    if base == current && current == proposed {
        return;
    }
    if result.len() >= MAX_REVIEW_DIFFERENCES {
        *truncated = true;
        return;
    }
    if let (
        Some(Value::Object(base)),
        Some(Value::Object(current)),
        Some(Value::Object(proposed)),
    ) = (base, current, proposed)
    {
        let mut base_keys = base.keys().peekable();
        let mut current_keys = current.keys().peekable();
        let mut proposed_keys = proposed.keys().peekable();
        while let Some(key) = [base_keys.peek(), current_keys.peek(), proposed_keys.peek()]
            .into_iter()
            .flatten()
            .min()
            .copied()
        {
            if base_keys.peek().copied() == Some(key) {
                base_keys.next();
            }
            if current_keys.peek().copied() == Some(key) {
                current_keys.next();
            }
            if proposed_keys.peek().copied() == Some(key) {
                proposed_keys.next();
            }
            let base_value = base.get(key);
            let current_value = current.get(key);
            let proposed_value = proposed.get(key);
            if base_value == current_value && current_value == proposed_value {
                continue;
            }
            json_review_differences(
                &pointer_child(path, key),
                base_value,
                current_value,
                proposed_value,
                result,
                truncated,
                ranges,
            );
            if result.len() == MAX_REVIEW_DIFFERENCES && *truncated {
                return;
            }
        }
    } else {
        let base = review_value(base, truncated);
        let current = review_value(current, truncated);
        let proposed = review_value(proposed, truncated);
        push_review_difference(
            result,
            truncated,
            path.to_owned(),
            base,
            current,
            proposed,
            ranges.clone(),
        );
    }
}

pub(in crate::collaboration) fn review_differences(
    change: &ProposalFileChange,
    current: Option<&str>,
) -> (Vec<ProposalDifference>, bool, bool, ProposalRawSources) {
    let mut result = Vec::new();
    let mut truncated = false;
    let raw = ProposalRawSources {
        base: change
            .base
            .as_deref()
            .map(|text| review_text(text, &mut truncated)),
        current: current.map(|text| review_text(text, &mut truncated)),
        proposed: change
            .proposed
            .as_deref()
            .map(|text| review_text(text, &mut truncated)),
    };
    let full_ranges = [change.base.as_deref(), current, change.proposed.as_deref()].map(|source| {
        source.map(|source| ProposalSourceRange {
            start_byte: 0,
            end_byte: source.len(),
        })
    });
    if change.domain == "presentation" {
        let parsed = [change.base.as_deref(), current, change.proposed.as_deref()]
            .map(|source| source.and_then(|source| parse_unique_json(source.as_bytes()).ok()));
        if let [Some(base), Some(current), Some(proposed)] = &parsed {
            json_review_differences(
                "",
                Some(base),
                Some(current),
                Some(proposed),
                &mut result,
                &mut truncated,
                &full_ranges,
            );
            return (result, truncated, false, raw);
        }
    } else if change.domain == "content" {
        if let (Some(base_text), Some(current_text), Some(proposed_text)) =
            (change.base.as_deref(), current, change.proposed.as_deref())
        {
            let base_paragraphs = paragraphs_with_ranges(base_text);
            let current_paragraphs = paragraphs_with_ranges(current_text);
            let proposed_paragraphs = paragraphs_with_ranges(proposed_text);
            let current_alignment = if base_text == current_text {
                Some(paragraph_identity_alignment(base_paragraphs.len()))
            } else {
                align_paragraphs(&base_paragraphs, &current_paragraphs)
            };
            let proposed_alignment = if base_text == proposed_text {
                Some(paragraph_identity_alignment(base_paragraphs.len()))
            } else {
                align_paragraphs(&base_paragraphs, &proposed_paragraphs)
            };
            if let (Some(current_alignment), Some(proposed_alignment)) =
                (current_alignment, proposed_alignment)
            {
                for boundary in 0..=base_paragraphs.len() {
                    let current_insertion = current_alignment.inserted[boundary]
                        .clone()
                        .and_then(|span| paragraph_span(current_text, &current_paragraphs, span));
                    let proposed_insertion = proposed_alignment.inserted[boundary]
                        .clone()
                        .and_then(|span| paragraph_span(proposed_text, &proposed_paragraphs, span));
                    let insertion_values = [None, current_insertion, proposed_insertion];
                    let insertion_texts = insertion_values
                        .each_ref()
                        .map(|item| item.as_ref().map(|(text, _)| *text));
                    if insertion_texts[0] != insertion_texts[1]
                        || insertion_texts[1] != insertion_texts[2]
                    {
                        let insertion_ranges = insertion_values
                            .each_ref()
                            .map(|item| item.as_ref().map(|(_, range)| range.clone()));
                        let insertion_texts = insertion_values
                            .map(|item| item.map(|(text, _)| review_text(text, &mut truncated)));
                        push_review_difference(
                            &mut result,
                            &mut truncated,
                            format!("/paragraphs/{}", boundary + 1),
                            insertion_texts[0].clone(),
                            insertion_texts[1].clone(),
                            insertion_texts[2].clone(),
                            insertion_ranges,
                        );
                    }

                    if boundary == base_paragraphs.len() {
                        continue;
                    }
                    let base_item = &base_paragraphs[boundary];
                    let current_item = current_alignment.matched[boundary]
                        .and_then(|index| current_paragraphs.get(index));
                    let proposed_item = proposed_alignment.matched[boundary]
                        .and_then(|index| proposed_paragraphs.get(index));
                    let paragraph_items = [Some(base_item), current_item, proposed_item];
                    let paragraph_texts = paragraph_items
                        .each_ref()
                        .map(|item| item.map(|(text, _)| *text));
                    if paragraph_texts[0] == paragraph_texts[1]
                        && paragraph_texts[1] == paragraph_texts[2]
                    {
                        continue;
                    }
                    let paragraph_ranges = paragraph_items
                        .each_ref()
                        .map(|item| item.map(|(_, range)| range.clone()));
                    let paragraph_texts = paragraph_items
                        .map(|item| item.map(|(text, _)| review_text(text, &mut truncated)));
                    push_review_difference(
                        &mut result,
                        &mut truncated,
                        format!("/paragraphs/{}", boundary + 1),
                        paragraph_texts[0].clone(),
                        paragraph_texts[1].clone(),
                        paragraph_texts[2].clone(),
                        paragraph_ranges,
                    );
                }
                return (result, truncated, false, raw);
            }
        }
        let has_difference =
            change.base.as_deref() != current || current != change.proposed.as_deref();
        if has_difference {
            push_review_difference(
                &mut result,
                &mut truncated,
                "".into(),
                raw.base.clone(),
                raw.current.clone(),
                raw.proposed.clone(),
                full_ranges,
            );
        }
        return (result, truncated, true, raw);
    }
    if change.base.as_deref() != current || current != change.proposed.as_deref() {
        push_review_difference(
            &mut result,
            &mut truncated,
            "".into(),
            raw.base.clone(),
            raw.current.clone(),
            raw.proposed.clone(),
            full_ranges,
        );
    }
    (result, truncated, false, raw)
}

pub(crate) fn review_checkpoint_text(
    base: Option<&str>,
    current: Option<&str>,
    checkpoint: Option<&str>,
) -> (Vec<ProposalDifference>, bool, bool, ProposalRawSources) {
    let change = ProposalFileChange {
        path: String::new(),
        domain: "content".into(),
        base: base.map(str::to_owned),
        proposed: checkpoint.map(str::to_owned),
    };
    review_differences(&change, current)
}

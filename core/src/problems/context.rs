//! 同稿原文窗口；显示边界按 grapheme，所有公开坐标仍按 UTF-8/scalar。
use super::*;
use unicode_segmentation::UnicodeSegmentation;

impl ProblemSourceContext {
    pub(crate) fn empty(role: ProblemSourceRole) -> Self {
        Self {
            version: 1,
            role,
            text: None,
            slice_byte_range: None,
            slice_char_range: None,
            hit_byte_range: None,
            hit_char_range: None,
            visibility: ProblemContextVisibility::NoText,
            prefix_clipped: false,
            suffix_clipped: false,
        }
    }
}

pub(crate) fn project(
    source: &str,
    byte_base: usize,
    char_base: usize,
    hit: Option<ProblemRange>,
    role: ProblemSourceRole,
    limit: usize,
) -> ProblemSourceContext {
    let mut context = ProblemSourceContext::empty(role);
    let anchor = hit.as_ref().map_or(0, |range| range.start);
    context.prefix_clipped = anchor > 0;
    context.suffix_clipped = anchor < source.len();
    if limit == 0 {
        return context;
    }
    let boundaries: Vec<_> = source
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .chain(std::iter::once(source.len()))
        .collect();
    let Some((start, end)) = window(&boundaries, hit.as_ref(), limit) else {
        return context;
    };
    let slice = &source[start..end];
    let start_char = source[..start].chars().count();
    let end_char = start_char + slice.chars().count();
    context.slice_byte_range = Some(ProblemRange {
        start: byte_base + start,
        end: byte_base + end,
    });
    context.slice_char_range = Some(ProblemRange {
        start: char_base + start_char,
        end: char_base + end_char,
    });
    let complete = if let Some(hit) = hit {
        let local_start = hit.start.max(start) - start;
        let local_end = hit.end.min(end) - start;
        context.hit_char_range = Some(ProblemRange {
            start: slice[..local_start].chars().count(),
            end: slice[..local_end].chars().count(),
        });
        context.hit_byte_range = Some(ProblemRange {
            start: local_start,
            end: local_end,
        });
        start <= hit.start && end >= hit.end
    } else {
        start == 0 && end == source.len()
    };
    context.text = Some(slice.to_owned());
    context.visibility = if complete {
        ProblemContextVisibility::Full
    } else {
        ProblemContextVisibility::Partial
    };
    context.prefix_clipped = start > 0;
    context.suffix_clipped = end < source.len();
    context
}

fn floor(boundaries: &[usize], point: usize) -> usize {
    boundaries[boundaries.partition_point(|boundary| *boundary <= point) - 1]
}
fn ceil(boundaries: &[usize], point: usize) -> usize {
    boundaries[boundaries.partition_point(|boundary| *boundary < point)]
}
fn window(
    boundaries: &[usize],
    hit: Option<&ProblemRange>,
    limit: usize,
) -> Option<(usize, usize)> {
    let length = *boundaries.last()?;
    let Some(hit) = hit else {
        let end = floor(boundaries, length.min(limit));
        return (end > 0 || length == 0).then_some((0, end));
    };
    let first = floor(boundaries, hit.start);
    let last = ceil(boundaries, hit.end);
    if last - first > limit {
        let end = floor(boundaries, first.saturating_add(limit).min(length));
        return (end > hit.start).then_some((first, end));
    }
    // Include the full hit first, balance context, then reuse slack at either edge.
    let spare = limit - (last - first);
    let mut start = ceil(boundaries, first.saturating_sub(spare / 2));
    let end = floor(boundaries, start.saturating_add(limit).min(length));
    start = ceil(boundaries, end.saturating_sub(limit));
    Some((start, end))
}

use super::*;
use crate::{project::Project, Span};
use unicode_segmentation::UnicodeSegmentation;

fn location(text: &str, span: Span, limit: usize) -> ProblemLocation {
    let root = std::env::temp_dir().join(format!("problem-context-{}", std::process::id()));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.set_text(&entry, text.into()).unwrap();
    super::location::project_location(
        &project,
        &entry.to_string_lossy(),
        span,
        Some(ProblemSourceRole::Target),
        limit,
    )
}
fn scalars(text: &str, range: &ProblemRange) -> String {
    text.chars().skip(range.start).take(range.end - range.start).collect()
}
fn assert_consistent(source: &str, location: &ProblemLocation, limit: usize) {
    let context = location.context.as_ref().unwrap();
    assert_eq!(context.version, 1);
    assert_eq!(context.text, location.excerpt);
    let Some(text) = context.text.as_deref() else {
        assert_eq!(context.visibility, ProblemContextVisibility::NoText);
        assert!(context.slice_byte_range.is_none() && context.slice_char_range.is_none());
        assert!(context.hit_byte_range.is_none() && context.hit_char_range.is_none());
        return;
    };
    assert!(text.len() <= limit);
    let bytes = context.slice_byte_range.as_ref().unwrap();
    let chars = context.slice_char_range.as_ref().unwrap();
    assert_eq!(text, &source[bytes.start..bytes.end]);
    assert_eq!(text, scalars(source, chars));
    if let Some(hit) = context.hit_byte_range.as_ref() {
        let chars = context.hit_char_range.as_ref().unwrap();
        assert_eq!(&text[hit.start..hit.end], scalars(text, chars));
        let full = location.byte_range.as_ref().unwrap();
        assert_eq!(bytes.start + hit.start, full.start.max(bytes.start));
        assert_eq!(bytes.start + hit.end, full.end.min(bytes.end));
        assert_eq!(
            context.visibility == ProblemContextVisibility::Full,
            bytes.start <= full.start && bytes.end >= full.end,
        );
    }
}
#[test]
fn tail_hit_is_visible_without_changing_authoritative_ranges() {
    let source = format!("event start\r\n  {}missing\r\n", "长中文段落😀".repeat(500));
    let line = source.split('\n').nth(1).unwrap().trim_end_matches('\r');
    let column = line.chars().count() - "missing".len() + 1;
    let full = location(&source, Span::new(2, column as u32, 7), 512);
    let context = full.context.as_ref().unwrap();
    assert_eq!(context.role, ProblemSourceRole::Target);
    assert_eq!(context.visibility, ProblemContextVisibility::Full);
    assert!(context.prefix_clipped && !context.suffix_clipped);
    assert!(context.text.as_ref().unwrap().ends_with("missing"));
    assert_consistent(&source, &full, 512);
    for limit in [0, 1, 2, 3, 4, 511, 512] {
        let bounded = location(&source, Span::new(2, column as u32, 7), limit);
        assert_eq!(bounded.byte_range, full.byte_range);
        assert_eq!(bounded.char_range, full.char_range);
        assert_eq!(bounded.span, full.span);
        assert_eq!(bounded.precision, ProblemPrecision::Span);
        assert_eq!(bounded.context.as_ref().unwrap().role, ProblemSourceRole::Target);
        assert_consistent(&source, &bounded, limit);
    }
}
#[test]
fn partial_hit_is_a_visible_intersection_never_a_shortened_full_range() {
    let source = format!("前{}后", "表达式".repeat(200));
    let span = Span::new(1, 2, 600);
    let bounded = location(&source, span, 512);
    let context = bounded.context.as_ref().unwrap();
    assert_eq!(context.visibility, ProblemContextVisibility::Partial);
    assert_eq!(bounded.char_range, Some(ProblemRange { start: 1, end: 601 }));
    assert!(context.prefix_clipped && context.suffix_clipped);
    assert_consistent(&source, &bounded, 512);
    for limit in [0, 1, 2, 3, 4, 511, 512] {
        let actual = location(&source, span, limit);
        assert_eq!(actual.char_range, bounded.char_range);
        assert_eq!(actual.byte_range, bounded.byte_range);
        assert_consistent(&source, &actual, limit);
    }
}
#[test]
fn tiny_budgets_do_not_split_scalar_or_extended_grapheme() {
    for hit in ["中", "😀", "e\u{301}", "👩🏽‍💻", "👨‍👩‍👧‍👦", "🇨🇳"] {
        let source = format!("前{hit}后");
        let span = Span::new(1, 2, hit.chars().count() as u32);
        for limit in [0, 1, 2, 3, 4, 511, 512] {
            let actual = location(&source, span, limit);
            assert_consistent(&source, &actual, limit);
            let context = actual.context.as_ref().unwrap();
            assert_eq!(context.role, ProblemSourceRole::Target);
            if limit < hit.len() {
                assert_eq!(context.visibility, ProblemContextVisibility::NoText);
            } else {
                assert_eq!(context.visibility, ProblemContextVisibility::Full);
                let range = context.slice_byte_range.as_ref().unwrap();
                let boundaries: Vec<_> = source.grapheme_indices(true).map(|(i, _)| i)
                    .chain(std::iter::once(source.len())).collect();
                assert!(boundaries.contains(&range.start) && boundaries.contains(&range.end));
            }
        }
    }
}
#[test]
fn scalar_hit_inside_grapheme_does_not_change_full_source_or_local_hit() {
    let source = "前e\u{301}后";
    let actual = location(source, Span::new(1, 3, 1), 3);
    assert_consistent(source, &actual, 3);
    let context = actual.context.unwrap();
    assert_eq!(context.text.as_deref(), Some("e\u{301}"));
    assert_eq!(context.hit_byte_range, Some(ProblemRange { start: 1, end: 3 }));
    assert_eq!(context.hit_char_range, Some(ProblemRange { start: 1, end: 2 }));
}
#[test]
fn repeated_word_and_crlf_empty_eof_use_physical_original_coordinates() {
    let source = "😀 missing\r\n  missing missing\r\n";
    let actual = location(source, Span::new(2, 11, 7), 7);
    assert_consistent(source, &actual, 7);
    assert_eq!(actual.byte_range, Some(ProblemRange { start: 24, end: 31 }));
    assert_eq!(actual.char_range, Some(ProblemRange { start: 21, end: 28 }));
    let eof = location(source, Span::new(3, 1, 0), 4);
    assert_consistent(source, &eof, 4);
    assert_eq!(eof.byte_range, Some(ProblemRange { start: source.len(), end: source.len() }));
    assert_eq!(eof.context.unwrap().visibility, ProblemContextVisibility::Full);
    let invalid = location(source, Span::new(2, 11, 8), 512);
    assert_eq!(invalid.precision, ProblemPrecision::Unavailable);
    assert_eq!(invalid.context.unwrap().role, ProblemSourceRole::Target);
}

#[test]
fn unproven_and_unavailable_sources_never_borrow_a_precise_span() {
    let root = std::env::temp_dir().join(format!("problem-context-roles-{}", std::process::id()));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.set_text(&entry, "正文😀\n".into()).unwrap();
    for limit in [0, 1, 2, 3, 4, 511, 512] {
        let document = super::location::project_location(
            &project, &entry.to_string_lossy(), Span::new(1, 1, 1), None, limit,
        );
        assert_eq!(document.precision, ProblemPrecision::Document);
        assert!(document.span.is_none() && document.byte_range.is_none());
        let context = document.context.as_ref().unwrap();
        assert_eq!(context.role, ProblemSourceRole::Document);
        assert!(context.hit_byte_range.is_none() && context.hit_char_range.is_none());
        assert_consistent("正文😀\n", &document, limit);
        let unavailable = super::location::project_location(
            &project, &entry.to_string_lossy(), Span::new(1, 1, 1),
            Some(ProblemSourceRole::Unavailable), limit,
        );
        assert_eq!(unavailable.precision, ProblemPrecision::Unavailable);
        assert_eq!(unavailable.context.unwrap().visibility, ProblemContextVisibility::NoText);
    }
}

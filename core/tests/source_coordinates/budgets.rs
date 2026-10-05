use super::*;
use worldline_core::source_coordinates::{
    MAX_SOURCE_COORDINATE_BYTES, MAX_SOURCE_COORDINATE_LINES, MAX_SOURCE_JUMP_CONTEXT_CHARACTERS,
    MAX_SOURCE_JUMP_REQUEST_BYTES,
};

#[test]
fn source_byte_budget_is_inclusive_and_counts_utf8_bytes() {
    let at = "🙂".repeat(MAX_SOURCE_COORDINATE_BYTES / 4);
    let coordinates = SourceCoordinates::new(&at).unwrap();
    assert_eq!(coordinates.line_count(), 1);
    assert_eq!(
        coordinates.position_at_byte(&at, at.len()).unwrap().column,
        at.chars().count() + 1
    );
    let over = at + "x";
    assert!(SourceCoordinates::new(&over).unwrap_err().contains("2 MiB"));
    let fixture = Fixture::new("", None);
    assert!(fixture.preview(&over, "1").is_err());
}

#[test]
fn line_budget_counts_empty_source_and_final_empty_line_inclusively() {
    let at = "\n".repeat(MAX_SOURCE_COORDINATE_LINES - 1);
    let coordinates = SourceCoordinates::new(&at).unwrap();
    assert_eq!(coordinates.line_count(), MAX_SOURCE_COORDINATE_LINES);
    assert_eq!(
        coordinates
            .locate(&at, &MAX_SOURCE_COORDINATE_LINES.to_string())
            .unwrap()
            .byte_offset,
        at.len()
    );
    let over = at + "\n";
    assert!(SourceCoordinates::new(&over)
        .unwrap_err()
        .contains("65,536"));
    let crlf_at = "\r\n".repeat(MAX_SOURCE_COORDINATE_LINES - 1);
    assert_eq!(
        SourceCoordinates::new(&crlf_at).unwrap().line_count(),
        MAX_SOURCE_COORDINATE_LINES
    );
}

#[test]
fn request_budget_includes_trimmed_whitespace_and_is_inclusive() {
    let coordinates = SourceCoordinates::new("").unwrap();
    let at = format!("{}1", " ".repeat(MAX_SOURCE_JUMP_REQUEST_BYTES - 1));
    assert_eq!(coordinates.locate("", &at).unwrap().column, 1);
    assert!(coordinates
        .locate("", &(at.clone() + " "))
        .unwrap_err()
        .contains("128"));
    let fixture = Fixture::new("", None);
    fixture.ready("", &at);
    assert!(fixture.preview("", &(at + " ")).is_err());
}

#[test]
fn long_line_context_is_bounded_centered_and_keeps_exact_unicode_ranges() {
    let fixture = Fixture::new("", None);
    let source = format!("前行\r\n{}\r\n", "中🙂e\u{301}\t".repeat(100));
    for (column, left, right) in [(1, false, true), (251, true, true), (501, true, false)] {
        let preview = fixture.ready(&source, &format!("2:{column}"));
        assert_eq!(
            preview.context.text.chars().count(),
            MAX_SOURCE_JUMP_CONTEXT_CHARACTERS
        );
        assert_eq!(preview.context.truncated_start, left);
        assert_eq!(preview.context.truncated_end, right);
        assert_eq!(preview.max_column, 501);
        assert_eq!(preview.line_count, 3);
        assert!(preview.context.start_column <= column);
        assert!(column <= preview.context.start_column + preview.context.text.chars().count());
        assert!(preview.context.byte_range.start <= preview.position.byte_offset);
        assert!(preview.position.byte_offset <= preview.context.byte_range.end);
        let coordinates = SourceCoordinates::new(&source).unwrap();
        let beginning = coordinates
            .position_at_byte(&source, preview.context.byte_range.start)
            .unwrap();
        assert_eq!(beginning.line, 2);
        assert_eq!(beginning.column, preview.context.start_column);
        assert!(!preview.context.text.contains(['\r', '\n']));
    }
    for length in [
        MAX_SOURCE_JUMP_CONTEXT_CHARACTERS - 1,
        MAX_SOURCE_JUMP_CONTEXT_CHARACTERS,
    ] {
        let source = "🙂".repeat(length);
        let preview = fixture.ready(&source, &format!("1:{}", length + 1));
        assert_eq!(preview.context.text, source);
        assert!(!preview.context.truncated_start && !preview.context.truncated_end);
    }
}

#[test]
fn largest_single_line_keeps_only_bounded_public_context_and_no_private_stamp() {
    let fixture = Fixture::new("", None);
    let source = "x".repeat(MAX_SOURCE_COORDINATE_BYTES);
    let preview = fixture.ready(&source, &format!("1:{}", source.len() + 1));
    assert_eq!(
        preview.context.text.len(),
        MAX_SOURCE_JUMP_CONTEXT_CHARACTERS
    );
    let serialized = serde_json::to_value(preview).unwrap();
    assert!(serialized.get("stamp").is_none());
    assert!(serialized.get("source").is_none());
    assert_eq!(serialized.as_object().unwrap().len(), 5);
    assert!(serde_json::to_vec(&serialized).unwrap().len() < 2048);
}

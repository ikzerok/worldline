use super::*;
use worldline_core::source_outline::*;

#[test]
fn byte_line_and_entry_limits_are_inclusive_and_overflow_is_not_partial() {
    let fixture = Fixture::new("", None);
    let at_line = format!("//{}", "x".repeat(MAX_SOURCE_OUTLINE_LINE_BYTES - 2));
    assert_eq!(fixture.outline(&at_line).status, Status::Ready);
    assert_eq!(
        fixture.outline(&(at_line.clone() + "x")).status,
        Status::BudgetExceeded
    );
    let at_bytes = format!("//{}\n", "x".repeat(8192 - 3)).repeat(64);
    assert_eq!(at_bytes.len(), MAX_SOURCE_OUTLINE_BYTES);
    assert_ne!(fixture.outline(&at_bytes).status, Status::BudgetExceeded);
    assert_eq!(
        fixture.outline(&(at_bytes + "\n")).status,
        Status::BudgetExceeded
    );
    let lines = "\n".repeat(MAX_SOURCE_OUTLINE_LINES);
    assert_eq!(fixture.outline(&lines).status, Status::Ready);
    assert_eq!(
        fixture.outline(&(lines + "\n")).status,
        Status::BudgetExceeded
    );
    let entries = "character same\n".repeat(MAX_SOURCE_OUTLINE_ENTRIES);
    let result = fixture.outline(&entries);
    assert_eq!(result.status, Status::Ready, "{:?}", result.message);
    assert_eq!(result.entries.len(), MAX_SOURCE_OUTLINE_ENTRIES);
    let result = fixture.outline(&(entries + "character extra\n"));
    assert_eq!(result.status, Status::BudgetExceeded);
    assert!(result.entries.is_empty());
}

#[test]
fn nesting_expression_and_exact_projection_output_are_bounded() {
    let fixture = Fixture::new("", None);
    let nested = |levels: usize, name_len: usize| {
        let mut source = "event e\n".to_string();
        for depth in 1..=levels {
            source += &format!("{}scene {}\n", " ".repeat(depth * 2), "x".repeat(name_len));
        }
        source += &format!("{}正文\n", " ".repeat((levels + 1) * 2));
        source
    };
    assert_eq!(fixture.outline(&nested(62, 1)).status, Status::Ready);
    assert_eq!(
        fixture.outline(&nested(63, 1)).status,
        Status::BudgetExceeded
    );
    let output_large = fixture.outline(&nested(60, 2000));
    assert_eq!(output_large.status, Status::BudgetExceeded);
    assert!(output_large.message.unwrap().contains("2 MiB"));
    assert!(output_large.entries.is_empty());
    for expression in [
        format!("{}1{}", "(".repeat(65), ")".repeat(65)),
        "not ".repeat(65) + "true",
        "-".repeat(257) + "1",
    ] {
        let source = format!("let value = {expression}\n");
        assert_eq!(fixture.outline(&source).status, Status::BudgetExceeded);
    }
}

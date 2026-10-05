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
    assert_eq!(fixture.outline(&at_bytes).status, Status::Ready);
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

#[test]
fn literal_parentheses_comments_and_escaped_quotes_do_not_consume_expression_budget() {
    let fixture = Fixture::new("", Some("1.13"));
    let literal = format!("\"{} {}", "(".repeat(1000), "not ".repeat(100));
    let quoted = serde_json::to_string(&literal).unwrap();
    let source = format!(
        "// {}\ncharacter c as {quoted}\n  property text = {quoted}\nevent e\n  {}\n  -> END\n",
        "(".repeat(1000),
        "(".repeat(1000)
    );
    let outline = fixture.ready(&source);
    assert_eq!(outline.entries[0].display, literal);
    assert_eq!(outline.entries.len(), 2);
}

#[test]
fn actual_expression_depth_combines_unary_and_parentheses_and_token_cap_is_inclusive() {
    let fixture = Fixture::new("", None);
    for (at, over) in [
        ("not ".repeat(64) + "true", "not ".repeat(65) + "true"),
        ("-".repeat(64) + "1", "-".repeat(65) + "1"),
        (
            format!("{}1{}", "(".repeat(64), ")".repeat(64)),
            format!("{}1{}", "(".repeat(65), ")".repeat(65)),
        ),
        (
            format!(
                "{}{}true{}",
                "not ".repeat(32),
                "(".repeat(32),
                ")".repeat(32)
            ),
            format!(
                "{}{}true{}",
                "not ".repeat(32),
                "(".repeat(33),
                ")".repeat(33)
            ),
        ),
        (
            format!("-{}1", "1+".repeat(127)),
            format!("{}1", "1+".repeat(128)),
        ),
    ] {
        assert_eq!(
            fixture.outline(&format!("let value = {at}\n")).status,
            Status::Ready,
            "{at}"
        );
        let outline = fixture.outline(&format!("let value = {over}\n"));
        assert_eq!(
            outline.status,
            Status::BudgetExceeded,
            "{over}: {:?}",
            outline.message
        );
        assert!(outline.entries.is_empty());
    }
}

#[test]
fn quoted_say_and_choice_and_unquoted_text_interpolations_share_formal_expression_budget() {
    let fixture = Fixture::new("", Some("1.13"));
    let literal = format!("\"{}", "(".repeat(1000));
    let expression = serde_json::to_string(&literal).unwrap();
    let raw = format!("{{{expression}}}");
    let quoted = serde_json::to_string(&raw).unwrap();
    let source = format!(
        "character c\nevent e\n  say c {quoted}\n  choice {quoted}\n    {raw}\n    -> END\n"
    );
    fixture.ready(&source);
    for deep in [
        "not ".repeat(65) + "true",
        "-".repeat(65) + "1",
        format!("{}1{}", "(".repeat(65), ")".repeat(65)),
    ] {
        let raw = format!("{{{deep}}}");
        let quoted = serde_json::to_string(&raw).unwrap();
        for body in [
            format!("say c {quoted}"),
            format!("choice {quoted}\n    -> END"),
            raw,
        ] {
            let source = format!("character c\nevent e\n  {body}\n");
            let outline = fixture.outline(&source);
            assert_eq!(
                outline.status,
                Status::BudgetExceeded,
                "{source}: {:?}",
                outline.message
            );
            assert!(outline.entries.is_empty());
        }
    }
}

//! 与手册第5节一致的最小教学例；不改词法器或旧空格语义。
use worldline_core::{compile_source_with_options, CompileOptions};
use worldline_runtime::{Output, Story};

const EXAMPLE: &str = r#"event keywords
  \choice 只是正文
  \if 只是正文
  \-> 只是正文
  -> END
"#;

#[test]
fn documented_keyword_escape_runs_identically_in_all_supported_languages() {
    for options in [
        CompileOptions::v1_9(),
        CompileOptions::v1_10(),
        CompileOptions::v1_11(),
        CompileOptions::v1_12(),
        CompileOptions::v1_13(),
    ] {
        let result = compile_source_with_options("keywords.wl", EXAMPLE, options);
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert_eq!(result.analysis.stats.choices, 0);
        let mut story = Story::new(&result.program, &result.analysis).unwrap();
        let text: Vec<_> = story
            .continue_story()
            .unwrap()
            .into_iter()
            .filter_map(|o| match o {
                Output::Text { content, .. } => Some(content),
                _ => None,
            })
            .collect();
        assert_eq!(text, ["choice 只是正文", "if 只是正文", "-> 只是正文"]);
        assert!(story.is_ended());
        assert!(story.choices().is_empty());
    }
}

#[test]
fn extra_indentation_does_not_escape_keyword_prose() {
    for spaces in [2, 3, 4, 8] {
        let source = format!(
            "event keywords\n{}choice 只是正文\n  -> END\n",
            " ".repeat(spaces)
        );
        for options in [CompileOptions::v1_9(), CompileOptions::v1_13()] {
            let result = compile_source_with_options("spaces.wl", &source, options);
            assert!(
                result.diagnostics.iter().any(|d| d.code == "P004"),
                "空格不应成为转义：{source:?}, {:?}",
                result.diagnostics
            );
        }
    }
}

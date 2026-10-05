//! 当前原文坐标与纯文本跳转的独立回归入口。
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{
    project::Project,
    source_coordinates::{SourceCoordinates, SourceJumpPreview, SourcePosition},
};

#[path = "source_coordinates/budgets.rs"]
mod budgets;
#[path = "source_coordinates/guards.rs"]
mod guards;

struct Fixture {
    root: PathBuf,
    project: Project,
}

impl Fixture {
    fn new(source: &str, version: Option<&str>) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "source-coordinates-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("world.wl"), source).unwrap();
        if let Some(version) = version {
            fs::create_dir_all(root.join(".world")).unwrap();
            fs::write(
                root.join(".world/project.json"),
                format!(r#"{{"schema_version":1,"language_version":"{version}","required_features":[]}}"#),
            )
            .unwrap();
        }
        let project = Project::open(&root).unwrap();
        Self { root, project }
    }

    fn preview(&self, source: &str, request: &str) -> Result<SourceJumpPreview, String> {
        self.project
            .preview_source_jump(&self.project.entry, source, request)
    }

    fn ready(&self, source: &str, request: &str) -> SourceJumpPreview {
        let preview = self.preview(source, request).unwrap();
        let range = self.project.resolve_source_jump(&preview, source).unwrap();
        assert_eq!(
            range,
            preview.position.byte_offset..preview.position.byte_offset
        );
        assert!(source.is_char_boundary(range.start));
        assert_eq!(
            &source[preview.context.byte_range.clone()],
            preview.context.text
        );
        preview
    }

    fn manifest(&mut self, text: &str) {
        fs::create_dir_all(self.root.join(".world")).unwrap();
        fs::write(self.root.join(".world/project.json"), text).unwrap();
        self.project.refresh().unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn ascii_unicode_combining_marks_tabs_and_crlf_use_scalar_columns() {
    let source = "A中🙂e\u{301}\tZ\r\n次\n";
    let coordinates = SourceCoordinates::new(source).unwrap();
    assert_eq!(coordinates.line_count(), 3);
    let cases = [
        (1, 1, 0, 0),
        (1, 2, 1, 1),
        (1, 3, 4, 2),
        (1, 4, 8, 3),
        (1, 5, 9, 4),
        (1, 6, 11, 5),
        (1, 7, 12, 6),
        (1, 8, 13, 7),
        (2, 1, 15, 9),
        (2, 2, 18, 10),
        (3, 1, 19, 11),
    ];
    for (line, column, byte_offset, character_offset) in cases {
        let expected = SourcePosition {
            line,
            column,
            byte_offset,
            character_offset,
        };
        assert_eq!(
            coordinates
                .locate(source, &format!("{line}:{column}"))
                .unwrap(),
            expected
        );
        assert_eq!(
            coordinates
                .position_at_character(source, character_offset)
                .unwrap(),
            expected
        );
        assert_eq!(
            coordinates.position_at_byte(source, byte_offset).unwrap(),
            expected
        );
    }
    assert!(coordinates.position_at_character(source, 8).is_err());
    assert!(coordinates.position_at_byte(source, 14).is_err());
    for byte in [2, 3, 5, 6, 7, 10, 16, 17] {
        assert!(coordinates.position_at_byte(source, byte).is_err());
    }
}

#[test]
fn empty_final_empty_and_consecutive_lines_have_eol_insertion_points() {
    for (source, count, cases) in [
        ("", 1, vec![(1, 0)]),
        ("\n", 2, vec![(1, 0), (2, 1)]),
        ("\r\n", 2, vec![(1, 0), (2, 2)]),
        ("\n\r\n\n", 4, vec![(1, 0), (2, 1), (3, 3), (4, 4)]),
    ] {
        let coordinates = SourceCoordinates::new(source).unwrap();
        assert_eq!(coordinates.line_count(), count);
        for (line, byte) in cases {
            let position = coordinates.locate(source, &format!("{line}:1")).unwrap();
            assert_eq!(position.byte_offset, byte);
            assert_eq!(position.column, 1);
            assert!(coordinates.locate(source, &format!("{line}:2")).is_err());
        }
        assert!(coordinates
            .locate(source, &format!("{}:1", count + 1))
            .is_err());
    }
    let source = "abc";
    let coordinates = SourceCoordinates::new(source).unwrap();
    assert_eq!(coordinates.locate(source, "1:4").unwrap().byte_offset, 3);
    assert!(coordinates.locate(source, "1:5").is_err());
}

#[test]
fn standalone_cr_and_unicode_separators_are_ordinary_scalars() {
    let source = "a\rb\u{2028}c\u{2029}";
    let coordinates = SourceCoordinates::new(source).unwrap();
    assert_eq!(coordinates.line_count(), 1);
    assert_eq!(
        coordinates.locate(source, "1:7").unwrap().byte_offset,
        source.len()
    );
    assert_eq!(coordinates.locate(source, "1:3").unwrap().byte_offset, 2);
}

#[test]
fn all_small_text_boundaries_round_trip_without_clamping() {
    for source in ["\n\r\n🙂\r中\n\u{301}\t\0", "\r\r\n", "中🙂e\u{301}\t"] {
        let coordinates = SourceCoordinates::new(source).unwrap();
        for byte in 0..=source.len() {
            let inside_crlf = source.as_bytes().get(byte) == Some(&b'\n')
                && byte > 0
                && source.as_bytes()[byte - 1] == b'\r';
            let result = coordinates.position_at_byte(source, byte);
            if !source.is_char_boundary(byte) || inside_crlf {
                assert!(result.is_err(), "{source:?} byte {byte}");
                continue;
            }
            let position = result.unwrap();
            assert_eq!(position.character_offset, source[..byte].chars().count());
            assert_eq!(
                coordinates
                    .position_at_character(source, position.character_offset)
                    .unwrap(),
                position
            );
            assert_eq!(
                coordinates
                    .locate(source, &format!("{}:{}", position.line, position.column))
                    .unwrap(),
                position
            );
        }
        for index in [source.chars().count() + 1, usize::MAX] {
            assert!(coordinates.position_at_character(source, index).is_err());
        }
        assert!(coordinates.position_at_byte(source, usize::MAX).is_err());
    }
}

#[test]
fn requests_trim_only_the_outside_and_reject_invalid_or_out_of_range_numbers() {
    let source = "ab\n中🙂";
    let coordinates = SourceCoordinates::new(source).unwrap();
    for request in ["2", "2:1", "  2:1\t", "\u{3000}02:001\u{a0}"] {
        let position = coordinates.locate(source, request).unwrap();
        assert_eq!((position.line, position.column), (2, 1));
    }
    for request in [
        "", " ", "0", "0:1", "1:0", "-1", "+1", "1:-1", "1:+1", ":1", "1:", "1:1:1", "1 :1",
        "1: 1", "1:\t1", "1\n:1", "１:1", "1:١", "1.0", "1e2", "3", "2:4", "1\0", "1：1",
    ] {
        assert!(coordinates.locate(source, request).is_err(), "{request:?}");
    }
    for request in [format!("{}0", usize::MAX), format!("1:{}0", usize::MAX)] {
        assert!(coordinates
            .locate(source, &request)
            .unwrap_err()
            .contains("溢出"));
    }
}

#[test]
fn coordinate_cache_requires_exact_content_including_equal_length_edits() {
    let coordinates = SourceCoordinates::new("a\nb").unwrap();
    for stale in ["x\ny", "a\r\nb", "a\nb\n", ""] {
        assert!(coordinates.locate(stale, "1").is_err());
        assert!(coordinates.position_at_byte(stale, 0).is_err());
        assert!(coordinates.position_at_character(stale, 0).is_err());
    }
    let cloned = coordinates.clone();
    assert_eq!(cloned.locate("a\nb", "2").unwrap().byte_offset, 2);
}

#[test]
fn syntax_errors_and_every_supported_language_version_remain_plain_text_navigable() {
    for version in [
        None,
        Some("1.9"),
        Some("1.10"),
        Some("1.11"),
        Some("1.12"),
        Some("1.13"),
    ] {
        let mut fixture = Fixture::new("event", version);
        assert!(fixture.project.compile().has_errors());
        for source in [
            "event",
            "/*未闭合🙂",
            "event e\n\t错误缩进",
            "entity old kind place",
            "\0",
        ] {
            let preview = fixture.ready(source, "1:1");
            assert_eq!(preview.position.byte_offset, 0);
            assert_eq!(preview.context.text, source.split('\n').next().unwrap());
        }
        assert_eq!(fixture.project.language_version(), version.unwrap_or("1.9"));
    }
}

#[test]
fn short_line_context_is_exact_and_empty_context_is_not_missing() {
    let fixture = Fixture::new("", None);
    let source = "🙂中\t e\u{301}\r\n\n";
    let preview = fixture.ready(source, "1:7");
    assert_eq!(preview.max_column, 7);
    assert_eq!(preview.line_count, 3);
    assert_eq!(preview.context.text, "🙂中\t e\u{301}");
    assert_eq!(preview.context.byte_range, 0..source.find('\r').unwrap());
    assert_eq!(preview.context.start_column, 1);
    assert!(!preview.context.truncated_start);
    assert!(!preview.context.truncated_end);
    for request in ["2", "3"] {
        let preview = fixture.ready(source, request);
        assert_eq!(preview.max_column, 1);
        assert!(preview.context.text.is_empty());
        assert_eq!(
            preview.context.byte_range,
            preview.position.byte_offset..preview.position.byte_offset
        );
    }
}

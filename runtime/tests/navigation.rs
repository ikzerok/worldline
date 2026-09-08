use worldline_core::ast::PropertyValue;
use worldline_core::authoring::CharacterDraft;
use worldline_core::catalog::TargetRef;
use worldline_core::navigation::{link_source, reading_lines};
use worldline_core::project::Project;
use worldline_core::{compile_source, fingerprint_program, CompileResult};
use worldline_runtime::{Output, Story};

const SOURCE: &str = r#"character lin as "林舟"
character mei as "梅"
tag harbor as "雾港"
tag calm as "平静"
tag alert as "警觉"
state mood on character lin with calm
alias character lin as "阿舟"
alias character mei as "阿舟"
alias character lin as "阿舟"
event start with lin
  你好，[[character:lin|阿舟]]在[[tag:harbor|雾港]]。
  choice "问[[character:lin|阿舟]]"
    become mood with alert
    scene inside
      看见[[character:lin|守望者]]。
      -> END
  choice "离开"
    become mood with calm
    -> END
"#;

fn valid(result: &CompileResult) {
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
}

fn project() -> Project {
    let root = std::env::temp_dir().join(format!("worldline-navigation-{}", std::process::id()));
    let mut project = Project::new(&root);
    project.documents.retain(|path, _| path == &project.entry);
    project
        .set_text(&project.entry.clone(), SOURCE.into())
        .unwrap();
    project
}

#[test]
fn links_are_indexed_without_changing_presence_and_aliases_are_ambiguous() {
    let result = compile_source("test.wl", SOURCE);
    valid(&result);
    let catalog = &result.analysis.catalog;
    assert_eq!(catalog.search_objects("阿舟").len(), 2);
    assert_eq!(
        catalog.aliases_for(&TargetRef::new("character", "lin")),
        ["阿舟"]
    );
    assert_eq!(catalog.text_links.len(), 4);
    assert!(catalog
        .text_links
        .iter()
        .any(|link| link.source == TargetRef::new("scene", "start.inside")));
    let first = &catalog.text_links[0];
    assert_eq!((first.line, first.column), (11, 6));
    assert_eq!(
        catalog
            .references_to(&TargetRef::new("character", "lin"))
            .iter()
            .filter(|r| r.kind == "正文链接")
            .count(),
        3
    );
    assert!(result.analysis.symbols.characters["mei"].events.is_empty());
    assert_eq!(catalog.states["mood"].changes.len(), 2);
    assert_ne!(
        catalog.states["mood"].changes[0].contexts,
        catalog.states["mood"].changes[1].contexts
    );
}

#[test]
fn playback_and_save_compatibility_depend_only_on_visible_link_text() {
    let plain = SOURCE
        .replace("[[character:lin|阿舟]]", "阿舟")
        .replace("[[tag:harbor|雾港]]", "雾港")
        .replace("[[character:lin|守望者]]", "守望者");
    let old = compile_source("test.wl", &plain);
    let linked = compile_source("test.wl", SOURCE);
    valid(&old);
    valid(&linked);
    assert_eq!(
        fingerprint_program(&old.program),
        fingerprint_program(&linked.program)
    );
    let mut story = Story::new(&old.program, &old.analysis).unwrap();
    story.continue_story().unwrap();
    let save = story.save().unwrap();
    let mut restored = Story::load(&linked.program, &linked.analysis, &save).unwrap();
    restored.continue_story().unwrap();
    assert_eq!(restored.choices()[0].label, "问阿舟");
    restored.choose(0).unwrap();
    let output = restored.continue_story().unwrap();
    assert!(output.iter().any(|o| matches!(o, Output::Text { content, .. } if content.contains("守望者") && !content.contains("[["))));
    let different = compile_source("test.wl", &SOURCE.replace("|阿舟]]", "|小舟]]"));
    assert_ne!(
        fingerprint_program(&linked.program),
        fingerprint_program(&different.program)
    );
    let retarget = compile_source(
        "test.wl",
        &SOURCE.replace("[[character:lin|", "[[character:mei|"),
    );
    valid(&retarget);
    assert_eq!(
        fingerprint_program(&linked.program),
        fingerprint_program(&retarget.program)
    );
}

#[test]
fn invalid_links_fail_and_escaped_or_property_mentions_are_not_links() {
    for text in [
        "[[character:missing|未知]]",
        "alias character missing as \"未知\"",
    ] {
        let source = if text.starts_with("alias") {
            format!("{text}\nevent start\n  -> END\n")
        } else {
            format!("event start\n  {text}\n  -> END\n")
        };
        assert!(compile_source("bad.wl", &source)
            .diagnostics
            .iter()
            .any(|d| d.code == "A218"));
    }
    for text in [
        "[[character:lin]]",
        "[[character:lin|]]",
        "[[character:lin|阿舟",
        "[[wrong:lin|阿舟]]",
        "[[character:lin|{x}]]",
    ] {
        let result = compile_source(
            "bad.wl",
            &format!("character lin\nevent start\n  {text}\n  -> END\n"),
        );
        assert!(
            result.diagnostics.iter().any(|d| d.code == "P004"),
            "{text}: {:?}",
            result.diagnostics
        );
    }
    let escaped = compile_source(
        "literal.wl",
        r#"character lin
  property note = "[[character:missing|原样资料]]"
event start
  \[\[character:missing|普通文本]]
  choice "\\[\\[character:missing|普通选择]]"
    -> END
"#,
    );
    valid(&escaped);
    assert!(escaped.analysis.catalog.text_links.is_empty());
    let mut story = Story::new(&escaped.program, &escaped.analysis).unwrap();
    story.continue_story().unwrap();
    assert_eq!(story.choices()[0].label, "[[character:missing|普通选择]]");
}

#[test]
fn cross_file_alias_edits_and_character_rename_preserve_long_text_and_comments() {
    let mut project = project();
    let extra = project.add_file(std::path::Path::new("notes.wl")).unwrap();
    project.set_text(&extra, "alias character lin as \"守望者\" // 保留别名注释\nevent extra\n  [[character:lin|老朋友]]说：lin。 // [[character:lin|注释]]\n  -> END\n".into()).unwrap();
    project
        .edit(|p| {
            p.set_aliases(
                &TargetRef::new("character", "lin"),
                &["阿舟".into(), "守望者".into()],
            )
        })
        .unwrap();
    let draft = CharacterDraft {
        id: "linzhou".into(),
        display: "林舟".into(),
        properties: vec![(
            "voice".into(),
            PropertyValue::Str("第一行\n第二行：\"到此为止\"\\背斜线".into()),
        )],
        ..Default::default()
    };
    let entry = project.entry.clone();
    project
        .edit(|p| p.write_character(&entry, Some("lin"), &draft))
        .unwrap();
    let result = project.compile();
    valid(&result);
    assert_eq!(
        result.analysis.symbols.characters["linzhou"].properties["voice"],
        draft.properties[0].1
    );
    assert_eq!(
        result
            .analysis
            .catalog
            .aliases_for(&TargetRef::new("character", "linzhou")),
        ["守望者", "阿舟"]
    );
    let text = project.document(&extra).unwrap();
    assert!(text.contains("// 保留别名注释"));
    assert!(text.contains("[[character:linzhou|老朋友]]说：lin。"));
    assert!(text.contains("// [[character:lin|注释]]"));
    assert!(project
        .document(&entry)
        .unwrap()
        .contains("choice \"问[[character:linzhou|阿舟]]\""));
    let before = project.sources();
    assert!(project
        .edit(|p| p.set_aliases(
            &TargetRef::new("character", "linzhou"),
            &["bad\nname".into()]
        ))
        .is_err());
    assert_eq!(project.sources(), before);
}

#[test]
fn source_reading_and_file_links_use_core_resolution() {
    let mut project = project();
    let extra = project.add_file(std::path::Path::new("notes.wl")).unwrap();
    project.set_text(&extra, "// 作者资料\n".into()).unwrap();
    project
        .edit(|p| {
            p.set_aliases(
                &TargetRef::new("file", &extra.to_string_lossy()),
                &["参考笔记".into()],
            )
        })
        .unwrap();
    let result = project.compile();
    valid(&result);
    assert_eq!(
        result.analysis.catalog.search_objects("参考笔记")[0]
            .target
            .id,
        extra.to_string_lossy()
    );
    let source = link_source(
        &TargetRef::new("file", &extra.to_string_lossy()),
        "笔记",
        &project.entry.to_string_lossy(),
    )
    .unwrap();
    assert_eq!(source, "[[file:notes.wl|笔记]]");
    let parts = reading_lines(
        &format!("  看{source}。 // [[character:missing|注释]]"),
        &project.entry.to_string_lossy(),
    );
    let links: Vec<_> = parts
        .iter()
        .flatten()
        .filter_map(|p| p.target.as_ref())
        .collect();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].id, extra.to_string_lossy());
    let event = result
        .analysis
        .catalog
        .object(&TargetRef::new("event", "start"))
        .unwrap();
    let text = project.object_source(&event.file, event.line).unwrap();
    assert!(text.contains("choice \"问[[character:lin|阿舟]]\""));
    assert!(!text.contains("alias character"));
}

use worldline_core::authoring::WorldDraft;
use worldline_core::catalog::TargetRef;
use worldline_core::project::Project;
use worldline_core::wiki::KeywordIndex;
use worldline_core::{compile_source, CompileResult};

const SOURCE: &str = r#"tag harbor as "雾港"
  description "林舟在雾港生活。"
tag pier as "雾港码头"
  description "雾港的码头。"
character lin as "林舟"
  property background = "阿舟来自雾港。"
character mei as "梅"
alias character lin as "阿舟"
alias character mei as "阿舟"
alias character lin as "LIN"
event start with lin
  林舟走过雾港码头，雾港雾港。阿舟与 Lin、LIN、lin。
  [[character:lin|梅]]与[[tag:harbor|远方]]。 // 雾港与林舟
  choice "去雾港找[[character:lin|梅]]"
    梅等着阿舟。
    -> END
"#;

fn compiled() -> CompileResult {
    let result = compile_source("wiki.wl", SOURCE);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}

#[test]
fn unicode_longest_repeated_ambiguous_and_ascii_boundaries() {
    let result = compiled();
    let index = KeywordIndex::new(&result);
    let text = "🧭雾港码头雾港雾港 阿舟 Lin LIN lin online LIN_1 alinx 林舟";
    let matches = index.find(text);
    let labels: Vec<_> = matches.iter().map(|m| &text[m.start..m.end]).collect();
    assert_eq!(
        labels,
        [
            "雾港码头",
            "雾港",
            "雾港",
            "阿舟",
            "Lin",
            "LIN",
            "lin",
            "林舟"
        ]
    );
    assert_eq!(matches[0].targets, [TargetRef::new("tag", "pier")]);
    assert_eq!(
        matches[3].targets,
        [
            TargetRef::new("character", "lin"),
            TargetRef::new("character", "mei")
        ]
    );
    assert!(index.find("").is_empty());
    assert!(index.find("没有匹配").is_empty());
}

#[test]
fn source_occurrences_preserve_columns_and_explicit_link_targets() {
    let result = compiled();
    let index = KeywordIndex::new(&result);
    let harbor = index.occurrences(&TargetRef::new("tag", "harbor"));
    assert_eq!(
        harbor
            .iter()
            .filter(|hit| hit.line == 12)
            .map(|hit| hit.column)
            .collect::<Vec<_>>(),
        [12, 14]
    );
    // 长词不会额外算作短词；评论中的名字不进入索引。
    assert_eq!(harbor.iter().filter(|hit| hit.line == 13).count(), 1);
    let mei = index.occurrences(&TargetRef::new("character", "mei"));
    assert!(!mei.iter().any(|hit| [13, 14].contains(&hit.line)));
    let lin = index.occurrences(&TargetRef::new("character", "lin"));
    assert_eq!(lin.iter().filter(|hit| hit.line == 13).count(), 1);
    assert!(lin.iter().any(|hit| hit.line == 14));
    for hit in harbor {
        assert_eq!(
            hit.preview,
            SOURCE.lines().nth(hit.line as usize - 1).unwrap()
        );
    }
    let escaped = SOURCE.replace("去雾港找", r#"去\"雾港\"找"#);
    let escaped = compile_source("escaped.wl", &escaped);
    assert!(!escaped.has_errors(), "{:?}", escaped.diagnostics);
    let index = KeywordIndex::new(&escaped);
    let hit = index
        .occurrences(&TargetRef::new("character", "lin"))
        .iter()
        .find(|hit| hit.line == 14)
        .unwrap();
    assert_eq!(hit.preview.chars().nth(hit.column as usize - 1), Some('['));
}

#[test]
fn wiki_authoring_refresh_and_rollback_preserve_source_and_fingerprint() {
    let root = std::env::temp_dir().join(format!("worldline-wiki-{}", std::process::id()));
    let mut project = Project::new(&root);
    project.documents.retain(|path, _| *path == project.entry);
    project
        .set_text(&project.entry.clone(), SOURCE.into())
        .unwrap();
    let before = project.compile().analysis.fingerprint;
    let mut draft = WorldDraft {
        id: "tide".into(),
        display: "潮汐".into(),
        description: "雾港的潮汐\n每晚出现。".into(),
        ..Default::default()
    };
    project
        .edit(|p| p.write_wiki_entry(None, &draft, &["涨潮".into()]))
        .unwrap();
    let result = project.compile();
    assert!(!result.has_errors());
    assert_eq!(result.analysis.fingerprint, before);
    assert_eq!(
        result.analysis.catalog.tags["tide"].description,
        draft.description
    );
    assert_eq!(KeywordIndex::new(&result).find("涨潮涨潮").len(), 2);
    let source = project.sources();
    draft.description = "新的释义".into();
    assert!(project
        .edit(|p| p.write_wiki_entry(Some("tide"), &draft, &["\n".into()]))
        .is_err());
    assert_eq!(project.sources(), source);
    project
        .edit(|p| p.write_wiki_entry(Some("tide"), &draft, &["落潮".into()]))
        .unwrap();
    let index = KeywordIndex::new(&project.compile());
    assert!(index.find("涨潮").is_empty());
    assert_eq!(index.find("落潮").len(), 1);
    assert_eq!(project.compile().analysis.fingerprint, before);
    draft.display = "\n".into();
    assert!(project
        .edit(|p| p.write_wiki_entry(Some("tide"), &draft, &[]))
        .is_err());
}

#[test]
fn playback_keeps_explicit_links_after_interpolation_without_advancing_choices() {
    use worldline_runtime::{Output, Story};
    let source = r#"let prefix = "🧭雾港"
character lin as "林舟"
character mei as "梅"
tag harbor as "雾港"
event start
  {prefix}[[character:lin|梅]]~
  林舟
  choice "找{prefix}[[character:lin|梅]]"
    -> END
"#;
    let result = compile_source("play.wl", source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    let outputs = story.continue_story().unwrap();
    let Output::Text { content, links, .. } = &outputs[0] else {
        panic!("缺少正文")
    };
    assert_eq!(content, "🧭雾港梅");
    assert_eq!(&content[links[0].start..links[0].end], "梅");
    let before = story.save().unwrap();
    let index = KeywordIndex::new(&result);
    let matches = index.find_with_links(content, links);
    assert_eq!(
        matches.last().unwrap().targets,
        [TargetRef::new("character", "lin")]
    );
    assert!(matches
        .iter()
        .all(|m| !m.targets.contains(&TargetRef::new("character", "mei"))));
    assert!(matches!(&outputs[1], Output::Text { new_line: false, links, .. } if links.is_empty()));
    let choice = &story.choices()[0];
    assert_eq!(
        &choice.label[choice.links[0].start..choice.links[0].end],
        "梅"
    );
    assert_eq!(story.save().unwrap(), before);
    let json = serde_json::to_value(&outputs).unwrap();
    assert_eq!(json[0]["links"][0]["target"]["id"], "lin");
    assert!(json[1].get("links").is_none());
    story.choose(0).unwrap();
    assert_eq!(story.turns(), 1);
}

#[test]
fn index_and_runtime_file_links_follow_nested_workspace_sources() {
    use worldline_runtime::{Output, Story};
    let root = std::env::temp_dir().join(format!("worldline-wiki-nested-{}", std::process::id()));
    let mut project = Project::new(&root);
    project.documents.retain(|path, _| *path == project.entry);
    project
        .set_text(&project.entry.clone(), "tag harbor as \"雾港\"\n".into())
        .unwrap();
    let child = project
        .add_file(std::path::Path::new("chapters/start.wl"))
        .unwrap();
    let source =
        "event start\n  来到/* 中文注释 */雾港雾港\n  [[file:../world.wl|入口]]\n  -> END\n";
    project.set_text(&child, source.into()).unwrap();
    let result = project.compile();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let index = KeywordIndex::new(&result);
    let hits: Vec<_> = index
        .occurrences(&TargetRef::new("tag", "harbor"))
        .iter()
        .filter(|hit| hit.file == child)
        .collect();
    assert_eq!(hits.len(), 2);
    assert_eq!(
        hits[0].preview.chars().nth(hits[0].column as usize - 1),
        Some('雾')
    );
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    let output = story.continue_story().unwrap();
    assert!(output.iter().any(|output| matches!(output, Output::Text { links, .. } if links.iter().any(|link| link.target == TargetRef::new("file", &project.entry.to_string_lossy())))));
    let files = project.export_files().unwrap();
    assert_eq!(
        files[std::path::Path::new("chapters/start.wl")],
        source.as_bytes()
    );
}

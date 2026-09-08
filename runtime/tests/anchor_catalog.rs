use worldline_core::anchors::AnchorDraft;
use worldline_core::catalog::TargetRef;
use worldline_core::project::Project;
use worldline_core::{compile_source, CompileResult};

const WORLD: &str = r#"tag calm as "平静"
tag alert as "警觉"
character lin as "林舟"
state mood on character lin with calm as "心境"
state other on character lin with []
event a with lin
  effect on enter
    become mood with alert as "转折"
  effect on exit if has(mood, alert)
    become mood with []
  if has(mood, alert)
    become mood with calm
  choice "看一看"
    scene inner
      become mood with alert
      become other with calm
      anchor "旧手动记录"
      -> END
event b
  become mood with calm
  -> END
"#;

const ANCHORS: &str = r#"anchor_def turn as "转折点" // 名称注释
  // 叙事意义注释
  description "重新理解自己 // 不是注释" /* 描述注释 */
anchor_def echo as "回声"
anchor_link turn character lin // 角色关联注释
anchor_link turn event a
anchor_link turn state mood
anchor_link turn anchor echo
mark anchor turn with alert
asset notes file "notes.txt"
attach anchor turn with notes
"#;

fn valid(result: &CompileResult) {
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
}

fn project() -> Project {
    let root = std::env::temp_dir().join(format!("worldline-anchor-draft-{}", std::process::id()));
    let mut p = Project::new(&root);
    p.documents.retain(|path, _| path == &p.entry);
    let entry = p.entry.clone();
    p.set_text(&entry, WORLD.into()).unwrap();
    let metadata = p.add_file(std::path::Path::new("anchors.wl")).unwrap();
    p.set_text(&metadata, ANCHORS.into()).unwrap();
    valid(&p.compile());
    p
}

#[test]
fn renaming_character_updates_anchor_links_and_preserves_comments() {
    let mut p = project();
    let path = p.entry.clone();
    let draft = worldline_core::authoring::CharacterDraft {
        id: "linzhou".into(),
        display: "林舟".into(),
        ..Default::default()
    };
    p.edit(|p| p.write_character(&path, Some("lin"), &draft))
        .unwrap();
    let result = p.compile();
    valid(&result);
    assert_eq!(
        result
            .analysis
            .catalog
            .anchors_for(&TargetRef::new("character", "linzhou"))[0]
            .id,
        "turn"
    );
    assert!(p.documents.values().any(|d| d
        .text
        .contains("anchor_link turn character linzhou // 角色关联注释")));
}

#[test]
fn overview_collects_cross_file_event_bodies_and_separates_effects() {
    let mut p = project();
    let extra = p.add_file(std::path::Path::new("background.wl")).unwrap();
    p.set_text(&extra, "event background as \"背景事件\"\n  背景正文。\n  effect on enter\n    become other add alert\n  -> END\n".into()).unwrap();
    let drafts = p.event_drafts();
    assert_eq!(drafts.len(), 3);
    let (path, draft) = drafts.iter().find(|(_, d)| d.id == "background").unwrap();
    assert_eq!(path, &extra);
    assert!(draft.body.contains("背景正文。"));
    assert!(!draft.body.contains("effect on enter"));
    assert_eq!(draft.effects[0].actions.trim(), "become other add alert");
}

#[test]
fn cross_file_anchors_index_targets_marks_attachments_and_change_intersection() {
    let mut p = project();
    let result = p.compile();
    let catalog = &result.analysis.catalog;
    let anchor = &catalog.anchors["turn"];
    assert_eq!(anchor.description, "重新理解自己 // 不是注释");
    assert!(anchor.file.ends_with("anchors.wl"));
    assert_eq!(anchor.links.len(), 4);
    assert!(anchor
        .links
        .iter()
        .all(|l| l.file == anchor.file && l.line > anchor.line));
    assert_eq!(
        catalog.anchors_for(&TargetRef::new("character", "lin"))[0].id,
        "turn"
    );
    assert_eq!(
        catalog.anchors_for(&TargetRef::new("event", "a"))[0].id,
        "turn"
    );
    assert!(catalog
        .references_to(&TargetRef::new("event", "a"))
        .iter()
        .any(|r| {
            r.source == TargetRef::new("anchor", "turn") && r.file.ends_with("anchors.wl")
        }));
    assert_eq!(
        catalog.tags_for(&TargetRef::new("anchor", "turn")),
        ["alert"]
    );
    assert_eq!(
        catalog.assets_for(&TargetRef::new("anchor", "turn"))[0].id,
        "notes"
    );
    assert!(catalog
        .query("alert", true)
        .iter()
        .any(|o| o.target == TargetRef::new("anchor", "turn")));
    let changes = catalog.anchor_changes("turn");
    assert_eq!(changes.len(), 4);
    for (state, change) in &changes {
        assert_eq!(state.id, "mood");
        assert_eq!(change.event, "a");
        assert!(change.file.ends_with("world.wl"));
        assert!(state.changes.iter().any(|c| std::ptr::eq(c, *change)));
    }
    assert!(changes.iter().any(|(_, c)| c.timing == "enter"));
    assert!(changes
        .iter()
        .any(|(_, c)| c.timing == "exit" && !c.contexts.is_empty()));
    assert!(changes
        .iter()
        .any(|(_, c)| c.node == "a.inner" && !c.contexts.is_empty()));
    assert!(catalog.anchor_changes("echo").is_empty());
    assert!(catalog.anchor_changes("missing").is_empty());
    assert!(result.analysis.timeline.edges.is_empty());
    assert_eq!(catalog.anchors.len(), 2); // 正文手动 anchor 不产生独立对象。
}

#[test]
fn unknown_ids_and_duplicate_declarations_report_a217_at_the_source() {
    for source in [
        "anchor_link missing event a",
        "anchor_def x as \"X\"\nanchor_link x event missing",
        "anchor_def x as \"X\"\nanchor_link x character missing",
        "anchor_def x as \"X\"\nanchor_link x state missing",
        "anchor_def x as \"X\"\nanchor_link x anchor missing",
        "anchor_def x as \"X\"\nanchor_def x as \"Y\"",
    ] {
        let result = compile_source("invalid.wl", &format!("{source}\n{WORLD}"));
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.code == "A217" && d.file.ends_with("invalid.wl")));
    }
    let mut p = project();
    let entry = p.entry.clone();
    let text = format!(
        "anchor_def turn as \"重复\"\n{}",
        p.document(&entry).unwrap()
    );
    p.set_text(&entry, text).unwrap();
    assert!(p.compile().diagnostics.iter().any(|d| d.code == "A217"));
}

#[test]
fn malformed_anchor_syntax_is_rejected() {
    for source in [
        "anchor_def",
        "anchor_def x",
        "anchor_def \"x\" as \"X\"",
        "anchor_def x as name",
        "anchor_def x \"as\" \"X\"",
        "anchor_def x as \"X\"\n  property unsupported = true",
        "anchor_def x as \"X\"\n  description \"a\"\n  description \"b\"",
        "anchor_def x as \"X\"\nanchor_link x scene a.inner",
        "anchor_def x as \"X\"\nanchor_link x \"event\" a",
        "anchor_def x as \"X\"\nanchor_link x event \"a\"",
        "anchor_def x as \"X\"\nanchor_link x event",
        "anchor_def x as \"X\"\nanchor_link x event a extra",
    ] {
        assert!(
            compile_source("invalid.wl", &format!("{source}\n{WORLD}")).has_errors(),
            "{source}"
        );
    }
}

#[test]
fn editing_roundtrips_comments_identity_cross_file_links_and_fingerprint() {
    let mut p = project();
    let entry = p.entry.clone();
    let entry_text = format!("anchor_link turn event b // 跨文件注释\n/* 跨行\n注释 */\nanchor_link turn state other /* 尾部\n注释 */\n{}", p.document(&entry).unwrap());
    p.set_text(&entry, entry_text).unwrap();
    let before = p.compile();
    valid(&before);
    let fp = before.analysis.fingerprint;
    let draft = AnchorDraft {
        id: "turn".into(),
        display: "新的转折".into(),
        description: "保留 \\\"引号与换行\n第二行".into(),
        targets: vec![
            TargetRef::new("event", "a"),
            TargetRef::new("state", "mood"),
            TargetRef::new("event", "a"),
        ],
    };
    p.edit(|p| p.write_anchor(Some("turn"), &draft)).unwrap();
    let after = p.compile();
    valid(&after);
    assert_eq!(fp, after.analysis.fingerprint);
    let anchor = &after.analysis.catalog.anchors["turn"];
    assert_eq!(anchor.id, draft.id);
    assert_eq!(anchor.display, draft.display);
    assert_eq!(anchor.description, draft.description);
    assert_eq!(anchor.links.len(), 2);
    assert_eq!(after.analysis.catalog.anchor_changes("turn").len(), 4);
    let joined = p.sources().values().cloned().collect::<Vec<_>>().join("\n");
    for comment in [
        "名称注释",
        "叙事意义注释",
        "描述注释",
        "角色关联注释",
        "跨文件注释",
        "/* 跨行\n注释 */",
        "/* 尾部\n注释 */",
    ] {
        assert!(joined.contains(comment), "丢失注释: {comment}");
    }
    assert!(joined.contains("anchor \"旧手动记录\""));
    assert_eq!(
        after
            .analysis
            .catalog
            .assets_for(&TargetRef::new("anchor", "turn"))
            .len(),
        1
    );
    let draft = AnchorDraft::from(anchor);
    p.edit(|p| p.write_anchor(Some("turn"), &draft)).unwrap();
    let again = p.compile();
    assert_eq!(again.analysis.fingerprint, fp);
    assert_eq!(again.analysis.catalog.anchors["turn"].links.len(), 2);
    p.edit(|p| p.set_anchor_links("turn", &[TargetRef::new("state", "mood")]))
        .unwrap();
    assert!(p
        .compile()
        .analysis
        .catalog
        .anchor_changes("turn")
        .is_empty());
    p.edit(|p| p.set_anchor_links("turn", &[])).unwrap();
    assert!(p.compile().analysis.catalog.anchors["turn"]
        .links
        .is_empty());
}

#[test]
fn invalid_edits_roll_back_all_files_and_new_metadata_does_not_change_fingerprint() {
    let mut p = project();
    let result = p.compile();
    let mut draft = AnchorDraft::from(&result.analysis.catalog.anchors["turn"]);
    let before = p.sources();
    draft.targets.push(TargetRef::new("event", "missing"));
    assert!(p.edit(|p| p.write_anchor(Some("turn"), &draft)).is_err());
    assert_eq!(p.sources(), before);
    draft.targets.clear();
    draft.id = "changed_id".into();
    assert!(p.edit(|p| p.write_anchor(Some("turn"), &draft)).is_err());
    assert_eq!(p.sources(), before);
    draft.id = "turn".into();
    assert!(p.edit(|p| p.write_anchor(None, &draft)).is_err());
    assert!(p.edit(|p| p.set_anchor_links("missing", &[])).is_err());
    assert!(p
        .edit(|p| p.set_anchor_links("turn", &[TargetRef::new("scene", "a.inner")]))
        .is_err());
    assert_eq!(p.sources(), before);
    let fingerprint = result.analysis.fingerprint;
    draft.id = "new_anchor".into();
    p.edit(|p| p.write_anchor(None, &draft)).unwrap();
    let after = p.compile();
    assert_eq!(after.analysis.fingerprint, fingerprint);
    assert!(after.analysis.catalog.anchors.contains_key("new_anchor"));
    let plain = compile_source("world.wl", WORLD);
    let annotated = compile_source("world.wl", &format!("{ANCHORS}\n{WORLD}"));
    valid(&plain);
    valid(&annotated);
    assert_eq!(plain.analysis.fingerprint, annotated.analysis.fingerprint);

    let source = "event start\n  anchor \"旧手动记录\"\n  -> END\n";
    let plain = compile_source("world.wl", source);
    let annotated = compile_source(
        "world.wl",
        &format!("anchor_def moment as \"叙事意义\"\nanchor_link moment event start\n{source}"),
    );
    valid(&annotated);
    let mut story = worldline_runtime::Story::new(&plain.program, &plain.analysis).unwrap();
    story.continue_story().unwrap();
    let save = story.save().unwrap();
    let restored =
        worldline_runtime::Story::load(&annotated.program, &annotated.analysis, &save).unwrap();
    assert_eq!(restored.anchors().len(), 1);
    assert_eq!(
        restored.anchors()[0].kind,
        worldline_runtime::AnchorKind::Manual
    );
}

#[test]
fn anchor_changes_follow_state_operations_and_source_edits_without_saved_line_ids() {
    use worldline_core::ast::ChangeKind;
    let mut p = project();
    let entry = p.entry.clone();
    let text = p
        .document(&entry)
        .unwrap()
        .replace("become mood with alert as", "become mood add alert as")
        .replace("become mood with []", "become mood remove calm");
    p.set_text(&entry, text).unwrap();
    let before = p.compile();
    valid(&before);
    let sites = before.analysis.catalog.anchor_changes("turn");
    assert!(sites.iter().any(|(_, c)| c.kind == ChangeKind::AddTags));
    assert!(sites.iter().any(|(_, c)| c.kind == ChangeKind::RemoveTags));
    let positions: Vec<_> = sites.iter().map(|(_, c)| c.line).collect();
    let text = format!(
        "// 新增注释，不改变关联身份\n\n{}",
        p.document(&entry).unwrap()
    );
    p.set_text(&entry, text).unwrap();
    let after = p.compile();
    valid(&after);
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    let shifted: Vec<_> = after
        .analysis
        .catalog
        .anchor_changes("turn")
        .iter()
        .map(|(_, c)| c.line)
        .collect();
    assert_eq!(
        shifted,
        positions.iter().map(|line| line + 2).collect::<Vec<_>>()
    );
}

#[test]
fn replacing_links_loads_new_disk_includes_into_the_transaction() {
    let root = std::env::temp_dir().join(format!(
        "worldline-anchor-includes-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let extra = root.join("extra.wl");
    std::fs::write(&extra, "anchor_link turn event b // 新引用的注释\n").unwrap();
    let mut p = Project::new(&root);
    p.documents.retain(|path, _| path == &p.entry);
    let entry = p.entry.clone();
    p.set_text(&entry, format!("include \"extra.wl\"\n{ANCHORS}\n{WORLD}"))
        .unwrap();
    assert!(!p.documents.contains_key(&extra));
    p.edit(|p| p.set_anchor_links("turn", &[TargetRef::new("event", "a")]))
        .unwrap();
    let result = p.compile();
    valid(&result);
    let links = &result.analysis.catalog.anchors["turn"].links;
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, TargetRef::new("event", "a"));
    assert!(p.document(&extra).unwrap().contains("新引用的注释"));
    assert!(!p.document(&extra).unwrap().contains("anchor_link"));
    assert!(std::fs::read_to_string(&extra)
        .unwrap()
        .contains("anchor_link"));
    std::fs::remove_file(extra).unwrap();
    std::fs::remove_dir(root).unwrap();
}

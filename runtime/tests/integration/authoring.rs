use super::*;

fn authoring_project(root: &std::path::Path) -> Project {
    let mut project = Project::new(root);
    project
        .set_text(&project.entry.clone(), "period phase\ntag marker\n".into())
        .unwrap();
    for (relative, source) in [
        (
            "characters.wl",
            "character actor as \"甲\"\n  property age = 28\ncharacter peer\n",
        ),
        (
            "events/first.wl",
            concat!(
                "storyline lane_a\n",
                "  event first with actor at 10 during phase\n",
                "    choice \"继续\"\n      ->> other\n",
                "    choice \"结束\"\n      -> last\n",
                "  event last with actor at 20 during phase follows first, other\n",
                "    LAST\n    -> END\n",
            ),
        ),
        (
            "events/other.wl",
            "storyline lane_b\n  event other with actor, peer at 10 during phase\n    OTHER\n    -> END\n",
        ),
    ] {
        let path = project.add_file(std::path::Path::new(relative)).unwrap();
        project.set_text(&path, source.into()).unwrap();
    }
    project
}

#[test]
fn blank_project_save_reopen_and_play_supports_author_edits() {
    let dir = ProjectDir::new();
    let root = dir.0.join("blank");
    let mut project = Project::new(&root);
    assert_eq!(project.documents.len(), 1);
    assert_eq!(
        project.document(&project.entry).unwrap(),
        "event start\n  -> END\n"
    );
    project.save().unwrap();

    let mut reopened = Project::open(&root).unwrap();
    assert!(!reopened.is_dirty());
    let compiled = reopened.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let mut story = Story::new(&compiled.program, &compiled.analysis).unwrap();
    assert_eq!(transcript(&mut story), "[END]");
    assert!(story.is_ended());

    reopened
        .set_text(
            &reopened.entry.clone(),
            "event start\n  新稿\n  -> END\n".into(),
        )
        .unwrap();
    reopened.save().unwrap();
    let mut edited = Project::open(&root).unwrap();
    assert!(!edited.is_dirty());
    assert_eq!(edited.documents.len(), 1);
    let compiled = edited.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let mut story = Story::new(&compiled.program, &compiled.analysis).unwrap();
    assert_eq!(transcript(&mut story), "新稿[END]");
    assert!(story.is_ended());
}

#[test]
fn project_visual_authoring_export_reopen_and_play() {
    let dir = ProjectDir::new();
    let mut project = authoring_project(&dir.0.join("draft"));
    let original = project.compile();
    assert!(!original.has_errors(), "{:?}", original.diagnostics);
    assert_eq!(original.program.files.len(), 4);
    assert!(original.analysis.world.is_none());
    let path = project
        .add_file(std::path::Path::new("events/return.wl"))
        .unwrap();
    let draft = EventDraft {
        id: "added".into(),
        summary: "重返雾港".into(),
        storyline: "lane_a".into(),
        characters: vec!["actor".into()],
        body: "你终于回到了家。\n-> END".into(),
        ..Default::default()
    };
    project
        .edit(|p| p.write_event(&path, None, &draft))
        .unwrap();
    project
        .edit(|p| p.connect_events("other", "added", "回家", true))
        .unwrap();
    project
        .edit(|p| p.move_event("added", "lane_a", 1))
        .unwrap();
    let result = project.compile();
    let order: Vec<_> = result
        .analysis
        .graph
        .nodes
        .iter()
        .filter(|n| n.storyline == "lane_a")
        .map(|n| (&n.name, n.seq))
        .collect();
    assert!(order.contains(&(&"added".into(), 10)));
    assert!(result.analysis.symbols.characters["actor"]
        .events
        .contains(&"added".into()));
    let output = dir.0.join("delivery");
    project.export(&output).unwrap();
    assert!(output.join("world.wl").is_file());
    assert!(output.join("events/return.wl").is_file());
    let mut reopened = Project::open(&output).unwrap();
    assert!(!reopened.is_dirty());
    let compiled = reopened.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert_eq!(result.analysis.fingerprint, compiled.analysis.fingerprint);
    let mut story = Story::new(&compiled.program, &compiled.analysis).unwrap();
    transcript(&mut story);
    story.choose(0).unwrap();
    transcript(&mut story);
    assert_eq!(story.choices()[0].label, "回家");
    story.choose(0).unwrap();
    assert!(transcript(&mut story).contains("你终于回到了家"));
}

#[test]
fn character_metadata_roundtrip_rename_and_reverse_index() {
    let dir = ProjectDir::new();
    let mut project = authoring_project(&dir.0.join("draft"));
    let path = project.root.join("characters.wl");
    let draft = CharacterDraft {
        id: "actor_new".into(),
        display: "林\"舟".into(),
        properties: vec![
            (
                "quote".into(),
                PropertyValue::Str("他说:\"回来\"\nC:\\书".into()),
            ),
            ("age".into(), PropertyValue::Num(-2.5)),
        ],
        relations: vec![("peer".into(), "同伴".into())],
    };
    project
        .edit(|p| p.write_character(&path, Some("actor"), &draft))
        .unwrap();
    let result = project.compile();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(!result.analysis.symbols.characters.contains_key("actor"));
    let info = &result.analysis.symbols.characters["actor_new"];
    assert_eq!(info.display, "林\"舟");
    assert_eq!(info.properties["quote"], draft.properties[0].1);
    assert_eq!(info.events.len(), 3);
    assert_eq!(info.relations[0].target, "peer");
}

#[test]
fn partial_order_across_files_keeps_independent_events_and_control_flow() {
    let dir = ProjectDir::new();
    let mut project = authoring_project(&dir.0.join("world"));
    let initial = project.compile();
    assert!(initial.diagnostics.is_empty(), "{:?}", initial.diagnostics);
    let timeline = &initial.analysis.timeline;
    assert_eq!(timeline.periods.len(), 1);
    let rank = |name: &str| {
        timeline
            .events
            .iter()
            .find(|e| e.event == name)
            .unwrap()
            .rank
    };
    assert_eq!((rank("first"), rank("other"), rank("last")), (0, 0, 1));
    assert_eq!(timeline.edges.len(), 2);
    let before = initial.analysis.fingerprint;
    project.edit(|p| p.order_events("first", "other")).unwrap();
    let ordered = project.compile();
    assert_eq!(ordered.analysis.fingerprint, before);
    assert_eq!(
        ordered
            .analysis
            .timeline
            .events
            .iter()
            .find(|e| e.event == "last")
            .unwrap()
            .rank,
        2
    );
    assert!(project
        .sources()
        .values()
        .any(|text| text.contains("during phase follows first")));
    // 时间顺序约束不充当播放调度器,保持原有分支与准入行为。
    let mut story = Story::new(&ordered.program, &ordered.analysis).unwrap();
    transcript(&mut story);
    story.choose(1).unwrap();
    let ending = transcript(&mut story);
    assert!(ending.contains("LAST"));
    assert!(!ending.contains("OTHER"));
    let saved = project.sources();
    assert!(project.edit(|p| p.order_events("last", "first")).is_err());
    assert_eq!(project.sources(), saved);
    let export = dir.0.join("export");
    project.export(&export).unwrap();
    let exported = compile_path(&export).unwrap();
    assert_eq!(exported.analysis.timeline.edges.len(), 3);
    assert!(exported
        .analysis
        .timeline
        .to_mermaid(&exported.analysis.graph)
        .contains("先于"));
}

#[test]
fn unordered_world_records_need_no_game_exits_or_reachability() {
    let source = "period night as \"夜晚\"\nevent a during night\n  港口记录。\nevent b during night\n  灯塔记录。\nevent c during night follows a\n  次日记录。\n";
    let result = compile_source("records.wl", source);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(result.analysis.timeline.edges.len(), 1);
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    let text = transcript(&mut story);
    assert!(text.contains("港口记录"));
    assert!(!text.contains("灯塔记录"));
    let plain = compile_source(
        "records.wl",
        &source
            .replace("period night as \"夜晚\"\n", "")
            .replace(" during night", "")
            .replace(" follows a", ""),
    );
    assert_eq!(result.analysis.fingerprint, plain.analysis.fingerprint);
}

#[test]
fn invalid_time_constraints_are_rejected() {
    for source in [
        "period p\nevent a during missing\n  文本\n",
        "period p\nevent a during p follows ghost\n  文本\n",
        "period p\nevent a during p follows a\n  文本\n",
        "period p\nevent a during p follows b\n  文本\nevent b during p follows a\n  文本\n",
        "period p\nperiod q\nevent a during p\n  文本\nevent b during q follows a\n  文本\n",
        "event a follows b\n  -> END\nevent b\n  -> END\n",
    ] {
        let result = compile_source("time.wl", source);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "A213" && d.severity == Severity::Error),
            "{source}: {:?}",
            result.diagnostics
        );
    }
    assert!(
        compile_source("time.wl", "period p\nperiod p\nevent a during p\n  文本\n")
            .diagnostics
            .iter()
            .any(|d| d.code == "A104")
    );
}

#[test]
fn tag_pointers_deduplicate_cycles_and_reference_whole_objects() {
    let result = compile_source("tags.wl", "tag coordinate as \"坐标\"\ntag port as \"港口\"\nmark tag port with coordinate\nmark tag coordinate with port\nmark event start with coordinate, port\nevent start\n  码头记录 #port\n  -> END\n");
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let catalog = &result.analysis.catalog;
    assert_eq!(catalog.query("coordinate", false).len(), 2);
    let recursive = catalog.query("coordinate", true);
    assert_eq!(
        recursive
            .iter()
            .filter(|o| o.target.kind == "event")
            .count(),
        1
    );
    assert!(recursive.iter().all(|o| o.target.kind != "text"));
    let target = worldline_core::catalog::TargetRef::new("event", "start");
    assert_eq!(catalog.tags_for(&target), ["coordinate", "port"]);
    assert!(catalog
        .references_to(&target)
        .iter()
        .any(|r| r.source.id == "port" && r.line == 7));
}

#[test]
fn tag_and_attachment_targets_validate_without_runtime_changes() {
    for (source, code) in [
        ("tag a\ntag a\nevent start\n  -> END\n", "A104"),
        (
            "tag a\nmark event missing with a\nevent start\n  -> END\n",
            "A214",
        ),
        (
            "mark event start with missing\nevent start\n  -> END\n",
            "A214",
        ),
        (
            "attach event start with missing\nevent start\n  -> END\n",
            "A214",
        ),
        (
            "tag a\n  property x = 1\n  property x = 2\nevent start\n  -> END\n",
            "A212",
        ),
    ] {
        assert!(
            compile_source("invalid.wl", source)
                .diagnostics
                .iter()
                .any(|d| d.code == code),
            "{source}"
        );
    }
    let plain = compile_source("tags.wl", "event start\n  正文\n  -> END\n");
    let tagged = compile_source(
        "tags.wl",
        "tag a as \"对象引用\"\nmark event start with a\nevent start\n  正文\n  -> END\n",
    );
    assert_eq!(plain.analysis.fingerprint, tagged.analysis.fingerprint);
    let mut story = Story::new(&tagged.program, &tagged.analysis).unwrap();
    assert_eq!(transcript(&mut story), "正文[END]");
}

#[test]
fn attachments_export_and_save_as_are_portable_and_preserve_sources() {
    use worldline_core::catalog::TargetRef;
    let dir = ProjectDir::new();
    let external = dir.0.join("draft/assets");
    std::fs::create_dir_all(&external).unwrap();
    let picture = external.join("概念 图.PNG");
    let voice = external.join("人物声音.wav");
    let reference = external.join("设计说明");
    std::fs::write(&picture, b"image fixture").unwrap();
    std::fs::write(&voice, b"audio fixture").unwrap();
    std::fs::write(&reference, "其他格式资料").unwrap();
    let mut project = authoring_project(&dir.0.join("draft"));
    let event = TargetRef::new("event", "first");
    let person = TargetRef::new("character", "actor");
    project
        .edit(|p| {
            p.add_asset_reference(&event, &picture)?;
            p.add_asset_reference(&person, &voice)?;
            p.add_asset_reference(&person, &reference)?;
            p.add_asset_reference(&person, &picture)?;
            Ok(())
        })
        .unwrap();
    let result = project.compile();
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(result.analysis.catalog.assets.len(), 3);
    assert_eq!(result.analysis.catalog.assets_for(&person).len(), 3);
    let before = project.sources();
    let output = dir.0.join("delivery");
    project.export(&output).unwrap();
    assert_eq!(project.sources(), before);
    assert!(!output.join("spec/catalog.md").exists());
    assert_eq!(std::fs::read_dir(output.join("assets")).unwrap().count(), 3);
    let compiled = compile_path(&output).unwrap();
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    // Windows 临时目录可能使用短路径或不同大小写，比较前统一解析真实路径。
    let canonical_output = output.canonicalize().unwrap();
    for asset in compiled.analysis.catalog.assets.values() {
        assert!(!std::path::Path::new(&asset.path).is_absolute());
        assert!(std::path::Path::new(&asset.resolved_path)
            .canonicalize()
            .unwrap()
            .starts_with(&canonical_output));
    }
    assert_eq!(
        std::fs::read(output.join("assets/概念 图.PNG")).unwrap(),
        b"image fixture"
    );
    assert_eq!(
        std::fs::read_to_string(output.join("assets/设计说明")).unwrap(),
        "其他格式资料"
    );
    project.save_as(&dir.0.join("saved")).unwrap();
    assert!(project.compile().diagnostics.is_empty());
    std::fs::remove_file(output.join("assets/概念 图.PNG")).unwrap();
    let mut broken = Project::open(&output).unwrap();
    assert!(broken
        .compile()
        .diagnostics
        .iter()
        .any(|d| d.code == "A215"));
    assert!(broken.export(&dir.0.join("incomplete")).is_err());
    assert!(!dir.0.join("incomplete").exists());
    // 原文件及其他引用不被删除。
    assert_eq!(std::fs::read(&picture).unwrap(), b"image fixture");
}

#[test]
fn character_rename_preserves_tag_and_asset_references() {
    let dir = ProjectDir::new();
    let mut project = authoring_project(&dir.0.join("draft"));
    let target = worldline_core::catalog::TargetRef::new("character", "actor");
    project
        .edit(|p| p.set_catalog_links(&target, &["marker".into()], false))
        .unwrap();
    std::fs::create_dir_all(&project.root).unwrap();
    let external = project.root.join("声音参考.txt");
    std::fs::write(&external, "声音描述").unwrap();
    project
        .edit(|p| {
            p.add_asset_reference(&target, &external)?;
            Ok(())
        })
        .unwrap();
    let path = project.root.join("characters.wl");
    let info = project.compile().analysis.symbols.characters["actor"].clone();
    let draft = CharacterDraft {
        id: "actor_updated".into(),
        display: "林舟".into(),
        properties: info.properties.into_iter().collect(),
        relations: vec![("peer".into(), "同行者".into())],
    };
    project
        .edit(|p| p.write_character(&path, Some("actor"), &draft))
        .unwrap();
    let catalog = project.compile().analysis.catalog;
    let renamed = worldline_core::catalog::TargetRef::new("character", "actor_updated");
    assert_eq!(catalog.tags_for(&renamed), ["marker"]);
    assert_eq!(catalog.assets_for(&renamed).len(), 1);
    assert!(catalog.assets_for(&target).is_empty());
}

#[test]
fn authoring_preserves_comments_and_searches_unsaved_unicode_text() {
    let dir = ProjectDir::new();
    let mut project = authoring_project(&dir.0.join("world"));
    let path = project.root.join("characters.wl");
    let text = project
        .document(&path)
        .unwrap()
        .replace(
            "character actor as \"甲\"",
            "character actor as \"甲\" // 港口作者的注释",
        )
        .replace("property age = 28", "property age = 28 /* 待核对年龄 */");
    project.set_text(&path, text).unwrap();
    let info = project.compile().analysis.symbols.characters["actor"].clone();
    let draft = CharacterDraft {
        id: "actor".into(),
        display: "林舟新名".into(),
        properties: info.properties.into_iter().collect(),
        relations: vec![("peer".into(), "同伴".into())],
    };
    project
        .edit(|p| p.write_character(&path, Some("actor"), &draft))
        .unwrap();
    let text = project.document(&path).unwrap();
    assert!(text.contains("// 港口作者的注释"));
    assert!(text.contains("/* 待核对年龄 */"));
    let (path, mut event) = project.event_draft("first").unwrap();
    let text = project
        .document(&path)
        .unwrap()
        .replace("at 10 during phase", "at 10 during phase // 保留事件注释");
    project.set_text(&path, text).unwrap();
    event.summary = "港口新记录".into();
    project
        .edit(|p| p.write_event(&path, Some("first"), &event))
        .unwrap();
    assert!(project.document(&path).unwrap().contains("// 保留事件注释"));
    let results = project.search("新记录");
    assert_eq!(results.len(), 1);
    let hit = &results[0];
    assert_eq!(hit.file, path);
    assert_eq!(
        hit.preview
            .chars()
            .skip(hit.column as usize - 1)
            .take(3)
            .collect::<String>(),
        "新记录"
    );
    assert!(project.search("不会出现的文字").is_empty());
    assert!(project.search("").is_empty());
}

#[test]
fn reverse_character_index_includes_nested_changes_and_effects_once() {
    let result = compile_source("index.wl", "character a\ncharacter b\nevent start with a\n  effect on enter\n    meet b\n  if true\n    meet a\n    part b\n  choice \"继续\"\n    meet b\n    -> END\n");
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(result.analysis.symbols.characters["a"].events, ["start"]);
    assert_eq!(result.analysis.symbols.characters["b"].events, ["start"]);
}

#[test]
fn metadata_errors_and_transaction_rollback_preserve_source() {
    for (source, code) in [
        ("world a\nworld b\nevent start\n  -> END\n", "A211"),
        (
            "character a\n  property age = 1\n  property age = 2\nevent start\n  -> END\n",
            "A212",
        ),
        (
            "character a\n  relation missing as \"朋友\"\nevent start\n  -> END\n",
            "A208",
        ),
        (
            "world a\n  property age = rnd(1, 3)\nevent start\n  -> END\n",
            "P004",
        ),
        ("event start at 0\n  -> END\n", "P004"),
    ] {
        let result = compile_source("invalid.wl", source);
        assert!(
            result.diagnostics.iter().any(|d| d.code == code),
            "{source}: {:?}",
            result.diagnostics
        );
    }
    let dir = ProjectDir::new();
    let mut project = authoring_project(&dir.0.join("draft"));
    let before = project.sources();
    assert!(project.edit(|p| p.remove_event("other")).is_err());
    assert_eq!(project.sources(), before);
    let (path, mut draft) = project.event_draft("first").unwrap();
    draft.characters.push("missing".into());
    assert!(project
        .edit(|p| p.write_event(&path, Some("first"), &draft))
        .is_err());
    assert_eq!(project.sources(), before);
}

#[test]
fn project_save_detects_collaborator_changes_and_undo_after_save_is_dirty() {
    let dir = ProjectDir::new();
    let mut project = authoring_project(&dir.0.join("draft"));
    project.save_as(&dir.0.join("saved")).unwrap();
    let previous = project.clone();
    let (path, mut draft) = project.event_draft("first").unwrap();
    draft.summary = "已修改".into();
    project
        .edit(|p| p.write_event(&path, Some("first"), &draft))
        .unwrap();
    project.save().unwrap();
    assert!(!project.is_dirty());
    project.restore(previous);
    assert!(project.is_dirty());
    std::fs::write(&path, "// 协作者修改\n").unwrap();
    assert!(project.save().is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "// 协作者修改\n");
}

#[test]
fn project_export_rejects_external_references_and_existing_destination() {
    let dir = ProjectDir::new();
    let mut project = Project::new(&dir.0.join("draft"));
    let entry = project.entry.clone();
    std::fs::write(dir.0.join("outside.wl"), "event outside\n  -> END\n").unwrap();
    project
        .set_text(
            &entry,
            format!(
                "{}\ninclude \"../outside.wl\"\n",
                project.document(&entry).unwrap()
            ),
        )
        .unwrap();
    assert!(project
        .compile()
        .diagnostics
        .iter()
        .any(|d| d.code == "A109"));
    let target = dir.0.join("output");
    assert!(project.export(&target).is_err());
    assert!(!target.exists());
    let project = Project::new(&dir.0.join("clean"));
    project.export(&target).unwrap();
    std::fs::write(target.join("precious.txt"), "保留").unwrap();
    assert!(project.export(&target).is_err());
    assert_eq!(
        std::fs::read_to_string(target.join("precious.txt")).unwrap(),
        "保留"
    );
}

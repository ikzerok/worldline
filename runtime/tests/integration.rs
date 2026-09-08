//! 集成测试:示例故事编译、试玩走查、诊断、存读档。
//! 位于 runtime 包(core + runtime 均可用)。

use worldline_core::ast::PropertyValue;
use worldline_core::authoring::{CharacterDraft, EventDraft};
use worldline_core::project::Project;
use worldline_core::{compile_path, compile_source, EdgeKind, Severity};
use worldline_runtime::{AnchorKind, Output, Story};

fn transcript(story: &mut Story) -> String {
    let mut buf = String::new();
    for o in story.continue_story().unwrap() {
        match o {
            Output::Text {
                content, new_line, ..
            } => {
                if new_line && !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&content);
            }
            Output::Ended => buf.push_str("[END]"),
        }
    }
    buf
}

/// 按脚本走完一个故事,收集全部输出(选择处消费脚本)。
fn play_all(source: &str, script: &[usize]) -> (String, Vec<String>) {
    let result = compile_source("test.wl", source);
    assert!(
        !result.has_errors(),
        "编译存在错误:{:#?}",
        result.diagnostics
    );
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    let mut log = String::new();
    let mut labels = Vec::new();
    loop {
        log.push_str(&transcript(&mut story));
        if story.is_ended() {
            return (log, labels);
        }
        let choices: Vec<String> = story.choices().iter().map(|c| c.label.clone()).collect();
        labels.push(choices.join("|"));
        let n = script.get(labels.len() - 1).copied().unwrap_or(0);
        if n >= choices.len() {
            panic!("脚本第 {} 步越界:{:?}", labels.len(), choices);
        }
        story.choose(n).unwrap();
        log.push_str(&format!(" <<{}>> ", choices[n]));
    }
}

fn example(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../examples")
        .join(name)
}

struct ProjectDir(std::path::PathBuf);
impl ProjectDir {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "worldline-project-test-{}-{id}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for ProjectDir {
    fn drop(&mut self) {
        assert!(self.0.is_absolute() && self.0.starts_with(std::env::temp_dir()));
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("worldline-project-test-"));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn project_visual_authoring_export_reopen_and_play() {
    let dir = ProjectDir::new();
    let mut project = Project::new(&dir.0.join("draft"));
    let original = project.compile();
    assert!(!original.has_errors(), "{:?}", original.diagnostics);
    assert_eq!(original.program.files.len(), 4);
    assert_eq!(
        original.analysis.world.as_ref().unwrap().display,
        "雾港纪事"
    );
    let path = project
        .add_file(std::path::Path::new("events/return.wl"))
        .unwrap();
    let draft = EventDraft {
        id: "return_home".into(),
        summary: "重返雾港".into(),
        storyline: "harbor".into(),
        characters: vec!["lin".into()],
        body: "你终于回到了家。\n-> END".into(),
        ..Default::default()
    };
    project
        .edit(|p| p.write_event(&path, None, &draft))
        .unwrap();
    project
        .edit(|p| p.connect_events("beacon", "return_home", "回家", true))
        .unwrap();
    project
        .edit(|p| p.move_event("return_home", "harbor", 1))
        .unwrap();
    let result = project.compile();
    let order: Vec<_> = result
        .analysis
        .graph
        .nodes
        .iter()
        .filter(|n| n.storyline == "harbor")
        .map(|n| (&n.name, n.seq))
        .collect();
    assert!(order.contains(&(&"return_home".into(), 10)));
    assert!(result.analysis.symbols.characters["lin"]
        .events
        .contains(&"return_home".into()));
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
    let mut project = Project::new(&dir.0.join("draft"));
    let path = project.root.join("characters.wl");
    let draft = CharacterDraft {
        id: "lin_new".into(),
        display: "林\"舟".into(),
        properties: vec![
            (
                "quote".into(),
                PropertyValue::Str("他说:\"回来\"\nC:\\书".into()),
            ),
            ("age".into(), PropertyValue::Num(-2.5)),
        ],
        relations: vec![("mei".into(), "同伴".into())],
    };
    project
        .edit(|p| p.write_character(&path, Some("lin"), &draft))
        .unwrap();
    let result = project.compile();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(!result.analysis.symbols.characters.contains_key("lin"));
    let info = &result.analysis.symbols.characters["lin_new"];
    assert_eq!(info.display, "林\"舟");
    assert_eq!(info.properties["quote"], draft.properties[0].1);
    assert_eq!(info.events.len(), 3);
    assert_eq!(info.relations[0].target, "mei");
}

#[test]
fn partial_order_across_files_keeps_independent_events_and_control_flow() {
    let dir = ProjectDir::new();
    let mut project = Project::new(&dir.0.join("world"));
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
    assert_eq!(
        (rank("arrival"), rank("beacon"), rank("farewell")),
        (0, 0, 1)
    );
    assert_eq!(timeline.edges.len(), 2);
    let before = initial.analysis.fingerprint;
    project
        .edit(|p| p.order_events("arrival", "beacon"))
        .unwrap();
    let ordered = project.compile();
    assert_eq!(ordered.analysis.fingerprint, before);
    assert_eq!(
        ordered
            .analysis
            .timeline
            .events
            .iter()
            .find(|e| e.event == "farewell")
            .unwrap()
            .rank,
        2
    );
    assert!(project
        .sources()
        .values()
        .any(|text| text.contains("during storm_night follows arrival")));
    // 时间顺序约束不充当播放调度器,保持原有分支与准入行为。
    let mut story = Story::new(&ordered.program, &ordered.analysis).unwrap();
    transcript(&mut story);
    story.choose(1).unwrap();
    let ending = transcript(&mut story);
    assert!(ending.contains("林舟将信收好"));
    assert!(!ending.contains("重新点亮灯塔"));
    let saved = project.sources();
    assert!(project
        .edit(|p| p.order_events("farewell", "arrival"))
        .is_err());
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
    let mut project = Project::new(&dir.0.join("draft"));
    let event = TargetRef::new("event", "arrival");
    let person = TargetRef::new("character", "lin");
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
    let mut project = Project::new(&dir.0.join("draft"));
    let target = worldline_core::catalog::TargetRef::new("character", "lin");
    project
        .edit(|p| p.set_catalog_links(&target, &["harbor_place".into()], false))
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
    let info = project.compile().analysis.symbols.characters["lin"].clone();
    let draft = CharacterDraft {
        id: "lin_updated".into(),
        display: "林舟".into(),
        properties: info.properties.into_iter().collect(),
        relations: vec![("mei".into(), "同行者".into())],
    };
    project
        .edit(|p| p.write_character(&path, Some("lin"), &draft))
        .unwrap();
    let catalog = project.compile().analysis.catalog;
    let renamed = worldline_core::catalog::TargetRef::new("character", "lin_updated");
    assert_eq!(catalog.tags_for(&renamed), ["harbor_place"]);
    assert_eq!(catalog.assets_for(&renamed).len(), 1);
    assert!(catalog.assets_for(&target).is_empty());
}

#[test]
fn authoring_preserves_comments_and_searches_unsaved_unicode_text() {
    let dir = ProjectDir::new();
    let mut project = Project::new(&dir.0.join("world"));
    let path = project.root.join("characters.wl");
    let text = project
        .document(&path)
        .unwrap()
        .replace(
            "character lin as \"林舟\"",
            "character lin as \"林舟\" // 港口作者的注释",
        )
        .replace("property age = 28", "property age = 28 /* 待核对年龄 */");
    project.set_text(&path, text).unwrap();
    let info = project.compile().analysis.symbols.characters["lin"].clone();
    let draft = CharacterDraft {
        id: "lin".into(),
        display: "林舟新名".into(),
        properties: info.properties.into_iter().collect(),
        relations: vec![("mei".into(), "同伴".into())],
    };
    project
        .edit(|p| p.write_character(&path, Some("lin"), &draft))
        .unwrap();
    let text = project.document(&path).unwrap();
    assert!(text.contains("// 港口作者的注释"));
    assert!(text.contains("/* 待核对年龄 */"));
    let (path, mut event) = project.event_draft("arrival").unwrap();
    let text = project.document(&path).unwrap().replace(
        "at 10 during storm_night",
        "at 10 during storm_night // 保留事件注释",
    );
    project.set_text(&path, text).unwrap();
    event.summary = "港口新记录".into();
    project
        .edit(|p| p.write_event(&path, Some("arrival"), &event))
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
    let mut project = Project::new(&dir.0.join("draft"));
    let before = project.sources();
    assert!(project.edit(|p| p.remove_event("beacon")).is_err());
    assert_eq!(project.sources(), before);
    let (path, mut draft) = project.event_draft("arrival").unwrap();
    draft.characters.push("missing".into());
    assert!(project
        .edit(|p| p.write_event(&path, Some("arrival"), &draft))
        .is_err());
    assert_eq!(project.sources(), before);
}

#[test]
fn project_save_detects_collaborator_changes_and_undo_after_save_is_dirty() {
    let dir = ProjectDir::new();
    let mut project = Project::new(&dir.0.join("draft"));
    project.save_as(&dir.0.join("saved")).unwrap();
    let previous = project.clone();
    let (path, mut draft) = project.event_draft("arrival").unwrap();
    draft.summary = "已修改".into();
    project
        .edit(|p| p.write_event(&path, Some("arrival"), &draft))
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

#[test]
fn memory_include_deduplication_cycles_and_entry_priority() {
    let dir = ProjectDir::new();
    let root = &dir.0;
    let entry = root.join("main.wl");
    let mut sources = std::collections::BTreeMap::from([
        (
            entry.clone(),
            "include \"chapter.wl\"\nevent start\n  -> sub\n".into(),
        ),
        (
            root.join("chapter.wl"),
            "include \"characters.wl\"\nevent sub with a\n  -> END\n".into(),
        ),
        (root.join("characters.wl"), "character a\n".into()),
    ]);
    let result = worldline_core::compile_sources(&entry, &sources);
    assert!(!result.has_errors());
    assert_eq!(result.program.entry, "start");
    sources
        .get_mut(&entry)
        .unwrap()
        .push_str("\ninclude \"characters.wl\"\n");
    assert_eq!(
        worldline_core::compile_sources(&entry, &sources)
            .analysis
            .stats
            .characters,
        1
    );
    sources
        .get_mut(&root.join("characters.wl"))
        .unwrap()
        .push_str("include \"main.wl\"\n");
    assert!(worldline_core::compile_sources(&entry, &sources)
        .diagnostics
        .iter()
        .any(|d| d.code == "A105"));
}

#[test]
fn layout_order_does_not_change_save_fingerprint() {
    let a = compile_source("story.wl", "event start at 10\n  -> END\n");
    let b = compile_source("story.wl", "event start at 50\n  -> END\n");
    assert_eq!(a.analysis.fingerprint, b.analysis.fingerprint);
    let a = compile_source(
        "story.wl",
        "world a\n  property era = \"古代\"\nevent start\n  -> END\n",
    );
    let b = compile_source(
        "story.wl",
        "world a\n  property era = \"现代\"\nevent start\n  -> END\n",
    );
    assert_ne!(a.analysis.fingerprint, b.analysis.fingerprint);
}

// ---------------------------------------------------------------------------

#[test]
fn examples_compile_clean() {
    for name in ["minimal.wl", "mansion.wl"] {
        let result = compile_path(&example(name)).expect("读取示例失败");
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{name} 存在错误:{errors:#?}");
    }
}

#[test]
fn include_example_entry_is_main_file() {
    let result = compile_path(&example("include-main.wl")).unwrap();
    assert_eq!(
        result.program.entry, "start",
        "入口必须是主文件的第一个事件"
    );
    assert!(
        result.program.events.iter().any(|e| e.name == "chapter2"),
        "include 的事件应被合并"
    );
}

#[test]
fn playthrough_minimal_left() {
    let (log, labels) = play_all(
        r#"
event start
  世界线在此分岔。
  choice "向左"
    左边是一条河。~
    河水很凉。
    -> END
  choice "向右"
    右边是一座山。
    -> END
"#,
        &[0],
    );
    assert_eq!(labels, vec!["向左|向右"]);
    assert_eq!(
        log,
        "世界线在此分岔。 <<向左>> 左边是一条河。河水很凉。[END]"
    );
}

#[test]
fn playthrough_conditions_and_vars() {
    let src = r#"
let coins = 5

event start
  你有 {coins} 枚硬币。
  choice "买地图" if coins >= 10
    -> END
  choice "买半张" if coins >= 4
    set coins = coins - 4
    还剩 {coins} 枚。
    -> END
"#;
    let (log, labels) = play_all(src, &[0]);
    // 条件不满足:买地图被过滤,首选项是"买半张"
    assert_eq!(labels, vec!["买半张"]);
    assert_eq!(log, "你有 5 枚硬币。 <<买半张>> 还剩 1 枚。[END]");
}

#[test]
fn once_choice_disappears_after_taken() {
    let src = r#"
event room
  房间一览无余。
  choice once "开宝箱"
    宝箱空了。
  choice "离开"
    -> END
  -> room
"#;
    let (_, labels) = play_all(src, &[0, 0]);
    assert_eq!(
        labels,
        vec!["开宝箱|离开", "离开"],
        "once 选择被选后应从列表消失"
    );
}

#[test]
fn fallback_falls_through_when_all_cond_fail() {
    let src = r#"
event start
  choice "敲门" if false
    -> END
  没有可用的选择,你转身离开。
  -> END
"#;
    let (log, labels) = play_all(src, &[]);
    assert!(labels.is_empty(), "无可用选择时不暂停:{labels:?}");
    assert_eq!(log, "没有可用的选择,你转身离开。[END]");
}

#[test]
fn gather_semantics_shared_continuation() {
    let src = r#"
event start
  choice "A"
    你选了 A。
  choice "B"
    你选了 B。
  两条路在此汇聚。~
  然后。
  -> END
"#;
    let (log_a, _) = play_all(src, &[0]);
    let (log_b, _) = play_all(src, &[1]);
    // ~ 粘接方向:作用于其后一行(汇聚行粘接"然后"行)
    assert!(
        log_a.contains("你选了 A。\n两条路在此汇聚。然后。"),
        "{log_a}"
    );
    assert!(
        log_b.contains("你选了 B。\n两条路在此汇聚。然后。"),
        "{log_b}"
    );
}

#[test]
fn nested_choice_inner_gather() {
    let src = r#"
event start
  choice "外层一"
    choice "内层甲"
      甲。
    内层汇聚点。
  外层汇聚点。
  -> END
"#;
    let (log, _) = play_all(src, &[0, 0]);
    assert!(log.contains("甲。\n内层汇聚点。\n外层汇聚点。"), "{log}");
}

#[test]
fn visits_and_turns() {
    let src = r#"
event start
  第 {visits(start)} 次来到这里,共 {turns()} 回合。
  choice "再来一次" if visits(start) < 3
    -> start
  choice "结束"
    -> END
"#;
    let (log, labels) = play_all(src, &[0, 0, 0]);
    assert_eq!(labels.len(), 3);
    assert!(log.contains("第 1 次来到这里,共 0 回合。"), "{log}");
    assert!(log.contains("第 2 次来到这里,共 1 回合。"), "{log}");
    assert!(log.contains("第 3 次来到这里,共 2 回合。"), "{log}");
}

#[test]
fn scene_nesting_and_divert() {
    let src = r#"
event market
  你走进集市。
  scene stall
    摊主向你招手。~
    "来看看。"
    -> market.exit

event market.exit
  你离开了集市。
  -> END
"#;
    let (log, _) = play_all(src, &[]);
    assert!(
        log.contains("你走进集市。\n摊主向你招手。\"来看看。\""),
        "{log}"
    );
    assert!(log.contains("你离开了集市。"), "{log}");
}

#[test]
fn mansion_full_walkthrough() {
    // 敲门 → 去书房 → 翻开日记(得钥匙) → 去地窖 → 勇气不足退回
    let result = compile_path(&example("mansion.wl")).unwrap();
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let mut log = String::new();
    log.push_str(&transcript(&mut s)); // start 暂停
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["敲门", "绕到后院", "多等一会儿"]
    );
    s.choose(0).unwrap(); // 敲门 courage=1
    log.push_str(&transcript(&mut s)); // hall → hall.choice 暂停
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["去书房", "回门口"],
        "无钥匙时地窖不应出现"
    );
    s.choose(0).unwrap(); // 去书房
    log.push_str(&transcript(&mut s)); // study 暂停
    s.choose(0).unwrap(); // 翻开日记(once,得钥匙)
    log.push_str(&transcript(&mut s)); // 汇聚 → hall.choice 暂停
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["去书房", "去地窖", "回门口"],
        "有钥匙后地窖应出现"
    );
    s.choose(1).unwrap(); // 去地窖(courage=1 < 3)
    log.push_str(&transcript(&mut s)); // cellar else 分支 → hall.choice
    assert!(log.contains("日记的最后一页夹着一把小钥匙。"), "{log}");
    assert!(log.contains("你合上了门。"), "{log}");
    assert!(log.contains("黑暗浓得化不开"), "{log}");
}

#[test]
fn rnd_within_bounds_and_save() {
    let src = r#"
event start
  骰子:{rnd(1, 6)}。
  -> END
"#;
    for _ in 0..20 {
        let result = compile_source("r.wl", src);
        let mut s = Story::new(&result.program, &result.analysis).unwrap();
        let log = transcript(&mut s);
        let n: f64 = log
            .trim_start_matches("骰子:")
            .trim_end_matches("。[END]")
            .parse()
            .unwrap();
        assert!((1.0..=6.0).contains(&n), "{log}");
    }
}

// -- 存读档 -------------------------------------------------------------------

#[test]
fn save_load_roundtrip() {
    let src = r#"
let courage = 0

event start
  你有 {courage} 点勇气。
  choice "加勇"
    set courage = courage + 1
    -> mid
  choice "直接走"
    -> mid

event mid
  中场:勇气 {courage}。
  choice "结局" if courage >= 1
    好结局。
    -> END
  choice "坏结局"
    坏结局。
    -> END
"#;
    let result = compile_source("save.wl", src);
    let mut a = Story::new(&result.program, &result.analysis).unwrap();
    transcript(&mut a);
    a.choose(0).unwrap(); // 加勇 → courage=1,进入 mid 并暂停
    transcript(&mut a);
    let json = a.save().unwrap();

    // 恢复:存档不含暂停态,continue 会重新遇到选择组并再次暂停
    let mut b = Story::load(&result.program, &result.analysis, &json).unwrap();
    let silence = transcript(&mut b);
    assert!(silence.is_empty(), "重建暂停不应产生新输出:{silence}");
    let labels: Vec<String> = b.choices().iter().map(|c| c.label.clone()).collect();
    assert_eq!(labels, vec!["结局", "坏结局"]);
    b.choose(0).unwrap();
    let out = transcript(&mut b);
    assert!(out.contains("好结局。"), "{out}");
}

#[test]
fn save_load_mid_text_execution() {
    // 在选择体内执行到一半(嵌套帧)时存档,恢复后继续执行剩余语句
    let src = r#"
event start
  choice "走"
    第一句。~
    -> keep
  -> END

event keep
  choice "继续"
    后半句 A。
    -> END
  choice "停"
    后半句 B。
    -> END
"#;
    let result = compile_source("s2.wl", src);
    let mut a = Story::new(&result.program, &result.analysis).unwrap();
    transcript(&mut a);
    a.choose(0).unwrap(); // 进入选择体:文本 + divert → keep 暂停
    let log1 = transcript(&mut a);
    assert!(log1.contains("第一句。"), "{log1}");
    let json = a.save().unwrap(); // 暂停于 keep 的选择组

    let mut b = Story::load(&result.program, &result.analysis, &json).unwrap();
    let silence = transcript(&mut b);
    assert!(silence.is_empty(), "重建暂停不应产生新输出:{silence}");
    assert_eq!(b.choices().len(), 2);
    b.choose(1).unwrap();
    let log2 = transcript(&mut b);
    assert!(log2.contains("后半句 B。"), "{log2}");
}

#[test]
fn fingerprint_rejects_changed_program() {
    let r1 = compile_source("a.wl", "event start\n  甲。\n  -> END\n");
    let s = Story::new(&r1.program, &r1.analysis).unwrap();
    let json = s.save().unwrap();
    let r2 = compile_source("a.wl", "event start\n  乙。\n  -> END\n");
    assert!(
        Story::load(&r2.program, &r2.analysis, &json).is_err(),
        "内容变化后旧档应被拒绝"
    );
}

#[test]
fn comment_does_not_break_saves() {
    let r1 = compile_source("a.wl", "event start\n  甲。\n  -> END\n");
    let s = Story::new(&r1.program, &r1.analysis).unwrap();
    let json = s.save().unwrap();
    let r2 = compile_source(
        "a.wl",
        "// 只是加了一条注释\nevent start\n  甲。\n  -> END\n",
    );
    assert!(
        Story::load(&r2.program, &r2.analysis, &json).is_ok(),
        "加注释不应破坏存档"
    );
}

// -- 诊断 -------------------------------------------------------------------

fn codes(source: &str) -> Vec<(String, Severity)> {
    let result = compile_source("d.wl", source);
    result
        .diagnostics
        .iter()
        .map(|d| (d.code.to_string(), d.severity))
        .collect()
}

#[test]
fn diagnostics_unknown_divert_and_var() {
    let c = codes("event start\n  -> nowhere\n  set x = y + 1\n  -> END\n");
    assert!(c.contains(&("A101".into(), Severity::Error)), "{c:?}");
    assert!(c.iter().filter(|(k, _)| k == "A102").count() >= 2, "{c:?}"); // x 与 y 未声明
}

#[test]
fn diagnostics_duplicate_symbols() {
    let c = codes("event start\n  -> END\nevent start\n  -> END\n");
    assert!(c.contains(&("A104".into(), Severity::Error)), "{c:?}");
}

#[test]
fn diagnostics_const_assignment() {
    let c = codes("const K = 1\nevent start\n  set K = 2\n  -> END\n");
    assert!(c.contains(&("A106".into(), Severity::Error)), "{c:?}");
}

#[test]
fn diagnostics_type_mismatch() {
    let c = codes("let a = 1\nevent start\n  if a + \"x\"\n    -> END\n  -> END\n");
    assert!(c.contains(&("A103".into(), Severity::Error)), "{c:?}");
}

#[test]
fn diagnostics_unknown_function() {
    let c = codes("event start\n  你 {foo(1)}\n  -> END\n");
    assert!(c.contains(&("A103".into(), Severity::Error)), "{c:?}");
}

#[test]
fn diagnostics_unreachable_event() {
    let c = codes("event start\n  -> END\n\nevent island\n  -> END\n");
    assert!(c.contains(&("A201".into(), Severity::Warning)), "{c:?}");
}

#[test]
fn diagnostics_missing_end_divert() {
    let c = codes("event start\n  就这样结束。\n");
    assert!(c.contains(&("A202".into(), Severity::Warning)), "{c:?}");
}

#[test]
fn diagnostics_all_cond_group() {
    let c = codes("let flag = false\nevent start\n  choice \"a\" if flag\n    -> END\n  -> END\n");
    assert!(c.contains(&("A203".into(), Severity::Warning)), "{c:?}");
}

#[test]
fn diagnostics_self_loop() {
    let c = codes("event start\n  -> start\n");
    assert!(c.contains(&("A206".into(), Severity::Warning)), "{c:?}");
}

#[test]
fn diagnostics_unused_var() {
    let c = codes("let u = 1\nevent start\n  -> END\n");
    assert!(c.contains(&("A107".into(), Severity::Warning)), "{c:?}");
}

#[test]
fn diagnostics_tab_indent() {
    let c = codes("event start\n\tx\n  -> END\n");
    assert!(c.contains(&("P002".into(), Severity::Error)), "{c:?}");
}

// -- 关系图 -------------------------------------------------------------------

#[test]
fn graph_edges_and_mermaid() {
    let result = compile_source(
        "g.wl",
        "event start\n  choice \"去A\"\n    -> a\n  -> b\n\nevent a\n  -> END\n\nevent b\n  -> END\n",
    );
    let g = &result.analysis.graph;
    assert_eq!(g.nodes.len(), 3);
    // Divert 两条:选择体内的 -> a,以及组后汇聚路径的 -> b;Choice 一条(带标签)
    let diverts = g
        .edges
        .iter()
        .filter(|e| e.kind == worldline_core::EdgeKind::Divert)
        .count();
    let choices = g
        .edges
        .iter()
        .filter(|e| e.kind == worldline_core::EdgeKind::Choice)
        .count();
    assert_eq!(diverts, 2, "{:?}", g.edges);
    assert_eq!(choices, 1);
    let mermaid = g.to_mermaid();
    assert!(mermaid.contains("flowchart TD"));
    assert!(mermaid.contains(".->"), "{mermaid}");
}

#[test]
fn graph_scene_enter_edge() {
    let result = compile_source("g.wl", "event start\n  scene inner\n    x\n    -> END\n");
    let enters = result
        .analysis
        .graph
        .edges
        .iter()
        .filter(|e| e.kind == worldline_core::EdgeKind::Enter)
        .count();
    assert_eq!(enters, 1);
}

// -- include -----------------------------------------------------------------

#[test]
fn include_cycle_detected() {
    let dir = std::env::temp_dir().join("wl_test_cycle");
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.wl");
    let b = dir.join("b.wl");
    std::fs::write(&a, "include \"b.wl\"\nevent start\n  -> END\n").unwrap();
    std::fs::write(&b, "include \"a.wl\"\n").unwrap();
    let result = compile_path(&a).unwrap();
    assert!(
        result.diagnostics.iter().any(|d| d.code == "A105"),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn include_nested_relative_paths() {
    let dir = std::env::temp_dir().join("wl_test_inc");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let main = dir.join("main.wl");
    std::fs::write(&main, "include \"sub/one.wl\"\nevent start\n  -> chapter\n").unwrap();
    std::fs::write(sub.join("one.wl"), "include \"two.wl\"\n").unwrap();
    std::fs::write(
        sub.join("two.wl"),
        "event chapter\n  嵌套成功。\n  -> END\n",
    )
    .unwrap();
    let result = compile_path(&main).unwrap();
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    assert!(result.program.events.iter().any(|e| e.name == "chapter"));
}

// -- v1.5:故事线 / 准入 / 效果 / 锚点 / 漂流 ----------------------------------

#[test]
fn storyline_forest_seq_and_drift_graph() {
    let result = compile_source(
        "t.wl",
        "storyline a as \"甲线\"\n  event start\n    -> x\n  event x\n    -> END\n\nstoryline b as \"乙线\"\n  event b.entry\n    ->> x\n",
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let g = &result.analysis.graph;
    assert_eq!(
        g.storyline_order,
        vec![
            ("a".to_string(), "甲线".to_string()),
            ("b".to_string(), "乙线".to_string())
        ]
    );
    let start = *g.ids.get("start").unwrap();
    let x = *g.ids.get("x").unwrap();
    assert_eq!(g.nodes[start as usize].seq, 1);
    assert_eq!(g.nodes[x as usize].seq, 2);
    assert_eq!(g.nodes[x as usize].storyline, "a");
    assert_eq!(
        g.edges.iter().filter(|e| e.kind == EdgeKind::Drift).count(),
        1
    );
    assert!(g.to_mermaid().contains("==>"), "{:?}", g.to_mermaid());
    let tl = g.to_timeline_mermaid();
    assert!(tl.contains("subgraph"), "{tl}");
    assert!(tl.contains("漂流"), "{tl}");
}

#[test]
fn admission_perm_gate_blocks_and_grant_unlocks() {
    let src = "event start\n  -> gated\n\nevent gated perm key\n  你进来了。\n  -> END\n";
    let result = compile_source("p.wl", src);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let err = s.continue_story().unwrap_err();
    assert!(err.message.contains("权限"), "{err}");

    let src2 =
        "event start\n  grant key\n  -> gated\n\nevent gated perm key\n  你进来了。\n  -> END\n";
    let r2 = compile_source("p.wl", src2);
    let mut s2 = Story::new(&r2.program, &r2.analysis).unwrap();
    let out = transcript(&mut s2);
    assert!(out.contains("你进来了"), "{out}");
    assert!(s2.perm_list().contains(&"key".to_string()));
}

#[test]
fn admission_after_gate_uses_seen() {
    let src = "event start\n  -> gated\n\nevent gated after seen(elsewhere)\n  你进来了。\n  -> END\n\nevent elsewhere\n  -> END\n";
    let result = compile_source("a.wl", src);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let err = s.continue_story().unwrap_err();
    assert!(err.message.contains("前置"), "{err}");
}

#[test]
fn effect_timing_enter_and_done() {
    let src = r#"
event start
  effect on enter
    grant early as "开场获得"
  权限测试:{perm(early)}。
  choice "去终章"
    -> finale
  -> END

event finale
  effect on done
    grant late as "完成时获得"
  正文结束。
"#;
    let result = compile_source("f.wl", src);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let out = transcript(&mut s);
    assert!(out.contains("权限测试:true"), "{out}");
    assert!(s.perm_list().contains(&"early".to_string()));
    s.choose(0).unwrap();
    let out2 = transcript(&mut s);
    assert!(out2.contains("[END]"), "{out2}");
    // on done 在事件体自然结束时触发
    assert!(
        s.perm_list().contains(&"late".to_string()),
        "{:?}",
        s.perm_list()
    );
    assert!(s.is_ended());
}

#[test]
fn done_effect_skipped_on_divert_exit() {
    let src = r#"
event start
  choice "离开"
    -> END

event gated
  effect on done
    grant never as "不应触发"
  你不会执行到这里。
"#;
    let result = compile_source("d2.wl", src);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    transcript(&mut s);
    s.choose(0).unwrap();
    transcript(&mut s);
    assert!(!s.perm_list().contains(&"never".to_string()));
}

#[test]
fn drift_switches_storyline_and_records_anchor() {
    let src = "storyline awake\n  event start\n    ->> dream.entry\n  event wake\n    -> END\n\nstoryline dream\n  event dream.entry\n    梦里。\n    -> END\n";
    let result = compile_source("d.wl", src);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let out = transcript(&mut s);
    assert!(out.contains("梦里"), "{out}");
    assert_eq!(s.storyline(), "dream");
    let drifts: Vec<_> = s
        .anchors()
        .iter()
        .filter(|a| a.kind == AnchorKind::Drift)
        .collect();
    assert_eq!(drifts.len(), 1);
    assert_eq!(drifts[0].detail.as_deref(), Some("dream.entry"));
}

#[test]
fn anchor_statement_records_manual() {
    let src = "event start\n  anchor \"关键转折\" as \"测试说明\"\n  -> END\n";
    let result = compile_source("an.wl", src);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    transcript(&mut s);
    assert_eq!(s.anchors().len(), 1);
    assert_eq!(s.anchors()[0].kind, AnchorKind::Manual);
    assert_eq!(s.anchors()[0].name, "关键转折");
    assert_eq!(s.anchors()[0].note.as_deref(), Some("测试说明"));
    // 分析层也有锚点声明
    assert_eq!(result.analysis.anchors.len(), 1);
    assert_eq!(result.analysis.anchors[0].node, "start");
}

#[test]
fn v15_diagnostics() {
    // A208:with 引用未定义角色
    let c = codes("character a\nevent start with ghost\n  -> END\n");
    assert!(c.contains(&("A208".into(), Severity::Error)), "{c:?}");
    // A210:to 目标故事线不存在
    let c2 = codes("event start\n  effect on enter\n    to nothere\n  -> END\n");
    assert!(c2.contains(&("A210".into(), Severity::Error)), "{c2:?}");
    // A209:同线漂流
    let c3 = codes("storyline a\n  event start\n    ->> start\n");
    assert!(c3.contains(&("A209".into(), Severity::Warning)), "{c3:?}");
}

#[test]
fn save_roundtrip_v15_state() {
    let src = "storyline a\n  event start\n    grant key\n    anchor \"记录点\" as \"存档前\"\n    ->> b.entry\n\nstoryline b\n  event b.entry\n    梦境。\n    choice \"继续\"\n      -> END\n";
    let result = compile_source("sv.wl", src);
    let mut a = Story::new(&result.program, &result.analysis).unwrap();
    transcript(&mut a); // b.entry 暂停
    assert_eq!(a.storyline(), "b");
    let json = a.save().unwrap();
    let mut b = Story::load(&result.program, &result.analysis, &json).unwrap();
    transcript(&mut b);
    assert_eq!(b.storyline(), "b");
    assert!(b.perm_list().contains(&"key".to_string()));
    let manuals: Vec<_> = b
        .anchors()
        .iter()
        .filter(|x| x.kind == AnchorKind::Manual)
        .collect();
    assert_eq!(manuals.len(), 1);
    assert_eq!(manuals[0].name, "记录点");
}

#[test]
fn chronicle_example_full_walkthrough() {
    let result = compile_path(&example("chronicle.wl")).unwrap();
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let mut log = String::new();
    log.push_str(&transcript(&mut s)); // start 暂停
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["敲门", "在门廊睡下"]
    );
    s.choose(0).unwrap(); // 敲门 → hall
    log.push_str(&transcript(&mut s)); // hall 暂停
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["询问宅子的历史", "告辞"]
    );
    s.choose(0).unwrap(); // 询问 → stair → sleep → 漂流入梦
    log.push_str(&transcript(&mut s)); // dream.entry 暂停
    assert_eq!(s.storyline(), "dream");
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["追问密室", "随雾漂流"]
    );
    s.choose(0).unwrap(); // 追问 → dream.door
    log.push_str(&transcript(&mut s)); // dream.door 暂停
    s.choose(0).unwrap(); // 推门而归 → 漂流回清醒世界
    log.push_str(&transcript(&mut s));
    assert!(log.contains("雾凝成的钥匙"), "{log}");
    assert_eq!(s.storyline(), "awake");
    assert!(s.perm_list().contains(&"brave".to_string()));
    assert!(s.met_list().contains(&"keeper".to_string()));
    assert!(
        !s.met_list().contains(&"servant".to_string()),
        "入梦时女仆应离场"
    );
    let manuals: Vec<_> = s
        .anchors()
        .iter()
        .filter(|a| a.kind == AnchorKind::Manual)
        .collect();
    assert_eq!(manuals.len(), 1);
    assert_eq!(manuals[0].name, "听闻密室");
    assert_eq!(
        s.anchors()
            .iter()
            .filter(|a| a.kind == AnchorKind::Drift)
            .count(),
        2
    );
}

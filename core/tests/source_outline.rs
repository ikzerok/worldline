//! 当前源结构：正式声明、可信字节与只读版本守卫。
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{
    project::Project,
    source_outline::{SourceOutline, SourceOutlineStatus as Status},
};
#[path = "source_outline/budgets.rs"]
mod budgets;
#[path = "source_outline/guards.rs"]
mod guards;

struct Fixture {
    root: PathBuf,
    project: Project,
}
impl Fixture {
    fn new(source: &str, version: Option<&str>) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "outline-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("world.wl"), source).unwrap();
        if let Some(version) = version {
            fs::create_dir_all(root.join(".world")).unwrap();
            fs::write(root.join(".world/project.json"), format!(r#"{{"schema_version":1,"language_version":"{version}","required_features":[],"maps":{{}},"graph_views":{{}}}}"#)).unwrap();
        }
        let project = Project::open(&root).unwrap();
        Self { root, project }
    }
    fn outline(&self, source: &str) -> SourceOutline {
        self.project.source_outline(&self.project.entry, source)
    }
    fn ready(&self, source: &str) -> SourceOutline {
        let outline = self.outline(source);
        assert_eq!(outline.status, Status::Ready, "{:?}", outline.message);
        for entry in &outline.entries {
            assert_eq!(
                self.project
                    .source_outline_range(&outline, source, entry.occurrence)
                    .unwrap(),
                entry.header
            );
        }
        outline
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn every_supported_explicit_kind_is_source_ordered_and_aliases_are_not_declarations() {
    let source = r#"world w as "世界"
character c as "人物"
entity e kind place as "地点"
relation_type knows as "认识"
relation_def r type knows from character c to entity e
period era as "时段"
tag t as "标签"
anchor_def a as "锚点"
state s on character c with t as "状态"
asset image image "image.png" as "图像"
let score = 0
const rate = 1
rule twice(n: num) -> num = n * 2
fragment note()
  注记
  return
schema facts for entity
  field title_id title text
bind entity e to facts
alias entity e as "别名"
mark entity e with t
attach entity e with image
anchor_link a entity e
storyline route as "主线"
  let nested = 1
  event start as "开场"
    scene room
      正文
      -> END
"#;
    let fixture = Fixture::new(source, Some("1.13"));
    let outline = fixture.ready(source);
    let kinds: Vec<_> = outline
        .entries
        .iter()
        .map(|entry| entry.kind.as_str())
        .collect();
    assert_eq!(
        kinds,
        [
            "world",
            "character",
            "entity",
            "relation_type",
            "relation",
            "period",
            "tag",
            "anchor",
            "state",
            "asset",
            "let",
            "const",
            "rule",
            "fragment",
            "schema",
            "storyline",
            "let",
            "event",
            "scene"
        ]
    );
    assert_eq!(outline.entries[2].entity_type.as_deref(), Some("place"));
    assert_eq!(outline.entries[15].display, "主线");
    assert_eq!(outline.entries[16].parent, Some(15));
    assert_eq!(outline.entries[17].parent, Some(15));
    assert_eq!(outline.entries[18].parent, Some(17));
    assert_eq!(outline.entries[18].id, "start.room");
    assert!(outline
        .current_item(source.find("alias").unwrap())
        .is_none());
}

#[test]
fn duplicate_ids_titles_storyline_blocks_and_nested_scenes_keep_source_identity() {
    let source = "character same as \"同名\"\ncharacter same as \"同名\"\nstoryline one as \"同名\"\n  event start as \"同名\"\n    scene room\n      scene room\n        正文\n    scene second\n      scene room\n        正文\nstoryline one as \"同名\"\n  event start as \"同名\"\n    -> END\n";
    let fixture = Fixture::new(source, None);
    let outline = fixture.ready(source);
    assert_eq!(outline.entries.len(), 10);
    assert_eq!(outline.entries[0].id, outline.entries[1].id);
    assert_ne!(outline.entries[0].header, outline.entries[1].header);
    assert_eq!(outline.entries[5].id, "start.room.room");
    assert_eq!(outline.entries[7].id, "start.second.room");
    assert_eq!(outline.entries[7].parent, Some(6));
    assert_eq!(outline.entries[9].parent, Some(8));
    assert!(fixture.project.clone().compile().has_errors());
}

#[test]
fn unicode_crlf_comments_and_eof_keep_exact_physical_bytes() {
    let source = "// event fake\r\n/*中文🙂*/ character hero as \"同名🙂\" //尾注🙂\r\nevent start as \"起点🙂\"\r\n  scene room\r\n    中文🙂 /*内注🙂*/ 后文\r\n\r\n// 尾注";
    let fixture = Fixture::new(source, None);
    let outline = fixture.ready(source);
    assert_eq!(
        &source[outline.entries[0].header.clone()],
        "character hero as \"同名🙂\""
    );
    assert_eq!(
        &source[outline.entries[1].header.clone()],
        "event start as \"起点🙂\""
    );
    assert_eq!(
        outline.entries[2].body.end,
        source.find("\r\n\r\n").unwrap()
    );
    let current = outline
        .current_item(source.find("中文🙂 /*").unwrap())
        .unwrap();
    assert_eq!(current.id, "start.room");
    assert!(outline.current_item(source.find("内注").unwrap()).is_none());
    assert!(outline.current_item(source.find("尾注").unwrap()).is_none());
    assert!(outline
        .current_item(source.find("同名🙂").unwrap() + 1)
        .is_none());
    assert!(outline.current_item(source.len()).is_none());
    assert!(outline
        .current_item(source.find("\r\n\r\n").unwrap() + 2)
        .is_none());
}

#[test]
fn strings_escaped_keywords_and_comments_never_generate_false_declarations() {
    let source = "character c as \"event pretend\"\n// scene ghost\n/* fragment f()\nentity wrong kind place\n*/\nevent real\n  \\scene escaped\n  \\event nope\n  文中 entity imaginary\n  -> END\n";
    let fixture = Fixture::new(source, Some("1.13"));
    assert_eq!(
        fixture
            .ready(source)
            .entries
            .iter()
            .map(|entry| entry.kind.as_str())
            .collect::<Vec<_>>(),
        ["character", "event"]
    );
}

#[test]
fn version_gates_remain_default_19_and_explicit_110_through_113() {
    for version in [
        None,
        Some("1.9"),
        Some("1.10"),
        Some("1.11"),
        Some("1.12"),
        Some("1.13"),
    ] {
        let version_number = version.unwrap_or("1.9");
        let mut source = "event start\n  -> END\n".to_string();
        if version_number != "1.9" {
            source.insert_str(0, "entity place kind location\n");
        }
        if matches!(version_number, "1.11" | "1.12" | "1.13") {
            source.insert_str(0, "fragment f()\n  正文\nrule yes() -> bool = true\n");
        }
        if matches!(version_number, "1.12" | "1.13") {
            source.insert_str(0, "schema s for entity\n  field name_id name text\n");
        }
        let fixture = Fixture::new(&source, version);
        let outline = fixture.ready(&source);
        assert_eq!(fixture.project.language_version(), version_number);
        assert_eq!(
            outline.entries.iter().any(|entry| entry.kind == "entity"),
            version_number != "1.9"
        );
    }
    let old = Fixture::new(
        "event start\n  entity old prose\n  rule old prose\n  schema old prose\n",
        None,
    );
    assert_eq!(
        old.ready(old.project.document(&old.project.entry).unwrap())
            .entries
            .len(),
        1
    );
    let gated = Fixture::new("", None);
    assert_eq!(
        gated.outline("entity place kind location\n").status,
        Status::SyntaxInvalid
    );
}

#[test]
fn no_scene_in_fragment_is_advertised_as_a_supported_object() {
    let source = "fragment f()\n  scene forbidden\n    正文\nevent start\n  -> END\n";
    let fixture = Fixture::new(source, Some("1.11"));
    let outline = fixture.ready(source);
    assert_eq!(
        outline
            .entries
            .iter()
            .map(|entry| entry.kind.as_str())
            .collect::<Vec<_>>(),
        ["fragment", "event"]
    );
    assert!(fixture.project.clone().compile().has_errors());
}

#[test]
fn scenes_inside_if_and_choice_have_nearest_declaration_parent() {
    let source = "event e\n  if true\n    scene left\n      正文\n  choice \"选择\"\n    scene right\n      -> END\n";
    let fixture = Fixture::new(source, None);
    let outline = fixture.ready(source);
    assert_eq!(outline.entries.len(), 3);
    assert_eq!(outline.entries[1].parent, Some(0));
    assert_eq!(outline.entries[2].parent, Some(0));
}

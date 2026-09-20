use std::collections::BTreeSet;
use std::fs;

use worldline_core::catalog::TargetRef;
use worldline_core::compile_source;
use worldline_core::deletion_content_references::content_deletion_references;
use worldline_core::project::Project;

fn target_event() -> TargetRef {
    TargetRef::new("event", "target")
}

#[test]
fn deletion_references_cover_external_v19_sources() {
    let source = r#"
tag label as "标签"
asset art file "art.txt" as "资料"
anchor_def clue as "线索"
alias event target as "目标别名"
mark event target with label
attach event target with art
anchor_link clue event target
state target_state on event target with label as "目标状态"
period chapter as "章节"

event target during chapter as "目标"
  if seen("target") and visits("target") > 0
    目标内部条件。
  目标正文。
  -> END

event source during chapter follows target after seen("target") and visits("target") > 0
  [[event:target|目标链接]]
  -> target
"#;
    let result = compile_source("world.wl", source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);

    let references = content_deletion_references(&result, &target_event());
    let kinds: BTreeSet<_> = references
        .iter()
        .map(|reference| reference.kind.as_str())
        .collect();

    assert!(kinds.contains("别名引用"));
    assert!(kinds.contains("标签标记"));
    assert!(kinds.contains("素材附件"));
    assert!(kinds.contains("锚点关联"));
    assert!(kinds.contains("状态定义"));
    assert!(kinds.contains("正文链接"));
    assert!(kinds.contains("叙事连接"));
    assert!(kinds.contains("先后约束"));
    assert!(kinds.contains("条件引用"));

    assert!(references.iter().any(|reference| {
        reference.kind == "条件引用" && reference.source == TargetRef::new("event", "source")
    }));
    assert!(references
        .iter()
        .all(|reference| { reference.source != TargetRef::new("event", "target") }));

    let tag_references = content_deletion_references(&result, &TargetRef::new("tag", "label"));
    assert!(tag_references.iter().any(|reference| {
        reference.source == TargetRef::new("file", "world.wl")
            && reference.target == TargetRef::new("tag", "label")
            && reference.kind == "标签引用"
    }));
    let asset_references = content_deletion_references(&result, &TargetRef::new("asset", "art"));
    assert!(asset_references.iter().any(|reference| {
        reference.source == TargetRef::new("file", "world.wl")
            && reference.target == TargetRef::new("asset", "art")
            && reference.kind == "素材引用"
    }));
}

#[test]
fn deletion_references_resolve_scene_calls_and_skip_target_event_body() {
    let source = r#"
event target as "目标"
  if seen("target.inner") and visits("target.inner") > 0
    目标内部条件。 #内部标签
  scene inner
    场景正文。
  -> END

event source after seen("target.inner") and visits("target.inner") > 0
  -> target
"#;
    let result = compile_source("world.wl", source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);

    let references = content_deletion_references(&result, &target_event());
    assert!(references.iter().any(|reference| {
        reference.kind == "条件引用"
            && reference.source == TargetRef::new("event", "source")
            && reference.target == TargetRef::new("scene", "target.inner")
    }));
    assert!(references
        .iter()
        .all(|reference| { reference.source != TargetRef::new("event", "target") }));
    assert!(!references
        .iter()
        .any(|reference| reference.kind == "标签引用"));
}

#[test]
fn deletion_references_cover_state_tag_and_variable_conditions() {
    let source = r#"
world setting as "世界"
tag gate as "门槛"
state access on world setting with gate as "访问状态"
let score = 1

event target as "目标"
  -> END

event source after has(access, gate) and score > 0
  set score = score + 1
  -> target
"#;
    let result = compile_source("world.wl", source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);

    let state_references = content_deletion_references(&result, &TargetRef::new("state", "access"));
    assert!(state_references.iter().any(|reference| {
        reference.source == TargetRef::new("event", "source")
            && reference.target == TargetRef::new("state", "access")
            && reference.kind == "条件引用"
    }));

    let tag_references = content_deletion_references(&result, &TargetRef::new("tag", "gate"));
    assert!(tag_references.iter().any(|reference| {
        reference.source == TargetRef::new("event", "source")
            && reference.target == TargetRef::new("tag", "gate")
            && reference.kind == "条件引用"
    }));

    let variable_references =
        content_deletion_references(&result, &TargetRef::new("variable", "score"));
    assert!(variable_references.iter().any(|reference| {
        reference.source == TargetRef::new("event", "source")
            && reference.target == TargetRef::new("variable", "score")
            && reference.kind == "条件引用"
    }));
}

fn assert_remove_event_blocked(label: &str, source: &str, expected_kind: &str) {
    let root = std::env::temp_dir().join(format!(
        "worldline-language-impact-{}-{label}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    let mut project = Project::open(&root.join("world.wl")).unwrap();
    let before = project.sources();

    let error = project.remove_event("target").unwrap_err();
    assert!(error.contains(expected_kind), "{error}");
    assert_eq!(project.sources(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn remove_event_blocks_alias_mark_and_condition_references_independently() {
    assert_remove_event_blocked(
        "alias",
        r#"
event target as "目标"
  -> END
alias event target as "目标别名"
"#,
        "别名引用",
    );
    assert_remove_event_blocked(
        "mark",
        r#"
tag label as "标签"
event target as "目标"
  -> END
mark event target with label
"#,
        "标签标记",
    );
    assert_remove_event_blocked(
        "seen",
        r#"
event target as "目标"
  -> END
event source after seen("target")
  -> END
"#,
        "条件引用",
    );
}

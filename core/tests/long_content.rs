use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::ast::PropertyValue;
use worldline_core::authoring::EntityDraft;
use worldline_core::project::Project;
use worldline_core::TargetRef;

fn root(name: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "worldline-long-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn long_unicode_template_content_aliases_and_custom_fields_roundtrip_exactly() {
    let root = root("roundtrip");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), "").unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1"],"maps":{},"graph_views":{}}"#,
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    let paragraph = "雾港潮声。第二句含“引号”、\\反斜杠、问号与 emoji 🌊。\n";
    let description = (0..1400)
        .map(|index| format!("{index:04}：{paragraph}"))
        .collect::<String>();
    let custom = format!(
        "未知栏目仍应保留。\n{}\n尾行不丢失",
        "甲乙丙\"\\\\ ".repeat(1200)
    );
    let draft = EntityDraft {
        id: "archive".into(),
        entity_type: "history".into(),
        display: "灯塔熄灭长文记录".into(),
        description: description.clone(),
        properties: vec![
            ("history_1".into(), PropertyValue::Str("事件概述".into())),
            ("custom_unknown".into(), PropertyValue::Str(custom.clone())),
        ],
    };
    project
        .write_entity(&root.join("world.wl"), None, &draft)
        .unwrap();
    project
        .set_aliases(
            &TargetRef::new("entity", "archive"),
            &["旧档案".into(), "The Long Record".into()],
        )
        .unwrap();
    let first = project.compile();
    assert!(!first.has_errors(), "{:?}", first.diagnostics);
    let entity = &first.analysis.catalog.entities["archive"];
    assert_eq!(entity.description, description);
    assert_eq!(
        entity.properties["custom_unknown"],
        PropertyValue::Str(custom.clone())
    );
    assert_eq!(
        first
            .analysis
            .catalog
            .aliases_for(&TargetRef::new("entity", "archive")),
        vec!["The Long Record".to_string(), "旧档案".to_string()]
    );

    let mut switched = draft.clone();
    switched.entity_type = "narrative".into();
    project
        .write_entity(&root.join("world.wl"), Some("archive"), &switched)
        .unwrap();
    let second = project.compile();
    let entity = &second.analysis.catalog.entities["archive"];
    assert_eq!(entity.description, description);
    assert_eq!(
        entity.properties["custom_unknown"],
        PropertyValue::Str(custom.clone())
    );

    project.save().unwrap();
    let mut reopened = Project::open(&root).unwrap();
    let third = reopened.compile();
    assert!(!third.has_errors(), "{:?}", third.diagnostics);
    let entity = &third.analysis.catalog.entities["archive"];
    assert_eq!(entity.description, description);
    assert_eq!(
        entity.properties["custom_unknown"],
        PropertyValue::Str(custom)
    );
    assert_eq!(
        third
            .analysis
            .catalog
            .aliases_for(&TargetRef::new("entity", "archive")),
        vec!["The Long Record".to_string(), "旧档案".to_string()]
    );
    let _ = fs::remove_dir_all(root);
}

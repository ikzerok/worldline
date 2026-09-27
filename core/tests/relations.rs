use std::fs;
use std::path::PathBuf;

pub(crate) fn relation_source(extra: &str) -> String {
    format!(
        r#"entity keepers kind organization as "守灯会"
entity lighthouse kind place as "灯塔"
entity harbor kind place as "港口"
period modern as "现代"
relation_type maintains as "维护"
  inverse "由其维护"
  direction directed
  from entity
  to entity
relation_def rel_1 type maintains from entity keepers to entity lighthouse
  description "守灯会维护灯塔。"
  source_note "设定稿"
  scope period modern
relation_def rel_2 type maintains from entity keepers to entity lighthouse
  description "守灯会也负责灯塔巡检。"
relation_def rel_3 type maintains from entity lighthouse to entity harbor
{extra}
"#
    )
}

pub(crate) fn project_root(name: &str, source: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("worldline-relations-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"language_version":"1.10","required_features":["content.relations.v1"]}"#,
    )
    .unwrap();
    root
}

#[path = "relations/catalog.rs"]
mod catalog;
#[path = "relations/editor.rs"]
mod editor;
#[path = "relations/query.rs"]
mod query;
#[path = "relations/topic_projection.rs"]
mod topic_projection;

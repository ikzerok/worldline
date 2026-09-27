use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::presentation_commands::Revision;
use worldline_core::project::Project;
use worldline_core::project_templates::{
    ProjectTemplateMutation, ProjectTemplateValueState, TemplateCommand,
};
#[path = "project_templates/lifecycle.rs"]
mod lifecycle;
#[path = "project_templates/object_refs.rs"]
mod object_refs;

fn root(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "worldline-project-templates-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn project(name: &str) -> Project {
    let root = root(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(
        root.join("world.wl"),
        concat!(
            "world setting as \"雾港\"\n",
            "  property motto = \"keep this prose\"\n",
            "entity harbor kind place as \"港口\"\n",
            "  description \"不可由模板重写的长正文。\"\n",
            "  property note = \"custom instance value\"\n",
            "  property status = \"active\"\n",
            "  property empty_note = \"\"\n",
            "character navigator as \"领航员\"\n",
            "  property home = \"user-owned alias-like value\"\n",
            "event arrival as \"抵达\"\n",
            "  -> END\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"project_id":"harbor","language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1","content.templates.v1"],"templates":{},"extension":{"keep":"manifest"}}"#,
    )
    .unwrap();
    Project::open(&root).unwrap()
}

fn project_with_object_refs(
    name: &str,
    language: &str,
    enable_refs: bool,
    source: &str,
) -> Project {
    let root = root(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    let mut features = vec!["content.entities.v1"];
    if enable_refs {
        features.push("content.object_refs.v1");
    }
    let features = serde_json::to_string(&features).unwrap();
    fs::write(
        root.join(".world/project.json"),
        format!(
            r#"{{"schema_version":1,"project_id":"harbor","language_version":"{language}","entry":"world.wl","required_features":{features},"templates":{{}}}}"#
        ),
    )
    .unwrap();
    Project::open(&root).unwrap()
}

fn template(id: &str, note_type: &str, unknown: &str) -> Vec<u8> {
    format!(
        r#"{{
  "schema_version": 1,
  "id": "{id}",
  "title": "港口档案",
  "applies_to": {{"kind":"entity", "entity_type":"place"}},
  "fields": [
    {{"id":"note_field", "key":"note", "label":"记录", "type":"{note_type}", "required":false, "{unknown}":{{"keep":true}}}},
    {{"id":"status_field", "key":"status", "label":"状态", "type":"enum", "required":false, "choices":["active","closed"], "default":"active"}},
    {{"id":"empty_field", "key":"empty_note", "label":"空记录", "type":"text", "required":false}},
    {{"id":"missing_field", "key":"not_present", "label":"尚未填写", "type":"text", "required":false}}
  ],
  "extensions": {{"vendor":"preserve me"}}
}}"#
    )
    .into_bytes()
}

fn command(
    project: &Project,
    revision: Revision,
    mutation: ProjectTemplateMutation,
) -> TemplateCommand {
    TemplateCommand {
        expected_revision: revision,
        expected_baseline: project.content_baseline(),
        mutation,
        check_integrity: true,
    }
}

fn import(id: &str, value: Vec<u8>) -> ProjectTemplateMutation {
    ProjectTemplateMutation::Import {
        id: id.into(),
        document: value,
    }
}

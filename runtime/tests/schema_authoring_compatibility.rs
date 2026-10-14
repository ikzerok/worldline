//! schema 表单不改变旧指纹算法；真正改变指纹的资料修改必须如实报告。
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::{
    ast::PropertyValue,
    catalog::TargetRef,
    project::Project,
    schemas::{SchemaAuthoringRequest, SchemaBindingMutation, SchemaFieldMutation},
};
use worldline_runtime::Story;

const SOURCE: &str = r#"schema world_data for world
  field value_id value number required
schema person_data for character
  field value_id value number required
schema entity_data for entity entity_type place
  field value_id value number required
schema relation_data for relation
  field value_id value number required
world shared as "世界"
  property value = 0
character shared as "人物"
  property value = 0
entity shared kind place as "地点"
  property value = 0
relation_type knows as "认识"
relation_def shared type knows from character shared to entity shared
  property value = 0
event start with shared
  正文。
  choice "继续"
    -> END
"#;

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "schema-runtime-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join(".world")).unwrap();
        fs::write(root.join("world.wl"), SOURCE).unwrap();
        fs::write(root.join(".world/project.json"),r#"{"schema_version":1,"language_version":"1.13","required_features":["content.relations.v1","content.object_refs.v1","content.character_refs.v1"]}"#).unwrap();
        Self(root)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn four_kinds_bind_without_invalidating_save_then_scalar_edits_follow_existing_hash() {
    for (kind, schema, changes_runtime) in [
        ("world", "world_data", true),
        ("character", "person_data", true),
        ("entity", "entity_data", false),
        ("relation", "relation_data", false),
    ] {
        let workspace = Workspace::new();
        let mut project = Project::open(&workspace.0).unwrap();
        let before = project.compile();
        assert!(!before.has_errors(), "{:?}", before.diagnostics);
        let mut story = Story::new_with_seed(&before.program, &before.analysis, 17).unwrap();
        story.continue_story().unwrap();
        let save = story.save().unwrap();
        let checkpoint = story.checkpoint().unwrap();
        let state = story.state_view();
        let mut request = SchemaAuthoringRequest {
            schema_version: 1,
            expected_baseline: project.content_baseline(),
            target: TargetRef::new(kind, "shared"),
            binding: SchemaBindingMutation::Bind {
                schema_id: schema.into(),
            },
            fields: vec![],
        };
        let binding = project.preview_schema_authoring(&request).unwrap();
        assert!(!binding.runtime_changed, "binding {kind}");
        assert_eq!(binding.before_fingerprint, binding.after_fingerprint);
        project
            .apply_schema_authoring(&request, &binding.plan_digest)
            .unwrap();
        let bound = project.compile();
        assert_eq!(bound.analysis.fingerprint, before.analysis.fingerprint);
        assert_eq!(
            Story::load(&bound.program, &bound.analysis, &save)
                .unwrap()
                .state_view(),
            state
        );
        request.expected_baseline = project.content_baseline();
        request.binding = SchemaBindingMutation::Keep;
        request.fields = vec![SchemaFieldMutation::Set {
            field_id: "value_id".into(),
            key: "value".into(),
            value: PropertyValue::Num(1.0),
        }];
        let plan = project.preview_schema_authoring(&request).unwrap();
        assert_eq!(plan.runtime_changed, changes_runtime, "field {kind}");
        assert_eq!(
            plan.before_fingerprint,
            bound.analysis.fingerprint.to_string()
        );
        project
            .apply_schema_authoring(&request, &plan.plan_digest)
            .unwrap();
        let edited = project.compile();
        assert_eq!(
            plan.after_fingerprint,
            edited.analysis.fingerprint.to_string()
        );
        if changes_runtime {
            assert!(
                Story::load(&edited.program, &edited.analysis, &save).is_err(),
                "{kind}"
            );
            assert!(
                Story::from_checkpoint(&edited.program, &edited.analysis, &checkpoint).is_err(),
                "{kind}"
            );
        } else {
            assert_eq!(
                Story::load(&edited.program, &edited.analysis, &save)
                    .unwrap()
                    .state_view(),
                state
            );
            assert_eq!(
                Story::from_checkpoint(&edited.program, &edited.analysis, &checkpoint)
                    .unwrap()
                    .state_view(),
                state
            );
        }
        project.save().unwrap();
        let mut reopened = Project::open(&workspace.0).unwrap();
        assert_eq!(
            reopened.compile().analysis.fingerprint,
            edited.analysis.fingerprint
        );
    }
}

#[test]
fn unbinding_keeps_scalar_source_and_save_without_weakening_story_content_guards() {
    let workspace = Workspace::new();
    let mut project = Project::open(&workspace.0).unwrap();
    let path = project.entry.clone();
    project
        .set_text(&path, format!("{SOURCE}bind world shared to world_data\n"))
        .unwrap();
    let before = project.compile();
    let mut story = Story::new_with_seed(&before.program, &before.analysis, 17).unwrap();
    story.continue_story().unwrap();
    let save = story.save().unwrap();
    let request = SchemaAuthoringRequest {
        schema_version: 1,
        expected_baseline: project.content_baseline(),
        target: TargetRef::new("world", "shared"),
        binding: SchemaBindingMutation::Unbind,
        fields: vec![],
    };
    let preview = project.preview_schema_authoring(&request).unwrap();
    assert!(!preview.runtime_changed);
    project
        .apply_schema_authoring(&request, &preview.plan_digest)
        .unwrap();
    let after = project.compile();
    assert!(Story::load(&after.program, &after.analysis, &save).is_ok());
    assert!(project
        .document(&path)
        .unwrap()
        .contains("property value = 0"));
    let changed = project
        .document(&path)
        .unwrap()
        .replace("正文。", "不同正文。");
    project.set_text(&path, changed).unwrap();
    let changed = project.compile();
    assert!(Story::load(&changed.program, &changed.analysis, &save).is_err());
}

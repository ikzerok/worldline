use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::presentation_commands::Revision;
use worldline_core::presentation_presets::{
    self, PresentationPresetDraft, PresetCommand, PresetGeometryRef,
};
use worldline_core::project::Project;
use worldline_core::TargetRef;

fn root(name: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "worldline-preset-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn project(name: &str) -> Project {
    let root = root(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(
        root.join("world.wl"),
        "period old as \"旧纪元\"\nentity place kind place as \"地点\"\n",
    )
    .unwrap();
    fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1","presentation.maps.v1","presentation.geometry.line_area.v1"],"maps":{"city":".world/maps/city.json"},"graph_views":{}}"#).unwrap();
    fs::write(root.join(".world/maps/city.json"), r#"{
      "schema_version":1,"id":"city","title":"城市","raster_layers":[],
      "canvas":{"width":1000,"height":800,"unit":"normalized"},
      "layer_order":["routes","areas"],
      "layers":{"routes":{"title":"路径","visible_default":true,"locked":false},"areas":{"title":"分布区","visible_default":true,"locked":false}},
      "placements":{
        "route":{"layer_id":"routes","target_ref":null,"geometry":{"kind":"polyline","points":[[0.1,0.1],[0.9,0.9]]},"annotation":"旧路","role":"路径说明","scope_refs":[]},
        "district":{"layer_id":"areas","target_ref":null,"geometry":{"kind":"polygon","points":[[0.1,0.1],[0.5,0.1],[0.4,0.5]]},"annotation":"港区","role":"分布区","scope_refs":[]}
      },"extensions":{}
    }"#).unwrap();
    Project::open(&root).unwrap()
}

#[test]
fn preset_saves_only_display_choices_and_preserves_content_and_geometry() {
    let mut project = project("roundtrip");
    let before_sources = project.sources();
    let before_fingerprint = project.compile().analysis.fingerprint;
    let before_map = project
        .authoring_document(&project.root.join(".world/maps/city.json"))
        .unwrap()
        .bytes()
        .to_vec();
    let mut revision = Revision::default();
    let draft = PresentationPresetDraft {
        id: "old_city".into(),
        title: "旧纪元城市".into(),
        map_id: Some("city".into()),
        graph_view_id: None,
        layer_visibility: BTreeMap::from([("routes".into(), true), ("areas".into(), false)]),
        scope_refs: vec![TargetRef::new("period", "old")],
        include_unscoped: false,
        include_period_children: false,
        geometry_refs: vec![
            PresetGeometryRef {
                placement_id: "route".into(),
                purpose: "path".into(),
                note: "旧路展示".into(),
            },
            PresetGeometryRef {
                placement_id: "district".into(),
                purpose: "distribution".into(),
                note: "港区范围展示".into(),
            },
        ],
    };
    let command = PresetCommand {
        expected_revision: revision,
        expected_baseline: project.content_baseline(),
        original: None,
        draft: draft.clone(),
    };
    let result = presentation_presets::apply(&mut project, &mut revision, command).unwrap();
    assert_eq!(result.changed_files.len(), 2);
    assert_eq!(project.sources(), before_sources);
    assert_eq!(project.compile().analysis.fingerprint, before_fingerprint);
    assert_eq!(
        project
            .authoring_document(&project.root.join(".world/maps/city.json"))
            .unwrap()
            .bytes(),
        before_map
    );
    let content = project.compile();
    let maps = worldline_core::presentation_commands::map_index_with_content(&project, &content);
    let graphs = worldline_core::graph_views::build_graph_view_index(&project, &content);
    let index = presentation_presets::build_preset_index(&project, &content, &maps, &graphs);
    assert_eq!(index.presets["old_city"].draft, draft);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
}

#[test]
fn preset_rejects_geometry_purpose_mismatch_and_stale_baseline_without_writes() {
    let mut project = project("invalid");
    let baseline = project.content_baseline();
    let mut revision = Revision::default();
    let mut draft = PresentationPresetDraft {
        id: "bad".into(),
        title: "错误".into(),
        map_id: Some("city".into()),
        graph_view_id: None,
        layer_visibility: BTreeMap::new(),
        scope_refs: vec![],
        include_unscoped: false,
        include_period_children: false,
        geometry_refs: vec![PresetGeometryRef {
            placement_id: "district".into(),
            purpose: "path".into(),
            note: String::new(),
        }],
    };
    let expected_revision = revision;
    let failed = presentation_presets::apply(
        &mut project,
        &mut revision,
        PresetCommand {
            expected_revision,
            expected_baseline: baseline.clone(),
            original: None,
            draft: draft.clone(),
        },
    );
    assert!(failed.is_err());
    assert_eq!(project.content_baseline(), baseline);
    draft.geometry_refs.clear();
    let entry = project.entry.clone();
    project
        .set_text(
            &entry,
            "period old as \"旧纪元\"\nentity changed kind place\n".into(),
        )
        .unwrap();
    let changed = project.content_baseline();
    assert_ne!(changed, baseline);
    let expected_revision = revision;
    assert!(presentation_presets::apply(
        &mut project,
        &mut revision,
        PresetCommand {
            expected_revision,
            expected_baseline: baseline,
            original: None,
            draft
        }
    )
    .is_err());
    assert_eq!(project.content_baseline(), changed);
}

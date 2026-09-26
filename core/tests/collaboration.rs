use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::collaboration::{
    self, AnchorStatus, ApplyProposalCommand, CommentAnchor, CommentCommand, CommentDraft,
    ProposalCommand, ProposalDraft, ProposalFileChange, ProposalStatus,
};
use worldline_core::presentation_commands::Revision;
use worldline_core::project::Project;
use worldline_core::TargetRef;

fn root(name: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "worldline-collab-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn map_json(p1_x: i64, p2_y: i64, layer_order: &[&str], include_p1: bool) -> String {
    let mut placements = serde_json::Map::new();
    if include_p1 {
        placements.insert(
            "p1".into(),
            serde_json::json!({
                "layer_id":"a","target_ref":{"kind":"entity","id":"a"},
                "geometry":{"kind":"point","position":[0.1,0.1]},
                "annotation":"","role":"资料入口","scope_refs":[],"x":p1_x
            }),
        );
    }
    placements.insert(
        "p2".into(),
        serde_json::json!({
            "layer_id":"b","target_ref":{"kind":"entity","id":"b"},
            "geometry":{"kind":"point","position":[0.2,0.2]},
            "annotation":"","role":"资料入口","scope_refs":[],"y":p2_y
        }),
    );
    serde_json::to_string_pretty(&serde_json::json!({
        "schema_version":1,"id":"city","title":"城市","raster_layers":[],
        "canvas":{"width":1000,"height":800,"unit":"normalized"},
        "layer_order":layer_order,
        "layers":{
            "a":{"title":"A","visible_default":true,"locked":false},
            "b":{"title":"B","visible_default":true,"locked":false}
        },
        "placements":placements,"extensions":{}
    }))
    .unwrap()
}

fn project(name: &str) -> Project {
    let root = root(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(
        root.join("world.wl"),
        "entity a kind place as \"甲\"\nentity b kind place as \"乙\"\nrelation_type knows as \"认识\"\nrelation_def rel type knows from entity a to entity b\n",
    )
    .unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{
          "schema_version":1,"language_version":"1.10","entry":"world.wl",
          "required_features":["content.entities.v1","content.relations.v1","presentation.maps.v1"],
          "maps":{"city":".world/maps/city.json"},"graph_views":{},"presets":{}
        }"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/city.json"),
        map_json(0, 0, &["a", "b"], true),
    )
    .unwrap();
    Project::open(&root).unwrap()
}

fn proposal(id: &str, base: String, proposed: String) -> ProposalDraft {
    ProposalDraft {
        id: id.into(),
        author: "作者甲".into(),
        reason: "需要明确审阅的改动".into(),
        status: ProposalStatus::Open,
        changes: vec![ProposalFileChange {
            path: ".world/maps/city.json".into(),
            domain: "presentation".into(),
            base: Some(base),
            proposed: Some(proposed),
        }],
    }
}

#[test]
fn comments_keep_object_marker_and_text_anchors_without_guessing_reanchors() {
    let mut project = project("comments");
    let mut revision = Revision::default();
    let before_sources = project.sources();
    let before_fingerprint = project.compile().analysis.fingerprint;

    let object = CommentDraft {
        id: "object_note".into(),
        author: "甲".into(),
        body: "对象意见".into(),
        anchor: CommentAnchor::Object {
            target: TargetRef::new("relation", "rel"),
        },
        resolved: false,
    };
    let expected_revision = revision;
    let baseline = project.content_baseline();
    collaboration::write_comment(
        &mut project,
        &mut revision,
        CommentCommand {
            expected_revision,
            expected_baseline: baseline,
            original: None,
            draft: object,
        },
    )
    .unwrap();
    let content = project.compile();
    let maps = worldline_core::presentation_commands::map_index_with_content(&project, &content);
    assert!(
        maps.maps
            .get("city")
            .is_some_and(|map| map.placements.contains_key("p1")),
        "{:?}",
        maps.diagnostics
    );
    let marker = CommentDraft {
        id: "marker_note".into(),
        author: "乙".into(),
        body: "标记位置意见".into(),
        anchor: CommentAnchor::MapPlacement {
            map_id: "city".into(),
            placement_id: "p1".into(),
        },
        resolved: false,
    };
    let expected_revision = revision;
    let baseline = project.content_baseline();
    collaboration::write_comment(
        &mut project,
        &mut revision,
        CommentCommand {
            expected_revision,
            expected_baseline: baseline,
            original: None,
            draft: marker,
        },
    )
    .unwrap();

    let entry = project.entry.clone();
    let text_anchor = collaboration::capture_text_anchor(&project, &entry, 1, 1).unwrap();
    let text_note = CommentDraft {
        id: "text_note".into(),
        author: "丙".into(),
        body: "正文措辞意见".into(),
        anchor: text_anchor,
        resolved: false,
    };
    let expected_revision = revision;
    let baseline = project.content_baseline();
    collaboration::write_comment(
        &mut project,
        &mut revision,
        CommentCommand {
            expected_revision,
            expected_baseline: baseline,
            original: None,
            draft: text_note,
        },
    )
    .unwrap();

    let content = project.compile();
    let maps = worldline_core::presentation_commands::map_index_with_content(&project, &content);
    let index = collaboration::build_comment_index(&project, &content, &maps);
    assert_eq!(index.comments.len(), 3);
    assert!(index
        .comments
        .values()
        .all(|comment| comment.anchor_status == AnchorStatus::Attached));
    assert_eq!(project.sources(), before_sources);
    assert_eq!(project.compile().analysis.fingerprint, before_fingerprint);
    let impact = project.deletion_impact(&TargetRef::new("relation", "rel"));
    assert_eq!(impact.comments.len(), 1);
    assert_eq!(impact.comments[0].comment_id, "object_note");
    let baseline_before_delete = project.content_baseline();
    let error = project.remove_relation("rel").unwrap_err();
    assert!(error.contains("批注"), "{error}");
    assert_eq!(project.content_baseline(), baseline_before_delete);

    let mut changed = project.document(&entry).unwrap().to_string();
    changed = changed.replacen(
        "entity a kind place as \"甲\"",
        "entity a kind place as \"甲改\"",
        1,
    );
    project.set_text(&entry, changed).unwrap();
    let content = project.compile();
    let maps = worldline_core::presentation_commands::map_index_with_content(&project, &content);
    let index = collaboration::build_comment_index(&project, &content, &maps);
    assert_eq!(
        index.comments["text_note"].anchor_status,
        AnchorStatus::Detached
    );
    assert_eq!(
        index.comments["object_note"].anchor_status,
        AnchorStatus::Attached
    );

    let mut reassigned = index.comments["text_note"].draft.clone();
    reassigned.anchor = collaboration::capture_text_anchor(&project, &entry, 1, 1).unwrap();
    let expected_revision = revision;
    let baseline = project.content_baseline();
    collaboration::write_comment(
        &mut project,
        &mut revision,
        CommentCommand {
            expected_revision,
            expected_baseline: baseline,
            original: Some("text_note".into()),
            draft: reassigned,
        },
    )
    .unwrap();
    let content = project.compile();
    let maps = worldline_core::presentation_commands::map_index_with_content(&project, &content);
    let index = collaboration::build_comment_index(&project, &content, &maps);
    assert_eq!(
        index.comments["text_note"].anchor_status,
        AnchorStatus::Attached
    );
}

#[test]
fn proposal_merges_different_json_object_keys_and_marks_it_accepted() {
    let mut project = project("merge");
    let map_path = project.root.join(".world/maps/city.json");
    let base = String::from_utf8(
        project
            .authoring_document(&map_path)
            .unwrap()
            .bytes()
            .to_vec(),
    )
    .unwrap();
    let draft = proposal("merge_ok", base.clone(), map_json(10, 0, &["a", "b"], true));
    let mut revision = Revision::default();
    let expected_revision = revision;
    let baseline = project.content_baseline();
    collaboration::write_proposal(
        &mut project,
        &mut revision,
        ProposalCommand {
            expected_revision,
            expected_baseline: baseline,
            draft,
        },
    )
    .unwrap();

    project
        .set_authoring_document(&map_path, map_json(0, 20, &["a", "b"], true).into_bytes())
        .unwrap();
    let indexed = collaboration::build_proposal_index(&project);
    let stored = &indexed.proposals["merge_ok"].draft;
    let preview = collaboration::preview_proposal(&project, stored).unwrap();
    assert!(preview.can_apply(), "{:?}", preview.conflicts);
    assert_eq!(preview.presentation_files(), 1);
    assert_eq!(preview.content_files(), 0);

    let expected_revision = revision;
    collaboration::apply_proposal(
        &mut project,
        &mut revision,
        ApplyProposalCommand {
            expected_revision,
            proposal_id: "merge_ok".into(),
        },
    )
    .unwrap();

    let merged: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&map_path).unwrap().bytes()).unwrap();
    assert_eq!(merged["placements"]["p1"]["x"], 10);
    assert_eq!(merged["placements"]["p2"]["y"], 20);
    let indexed = collaboration::build_proposal_index(&project);
    assert_eq!(
        indexed.proposals["merge_ok"].draft.status,
        ProposalStatus::Accepted
    );
}

#[test]
fn proposal_conflicts_on_same_field_delete_modify_and_array_reorder() {
    let cases = [
        (
            "same_field",
            map_json(10, 0, &["a", "b"], true),
            map_json(20, 0, &["a", "b"], true),
            "同一字段",
        ),
        (
            "delete_modify",
            map_json(0, 0, &["a", "b"], false),
            map_json(20, 0, &["a", "b"], true),
            "删除与修改",
        ),
        (
            "layer_order",
            map_json(0, 0, &["a"], true),
            map_json(0, 0, &["b", "a"], true),
            "数组",
        ),
    ];
    for (name, proposed, current, expected) in cases {
        let mut project = project(name);
        let path = project.root.join(".world/maps/city.json");
        let base =
            String::from_utf8(project.authoring_document(&path).unwrap().bytes().to_vec()).unwrap();
        let draft = proposal(name, base, proposed);
        let mut revision = Revision::default();
        let expected_revision = revision;
        let baseline = project.content_baseline();
        collaboration::write_proposal(
            &mut project,
            &mut revision,
            ProposalCommand {
                expected_revision,
                expected_baseline: baseline,
                draft,
            },
        )
        .unwrap();
        project
            .set_authoring_document(&path, current.into_bytes())
            .unwrap();
        let indexed = collaboration::build_proposal_index(&project);
        let preview =
            collaboration::preview_proposal(&project, &indexed.proposals[name].draft).unwrap();
        assert!(!preview.can_apply(), "{name}");
        assert!(
            preview
                .conflicts
                .iter()
                .any(|conflict| conflict.message.contains(expected)),
            "{name}: {:?}",
            preview.conflicts
        );
        let before_apply = project.content_baseline();
        let expected_revision = revision;
        assert!(collaboration::apply_proposal(
            &mut project,
            &mut revision,
            ApplyProposalCommand {
                expected_revision,
                proposal_id: name.into(),
            },
        )
        .is_err());
        assert_eq!(project.content_baseline(), before_apply);
    }
}

#[test]
fn capture_dirty_proposal_separates_content_and_presentation_and_skips_registry_bookkeeping() {
    let mut project = project("capture");
    let entry = project.entry.clone();
    let map_path = project.root.join(".world/maps/city.json");
    let mut source = project.document(&entry).unwrap().to_string();
    source.push_str("entity c kind place as \"丙\"\n");
    project.set_text(&entry, source).unwrap();
    project
        .set_authoring_document(&map_path, map_json(30, 0, &["a", "b"], true).into_bytes())
        .unwrap();

    let captured =
        collaboration::capture_dirty_proposal(&project, "draft_review", "甲", "内容与版式分开审阅")
            .unwrap();
    assert_eq!(captured.status, ProposalStatus::Open);
    assert_eq!(captured.changes.len(), 2);
    assert!(captured
        .changes
        .iter()
        .any(|change| change.domain == "content" && change.path == "world.wl"));
    assert!(captured.changes.iter().any(|change| {
        change.domain == "presentation" && change.path == ".world/maps/city.json"
    }));
    assert!(!captured
        .changes
        .iter()
        .any(|change| change.path == ".world/project.json"));
}

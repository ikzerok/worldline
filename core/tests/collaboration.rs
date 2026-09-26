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

fn content_proposal(id: &str, path: &str, base: &str, proposed: &str) -> ProposalDraft {
    ProposalDraft {
        id: id.into(),
        author: "作者甲".into(),
        reason: "需要明确审阅的改动".into(),
        status: ProposalStatus::Open,
        changes: vec![ProposalFileChange {
            path: path.into(),
            domain: "content".into(),
            base: Some(base.into()),
            proposed: Some(proposed.into()),
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
    assert_eq!(preview.expected_baseline, project.content_baseline());
    let exported = serde_json::to_value(&preview).unwrap();
    assert_eq!(exported["expected_baseline"], project.content_baseline());
    assert_eq!(
        exported["files"][0]["differences"][0]["path"],
        "/placements/p1/x"
    );
    assert!(preview.files[0].differences.iter().any(|difference| {
        difference.path == "/placements/p1/x"
            && difference.base.as_deref() == Some("0")
            && difference.current.as_deref() == Some("0")
            && difference.proposed.as_deref() == Some("10")
    }));

    let expected_revision = revision;
    collaboration::apply_proposal(
        &mut project,
        &mut revision,
        ApplyProposalCommand {
            expected_revision,
            expected_baseline: preview.expected_baseline,
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
                expected_baseline: preview.expected_baseline,
                proposal_id: name.into(),
            },
        )
        .is_err());
        assert_eq!(project.content_baseline(), before_apply);
    }
}

#[test]
fn proposal_review_baseline_rejects_changes_even_when_revision_did_not_advance() {
    let mut project = project("review_stale");
    let path = project.root.join(".world/maps/city.json");
    let base =
        String::from_utf8(project.authoring_document(&path).unwrap().bytes().to_vec()).unwrap();
    let draft = proposal("review_stale", base, map_json(10, 0, &["a", "b"], true));
    let mut revision = Revision::default();
    let baseline = project.content_baseline();
    let expected_revision = revision;
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
    let indexed = collaboration::build_proposal_index(&project);
    let preview =
        collaboration::preview_proposal(&project, &indexed.proposals["review_stale"].draft)
            .unwrap();
    let before = project.content_baseline();
    project
        .set_authoring_document(&path, map_json(0, 12, &["a", "b"], true).into_bytes())
        .unwrap();
    let changed = project.content_baseline();
    assert_ne!(before, changed);
    let expected_revision = revision;
    let error = collaboration::apply_proposal(
        &mut project,
        &mut revision,
        ApplyProposalCommand {
            expected_revision,
            expected_baseline: preview.expected_baseline,
            proposal_id: "review_stale".into(),
        },
    )
    .unwrap_err();
    assert!(error.contains("StaleBaseline"), "{error}");
    assert_eq!(project.content_baseline(), changed);
}

#[test]
fn proposal_review_reports_exact_utf8_paragraph_ranges_without_writing() {
    let mut project = project("review_paragraphs");
    let entry = project.entry.clone();
    let base = "entity a kind place as \"甲\"\n\nentity b kind place as \"乙\"\nrelation_type knows as \"认识\"\nrelation_def rel type knows from entity a to entity b\n";
    project.set_text(&entry, base.into()).unwrap();
    let proposed = base.replace("\"乙\"", "\"乙改\"");
    let draft = ProposalDraft {
        id: "paragraphs".into(),
        author: "甲".into(),
        reason: "改名".into(),
        status: ProposalStatus::Open,
        changes: vec![ProposalFileChange {
            path: "world.wl".into(),
            domain: "content".into(),
            base: Some(base.into()),
            proposed: Some(proposed.clone()),
        }],
    };
    let baseline = project.content_baseline();
    let preview = collaboration::preview_proposal(&project, &draft).unwrap();
    assert_eq!(project.content_baseline(), baseline);
    assert!(preview.files[0].reference_impact_complete);
    assert!(preview.files[0].reference_impacts.iter().any(|impact| {
        impact.target == TargetRef::new("entity", "a")
            && !impact.current.is_empty()
            && !impact.proposed.is_empty()
    }));
    assert_eq!(preview.files[0].differences.len(), 1);
    let difference = &preview.files[0].differences[0];
    assert_eq!(difference.path, "/paragraphs/2");
    let range = difference.proposed_range.as_ref().unwrap();
    assert_eq!(
        &proposed[range.start_byte..range.end_byte],
        difference.proposed.as_deref().unwrap()
    );
    assert_eq!(range.start_byte, base.find("entity b").unwrap());
}

#[test]
fn proposal_review_has_explicit_budget_and_retains_conflict() {
    let mut project = project("review_budget");
    let entry = project.entry.clone();
    let base = project.document(&entry).unwrap().to_owned();
    let long = format!("{}\n\n{}", base, "长".repeat(20_000));
    let draft = ProposalDraft {
        id: "large".into(),
        author: "甲".into(),
        reason: "长文".into(),
        status: ProposalStatus::Open,
        changes: vec![ProposalFileChange {
            path: "world.wl".into(),
            domain: "content".into(),
            base: Some(base.clone()),
            proposed: Some(long),
        }],
    };
    project
        .set_text(&entry, format!("{base}\n// 并行编辑"))
        .unwrap();
    let preview = collaboration::preview_proposal(&project, &draft).unwrap();
    assert!(preview.files[0].truncated);
    assert!(!preview.can_apply());
    assert!(!preview.files[0].differences.is_empty());
    for text in [
        preview.files[0].raw.base.as_deref(),
        preview.files[0].raw.current.as_deref(),
        preview.files[0].raw.proposed.as_deref(),
    ]
    .into_iter()
    .chain(preview.files[0].differences.iter().flat_map(|difference| {
        [
            difference.base.as_deref(),
            difference.current.as_deref(),
            difference.proposed.as_deref(),
        ]
    }))
    .flatten()
    {
        assert!(text.len() <= 16 * 1024);
    }
}

#[test]
fn proposal_review_bounds_large_json_field_text() {
    let project = project("review_json_text_budget");
    let map_path = project.root.join(".world/maps/city.json");
    let mut base: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&map_path).unwrap().bytes()).unwrap();
    base["extensions"]["large_note"] = serde_json::Value::String("甲".repeat(20_000));
    let mut proposed = base.clone();
    proposed["extensions"]["large_note"] = serde_json::Value::String("乙".repeat(20_000));
    let draft = proposal(
        "json_text_budget",
        serde_json::to_string_pretty(&base).unwrap(),
        serde_json::to_string_pretty(&proposed).unwrap(),
    );

    let preview = collaboration::preview_proposal(&project, &draft).unwrap();
    let file = &preview.files[0];
    assert!(file.truncated);
    let difference = file
        .differences
        .iter()
        .find(|difference| difference.path == "/extensions/large_note")
        .unwrap();
    for text in [
        difference.base.as_deref(),
        difference.current.as_deref(),
        difference.proposed.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        assert!(text.len() <= 16 * 1024);
    }
}

#[test]
fn proposal_review_distinguishes_json_reformatting_from_semantic_changes() {
    let project = project("review_format");
    let map_path = project.root.join(".world/maps/city.json");
    let base = String::from_utf8(
        project
            .authoring_document(&map_path)
            .unwrap()
            .bytes()
            .to_vec(),
    )
    .unwrap();
    let proposed =
        serde_json::to_string(&serde_json::from_str::<serde_json::Value>(&base).unwrap()).unwrap();
    assert_ne!(base, proposed);
    let draft = proposal("format_only", base, proposed);
    let preview = collaboration::preview_proposal(&project, &draft).unwrap();
    assert!(preview.files[0].changed);
    assert!(!preview.files[0].semantic_changed);
    assert!(preview.files[0].differences.is_empty());
}

#[test]
fn proposal_review_aligns_inserted_and_deleted_paragraphs_to_their_source_ranges() {
    let mut project = project("review_paragraph_alignment");
    let entry = project.entry.clone();
    let base = "// 第一段\n\n// 第二段\n\n// 第三段\n";
    project.set_text(&entry, base.into()).unwrap();

    let inserted = format!("// 新段落\n\n{base}");
    let insert_review = collaboration::preview_proposal(
        &project,
        &content_proposal("insert_paragraph", "world.wl", base, &inserted),
    )
    .unwrap();
    let insert_file = &insert_review.files[0];
    assert_eq!(insert_file.differences.len(), 1);
    let insertion = &insert_file.differences[0];
    assert!(insertion.base.is_none());
    assert!(insertion.current.is_none());
    assert!(insertion
        .proposed
        .as_deref()
        .unwrap()
        .starts_with("// 新段落"));
    let range = insertion.proposed_range.as_ref().unwrap();
    assert_eq!(
        &inserted[range.start_byte..range.end_byte],
        insertion.proposed.as_deref().unwrap()
    );

    let without_middle = "// 第一段\n\n// 第三段\n";
    let delete_review = collaboration::preview_proposal(
        &project,
        &content_proposal("delete_paragraph", "world.wl", base, without_middle),
    )
    .unwrap();
    let delete_file = &delete_review.files[0];
    assert_eq!(delete_file.differences.len(), 1);
    let deletion = &delete_file.differences[0];
    assert_eq!(deletion.path, "/paragraphs/2");
    assert!(deletion.base.as_deref().unwrap().starts_with("// 第二段"));
    assert!(deletion
        .current
        .as_deref()
        .unwrap()
        .starts_with("// 第二段"));
    assert!(deletion.proposed.is_none());
}

#[test]
fn proposal_review_keeps_trailing_paragraphs_aligned_after_concurrent_insertion() {
    let mut project = project("review_concurrent_paragraph_insertion");
    let entry = project.entry.clone();
    let base = "// 第一段\n\n// 第二段\n\n// 第三段\n";
    let current = "// 第一段\n\n// 并行新增\n\n// 第二段\n\n// 第三段\n";
    let proposed = "// 第一段\n\n// 第二段修改\n\n// 第三段\n";
    project.set_text(&entry, current.into()).unwrap();
    let review = collaboration::preview_proposal(
        &project,
        &content_proposal("concurrent_insertion", "world.wl", base, proposed),
    )
    .unwrap();

    let differences = &review.files[0].differences;
    assert_eq!(differences.len(), 2);
    assert!(differences.iter().any(|difference| {
        difference.base.is_none()
            && difference
                .current
                .as_deref()
                .unwrap()
                .starts_with("// 并行新增")
            && difference.proposed.is_none()
    }));
    assert!(differences.iter().any(|difference| {
        difference
            .base
            .as_deref()
            .unwrap_or_default()
            .starts_with("// 第二段")
            && difference
                .current
                .as_deref()
                .unwrap_or_default()
                .starts_with("// 第二段")
            && difference
                .proposed
                .as_deref()
                .unwrap_or_default()
                .starts_with("// 第二段修改")
    }));
    assert!(!differences.iter().any(|difference| {
        difference
            .base
            .as_deref()
            .unwrap_or_default()
            .starts_with("// 第三段")
    }));
}

#[test]
fn proposal_review_splits_crlf_paragraphs_and_preserves_utf8_byte_ranges() {
    let mut project = project("review_crlf_paragraphs");
    let entry = project.entry.clone();
    let base = "// 第一段\r\n\r\n// 第二段\r\n\r\n// 第三段\r\n";
    let proposed = base.replace("第二段", "第二段修改");
    project.set_text(&entry, base.into()).unwrap();
    let review = collaboration::preview_proposal(
        &project,
        &content_proposal("crlf_paragraphs", "world.wl", base, &proposed),
    )
    .unwrap();

    let differences = &review.files[0].differences;
    assert_eq!(differences.len(), 1);
    assert_eq!(differences[0].path, "/paragraphs/2");
    let range = differences[0].proposed_range.as_ref().unwrap();
    assert_eq!(range.start_byte, proposed.find("// 第二段修改").unwrap());
    assert_eq!(
        &proposed[range.start_byte..range.end_byte],
        differences[0].proposed.as_deref().unwrap()
    );
}

#[test]
fn proposal_review_marks_ambiguous_duplicate_paragraphs_as_raw_fallback() {
    let mut project = project("review_ambiguous_paragraphs");
    let entry = project.entry.clone();
    let base = "// 重复段落\n\n// 重复段落\n\n// 结尾\n";
    let proposed = "// 重复段落\n\n// 结尾\n";
    project.set_text(&entry, base.into()).unwrap();
    let review = collaboration::preview_proposal(
        &project,
        &content_proposal("ambiguous_paragraphs", "world.wl", base, proposed),
    )
    .unwrap();

    let file = &review.files[0];
    assert!(file.alignment_uncertain);
    assert_eq!(file.differences.len(), 1);
    assert_eq!(file.differences[0].path, "");
    assert_eq!(file.raw.base.as_deref(), Some(base));
    assert_eq!(file.raw.current.as_deref(), Some(base));
    assert_eq!(file.raw.proposed.as_deref(), Some(proposed));
}

#[test]
fn proposal_review_reference_impacts_use_the_combined_multifile_candidate() {
    let mut project = project("review_multifile_candidate");
    let entry = project.entry.clone();
    let extra = project.add_file(std::path::Path::new("extra.wl")).unwrap();
    let entry_base = project.document(&entry).unwrap().to_owned();
    let extra_base = "entity c kind place as \"丙\"\n";
    project.set_text(&extra, extra_base.into()).unwrap();
    let entry_proposed = entry_base.replace("entity a kind place", "entity shared kind place");
    let extra_proposed = extra_base.replace("entity c kind place", "entity shared kind place");

    let preview = collaboration::preview_proposal(
        &project,
        &ProposalDraft {
            id: "multifile_candidate".into(),
            author: "作者甲".into(),
            reason: "跨文件重复对象身份应被候选编译发现".into(),
            status: ProposalStatus::Open,
            changes: vec![
                ProposalFileChange {
                    path: "world.wl".into(),
                    domain: "content".into(),
                    base: Some(entry_base),
                    proposed: Some(entry_proposed),
                },
                ProposalFileChange {
                    path: "extra.wl".into(),
                    domain: "content".into(),
                    base: Some(extra_base.into()),
                    proposed: Some(extra_proposed),
                },
            ],
        },
    )
    .unwrap();

    assert_eq!(preview.files.len(), 2);
    assert!(preview
        .files
        .iter()
        .all(|file| !file.reference_impact_complete));
}

#[test]
fn proposal_review_reference_lists_reflect_sibling_file_edits() {
    let mut project = project("review_multifile_references");
    let entry = project.entry.clone();
    let extra = project.add_file(std::path::Path::new("extra.wl")).unwrap();
    let entry_base = project.document(&entry).unwrap().to_owned();
    let extra_base = "relation_def rel_extra type knows from entity a to entity b\n";
    project.set_text(&extra, extra_base.into()).unwrap();
    let entry_proposed = entry_base.replace(
        "entity a kind place as \"甲\"",
        "entity a kind place as \"甲新\"",
    );

    let preview = collaboration::preview_proposal(
        &project,
        &ProposalDraft {
            id: "multifile_references".into(),
            author: "作者甲".into(),
            reason: "跨文件引用修改应反映在同一候选中".into(),
            status: ProposalStatus::Open,
            changes: vec![
                ProposalFileChange {
                    path: "world.wl".into(),
                    domain: "content".into(),
                    base: Some(entry_base),
                    proposed: Some(entry_proposed),
                },
                ProposalFileChange {
                    path: "extra.wl".into(),
                    domain: "content".into(),
                    base: Some(extra_base.into()),
                    proposed: Some(String::new()),
                },
            ],
        },
    )
    .unwrap();

    let file = preview
        .files
        .iter()
        .find(|file| file.path == "world.wl")
        .unwrap();
    let impact = file
        .reference_impacts
        .iter()
        .find(|impact| impact.target == TargetRef::new("entity", "a"))
        .unwrap();
    assert!(impact
        .current
        .iter()
        .any(|reference| { reference.source == TargetRef::new("relation", "rel_extra") }));
    assert!(!impact
        .proposed
        .iter()
        .any(|reference| { reference.source == TargetRef::new("relation", "rel_extra") }));
    assert!(file.reference_impact_complete);
}

#[test]
fn proposal_review_reference_impacts_match_the_exact_changed_file_path() {
    let mut project = project("review_exact_reference_file");
    let entry = project.entry.clone();
    let nested = project
        .add_file(std::path::Path::new("nested/world.wl"))
        .unwrap();
    let entry_base = project.document(&entry).unwrap().to_owned();
    let nested_base = "entity nested_only kind place as \"嵌套对象\"\n";
    project.set_text(&nested, nested_base.into()).unwrap();
    let proposed = entry_base.replace(
        "entity a kind place as \"甲\"",
        "entity a kind place as \"甲新\"",
    );

    let preview = collaboration::preview_proposal(
        &project,
        &content_proposal("exact_reference_file", "world.wl", &entry_base, &proposed),
    )
    .unwrap();
    let targets = &preview.files[0].reference_impacts;
    assert!(targets
        .iter()
        .any(|impact| impact.target == TargetRef::new("entity", "a")));
    assert!(!targets
        .iter()
        .any(|impact| impact.target == TargetRef::new("entity", "nested_only")));
}

#[test]
fn proposal_review_reference_limit_truncates_each_target_and_marks_incomplete() {
    for (reference_count, expected_complete) in [(256, true), (257, false)] {
        let mut project = project(&format!("review_reference_limit_{reference_count}"));
        let entry = project.entry.clone();
        let references_file = project
            .add_file(std::path::Path::new("references.wl"))
            .unwrap();
        let entry_base = project.document(&entry).unwrap().replace(
            "relation_def rel type knows from entity a to entity b\n",
            "",
        );
        project.set_text(&entry, entry_base.clone()).unwrap();
        let mut references = String::new();
        for index in 0..reference_count {
            references.push_str(&format!(
                "relation_def rel{index} type knows from entity a to entity b\n"
            ));
        }
        project.set_text(&references_file, references).unwrap();
        let proposed = entry_base.replace(
            "entity a kind place as \"甲\"",
            "entity a kind place as \"甲新\"",
        );
        let review = collaboration::preview_proposal(
            &project,
            &content_proposal(
                &format!("reference_limit_{reference_count}"),
                "world.wl",
                &entry_base,
                &proposed,
            ),
        )
        .unwrap();

        let impact = review.files[0]
            .reference_impacts
            .iter()
            .find(|impact| impact.target == TargetRef::new("entity", "a"))
            .unwrap();
        assert_eq!(impact.current.len(), 256);
        assert_eq!(impact.proposed.len(), 256);
        assert_eq!(review.files[0].reference_impact_complete, expected_complete);
    }
}

#[test]
fn proposal_review_bounds_affected_objects_per_content_file() {
    for (object_count, expected_complete) in [(255, true), (256, false)] {
        let mut project = project(&format!("review_object_limit_{object_count}"));
        let entry = project.entry.clone();
        let mut base = String::new();
        for index in 0..object_count {
            base.push_str(&format!("entity e{index} kind place as \"name{index}\"\n"));
        }
        let proposed = base.replace(
            "entity e0 kind place as \"name0\"",
            "entity e0 kind place as \"updated\"",
        );
        project.set_text(&entry, base.clone()).unwrap();
        let review = collaboration::preview_proposal(
            &project,
            &content_proposal(
                &format!("object_limit_{object_count}"),
                "world.wl",
                &base,
                &proposed,
            ),
        )
        .unwrap();

        assert_eq!(review.files[0].reference_impacts.len(), 256);
        assert_eq!(review.files[0].reference_impact_complete, expected_complete);
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

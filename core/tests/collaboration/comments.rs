use super::*;

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

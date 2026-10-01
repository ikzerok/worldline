use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{
    collaboration::*, presentation_commands::Revision, project::Project, TargetRef,
};

fn project() -> Project {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "review-comments-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(
        root.join("world.wl"),
        "character traveler as \"旅人\"\nevent start\n  中文第一行。\n  第二行。\n  -> END\n",
    )
    .unwrap();
    fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.9","entry":"world.wl","required_features":[]}"#).unwrap();
    Project::open(&root).unwrap()
}
fn index(project: &Project) -> CommentIndex {
    let result = project.clone().compile();
    let maps = worldline_core::presentation_commands::map_index_with_content(project, &result);
    build_comment_index(project, &result, &maps)
}
fn write(
    project: &mut Project,
    revision: &mut Revision,
    original: Option<&str>,
    draft: CommentDraft,
) -> Result<CollaborationResult, String> {
    let expected_revision = *revision;
    let expected_baseline = project.content_baseline();
    write_comment(
        project,
        revision,
        CommentCommand {
            expected_revision,
            expected_baseline,
            original: original.map(str::to_owned),
            draft,
        },
    )
}
fn note(id: &str, anchor: CommentAnchor) -> CommentDraft {
    CommentDraft {
        id: id.into(),
        author: "作者甲".into(),
        body: "PRIVATE_REVIEW_SENTINEL".into(),
        anchor,
        resolved: false,
    }
}
#[test]
fn selection_preview_uses_actual_unicode_multiline_source_and_refuses_unapplied_or_empty() {
    let p = project();
    let source = p.document(&p.entry).unwrap();
    let start = source.find("中文").unwrap();
    let end = source.find("  -> END").unwrap();
    let anchor = capture_text_selection(&p, &p.entry, source, start..end).unwrap();
    assert!(
        matches!(anchor, CommentAnchor::TextRange { start_line:3,end_line:4,ref quote,.. } if quote=="  中文第一行。\n  第二行。")
    );
    assert!(capture_text_selection(&p, &p.entry, source, start..start).is_err());
    assert!(capture_text_selection(&p, &p.entry, source, start + 1..end).is_err());
    assert!(
        capture_text_selection(&p, &p.entry, &source.replace("中文", "未应用"), start..end)
            .unwrap_err()
            .contains("未应用")
    );
    assert_eq!(p.document(&p.entry).unwrap(), source);
}
#[test]
fn review_projection_all_300_items_filter_separate_resolution_and_anchor_without_todo_changes() {
    let p = project();
    let mut notes = CommentIndex::default();
    for at in 0..300 {
        let id = format!("note_{at:03}");
        let mut draft = note(
            &id,
            CommentAnchor::Object {
                target: TargetRef::new("character", "traveler"),
            },
        );
        draft.body = format!("第{at}条修订");
        draft.resolved = at % 3 == 0;
        notes.comments.insert(
            id,
            CommentDocument {
                draft,
                path: p.root.join(format!(".world/comments/{at}.json")),
                source: serde_json::json!({}),
                read_only: at == 299,
                anchor_status: if at % 2 == 0 {
                    AnchorStatus::Attached
                } else {
                    AnchorStatus::Detached
                },
            },
        );
    }
    let open = notes.review_projection(&Default::default());
    assert_eq!((open.total, open.unresolved, open.matched), (300, 200, 200));
    assert_eq!(open.items.last().unwrap().draft.id, "note_299");
    assert!(open.items.last().unwrap().read_only);
    let filter = CommentReviewFilter {
        resolution: CommentResolutionFilter::All,
        text: "NOTE_299".into(),
        ..Default::default()
    };
    assert_eq!(notes.review_projection(&filter).matched, 1);
    let filter = CommentReviewFilter {
        resolution: CommentResolutionFilter::Resolved,
        anchor: CommentAnchorFilter::Attached,
        text: String::new(),
    };
    assert_eq!(notes.review_projection(&filter).matched, 50);
    assert_eq!(p.todo_projection().schema_version, 1);
}
#[test]
fn detached_note_can_resolve_without_rebinding_but_new_or_changed_bad_anchor_rejects() {
    let mut p = project();
    let mut revision = Revision::default();
    let anchor = capture_text_anchor(&p, &p.entry, 3, 3).unwrap();
    write(&mut p, &mut revision, None, note("text", anchor)).unwrap();
    let path = p.entry.clone();
    let changed = p.document(&path).unwrap().replace("中文第一行。", "改稿。");
    p.set_text(&path, changed).unwrap();
    let mut draft = index(&p).comments["text"].draft.clone();
    assert_eq!(
        index(&p).comments["text"].anchor_status,
        AnchorStatus::Detached
    );
    draft.resolved = true;
    write(&mut p, &mut revision, Some("text"), draft.clone()).unwrap();
    assert_eq!(
        index(&p).comments["text"].anchor_status,
        AnchorStatus::Detached
    );
    assert!(index(&p).comments["text"].draft.resolved);
    let baseline = p.content_baseline();
    draft.anchor = CommentAnchor::Object {
        target: TargetRef::new("character", "missing"),
    };
    assert!(write(&mut p, &mut revision, Some("text"), draft.clone()).is_err());
    draft.id = "new_bad".into();
    assert!(write(&mut p, &mut revision, None, draft).is_err());
    assert_eq!(p.content_baseline(), baseline);
}
#[test]
fn comment_roundtrip_preserves_nested_extras_and_fingerprint_and_rejects_disk_conflict() {
    let mut p = project();
    let mut revision = Revision::default();
    let fingerprint = p.compile().analysis.fingerprint;
    write(
        &mut p,
        &mut revision,
        None,
        note(
            "object",
            CommentAnchor::Object {
                target: TargetRef::new("character", "traveler"),
            },
        ),
    )
    .unwrap();
    let path = index(&p).comments["object"].path.clone();
    let mut raw: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&path).unwrap().bytes()).unwrap();
    raw["future_top"] = 7.into();
    raw["anchor"]["future_anchor"] = "keep".into();
    raw["anchor"]["target"]["future_target"] = true.into();
    p.set_authoring_document(&path, serde_json::to_vec(&raw).unwrap())
        .unwrap();
    let mut draft = index(&p).comments["object"].draft.clone();
    draft.resolved = true;
    write(&mut p, &mut revision, Some("object"), draft.clone()).unwrap();
    let after = &index(&p).comments["object"].source;
    assert_eq!(after["future_top"], 7);
    assert_eq!(after["anchor"]["future_anchor"], "keep");
    assert_eq!(after["anchor"]["target"]["future_target"], true);
    assert_eq!(p.compile().analysis.fingerprint, fingerprint);
    p.save().unwrap();
    fs::write(&path, b"external replacement").unwrap();
    let before = p.content_baseline();
    draft.body = "do not overwrite".into();
    assert!(write(&mut p, &mut revision, Some("object"), draft).is_err());
    assert_eq!(p.content_baseline(), before);
    assert_eq!(fs::read(&path).unwrap(), b"external replacement");
}
#[test]
fn unknown_comment_capability_keeps_original_bytes_and_refuses_edit() {
    let mut p = project();
    let mut revision = Revision::default();
    write(
        &mut p,
        &mut revision,
        None,
        note(
            "private",
            CommentAnchor::Object {
                target: TargetRef::new("character", "traveler"),
            },
        ),
    )
    .unwrap();
    p.save().unwrap();
    let path = index(&p).comments["private"].path.clone();
    let mut raw: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    raw["required_features"] = serde_json::json!(["future.review.v99"]);
    let bytes = serde_json::to_vec(&raw).unwrap();
    fs::write(&path, &bytes).unwrap();
    let mut p = Project::open(&p.root).unwrap();
    let draft = index(&p).comments["private"].draft.clone();
    assert!(index(&p).comments["private"].read_only);
    let before = p.content_baseline();
    assert!(write(&mut p, &mut revision, Some("private"), draft).is_err());
    assert_eq!(p.content_baseline(), before);
    assert_eq!(p.authoring_document(&path).unwrap().bytes(), bytes);
}

#[test]
fn valid_unresolved_comments_are_in_review_without_changing_legacy_todo_or_public_exports() {
    use worldline_core::reader_export::ReaderExportSelection;
    let mut p = project();
    let mut revision = Revision::default();
    let initial = p.compile().analysis.fingerprint;
    write(
        &mut p,
        &mut revision,
        None,
        note(
            "private_note",
            CommentAnchor::Object {
                target: TargetRef::new("event", "start"),
            },
        ),
    )
    .unwrap();
    let projection = index(&p).review_projection(&Default::default());
    assert_eq!(projection.unresolved, 1);
    assert_eq!(projection.items[0].anchor_status, AnchorStatus::Attached);
    assert!(!p
        .todo_projection()
        .items
        .iter()
        .any(|item| item.kind == worldline_core::queries::TodoKind::DetachedComment));
    let request = ReaderExportSelection {
        schema_version: 1,
        site_title: "Public".into(),
        objects: vec![TargetRef::new("event", "start")],
        manuscripts: vec![],
        attachments: vec![],
        maps: vec![],
        fields: vec![],
        required_features: vec![],
    };
    let preview = p.preview_reader_export(&request).unwrap();
    let files = p
        .build_reader_export(&request, &preview.plan_digest)
        .unwrap();
    for (path, bytes) in files {
        assert!(!path.to_string_lossy().contains("comment"));
        assert!(!String::from_utf8_lossy(&bytes).contains("PRIVATE_REVIEW_SENTINEL"));
    }
    assert_eq!(p.compile().analysis.fingerprint, initial);
}

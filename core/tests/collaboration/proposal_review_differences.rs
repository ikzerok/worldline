use super::*;

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

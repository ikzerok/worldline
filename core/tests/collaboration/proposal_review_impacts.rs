use super::*;

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

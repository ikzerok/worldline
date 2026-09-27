use super::*;

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
fn proposal_resolutions_require_every_conflict_and_preserve_the_original_proposal() {
    let mut project = project("resolve_values");
    let path = project.root.join(".world/maps/city.json");
    let base =
        String::from_utf8(project.authoring_document(&path).unwrap().bytes().to_vec()).unwrap();
    let mut proposed_value: serde_json::Value =
        serde_json::from_str(&map_json(10, 10, &["a", "b"], true)).unwrap();
    proposed_value["extensions"]["owner/name~tag"] = "提议值".into();
    let proposed = serde_json::to_string(&proposed_value).unwrap();
    let draft = proposal("resolve_values", base, proposed.clone());
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

    let mut current: serde_json::Value =
        serde_json::from_str(&map_json(20, 20, &["a", "b"], true)).unwrap();
    current["placements"]["p1"]["annotation"] = "当前注释".into();
    current["extensions"]["owner/name~tag"] = "当前值".into();
    let current_text = serde_json::to_string(&current).unwrap();
    project
        .set_authoring_document(&path, current_text.as_bytes().to_vec())
        .unwrap();
    let indexed = collaboration::build_proposal_index(&project);
    let preview =
        collaboration::preview_proposal(&project, &indexed.proposals["resolve_values"].draft)
            .unwrap();
    assert_eq!(preview.conflicts.len(), 3, "{:?}", preview.conflicts);
    let current_bytes = project.authoring_document(&path).unwrap().bytes().to_vec();
    let current_baseline = project.content_baseline();
    let current_revision = revision;

    let first = &preview.conflicts[0];
    let incomplete = [ProposalResolution {
        path: first.path.clone(),
        location: first.location.clone(),
        value: Some("30".into()),
    }];
    let expected_revision = revision;
    let error = collaboration::apply_proposal_with_resolutions(
        &mut project,
        &mut revision,
        ApplyProposalCommand {
            expected_revision,
            expected_baseline: preview.expected_baseline.clone(),
            proposal_id: "resolve_values".into(),
        },
        &incomplete,
    )
    .unwrap_err();
    assert!(error.contains("未解决"), "{error}");
    assert_eq!(
        project.authoring_document(&path).unwrap().bytes(),
        current_bytes.as_slice()
    );
    assert_eq!(project.content_baseline(), current_baseline);
    assert_eq!(revision, current_revision);

    let invalid = preview
        .conflicts
        .iter()
        .map(|conflict| ProposalResolution {
            path: conflict.path.clone(),
            location: conflict.location.clone(),
            value: Some(
                if conflict.location.ends_with("/x") {
                    "不是 JSON"
                } else {
                    "40"
                }
                .into(),
            ),
        })
        .collect::<Vec<_>>();
    let expected_revision = revision;
    let error = collaboration::apply_proposal_with_resolutions(
        &mut project,
        &mut revision,
        ApplyProposalCommand {
            expected_revision,
            expected_baseline: preview.expected_baseline.clone(),
            proposal_id: "resolve_values".into(),
        },
        &invalid,
    )
    .unwrap_err();
    assert!(error.contains("JSON"), "{error}");
    assert_eq!(
        project.authoring_document(&path).unwrap().bytes(),
        current_bytes.as_slice()
    );
    assert_eq!(project.content_baseline(), current_baseline);
    assert_eq!(revision, current_revision);

    let resolutions = preview
        .conflicts
        .iter()
        .map(|conflict| ProposalResolution {
            path: conflict.path.clone(),
            location: conflict.location.clone(),
            value: Some(
                if conflict.location.ends_with("/x") {
                    "30"
                } else if conflict.location.ends_with("/y") {
                    "40"
                } else {
                    "\"已解决\""
                }
                .into(),
            ),
        })
        .collect::<Vec<_>>();
    let expected_revision = revision;
    collaboration::apply_proposal_with_resolutions(
        &mut project,
        &mut revision,
        ApplyProposalCommand {
            expected_revision,
            expected_baseline: preview.expected_baseline,
            proposal_id: "resolve_values".into(),
        },
        &resolutions,
    )
    .unwrap();

    let merged: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    assert_eq!(merged["placements"]["p1"]["x"], 30);
    assert_eq!(merged["placements"]["p2"]["y"], 40);
    assert_eq!(merged["extensions"]["owner/name~tag"], "已解决");
    let stored = &collaboration::build_proposal_index(&project).proposals["resolve_values"];
    assert_eq!(stored.draft.status, ProposalStatus::Accepted);
    assert_eq!(
        stored.draft.changes[0].proposed.as_deref(),
        Some(proposed.as_str())
    );
}

#[test]
fn proposal_resolution_can_choose_deletion_and_replace_a_text_conflict() {
    let mut project = project("resolve_delete");
    let path = project.root.join(".world/maps/city.json");
    let base =
        String::from_utf8(project.authoring_document(&path).unwrap().bytes().to_vec()).unwrap();
    let proposed = map_json(0, 0, &["a", "b"], false);
    let draft = proposal("resolve_delete", base, proposed);
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
    project
        .set_authoring_document(&path, map_json(20, 0, &["a", "b"], true).into_bytes())
        .unwrap();
    let indexed = collaboration::build_proposal_index(&project);
    let preview =
        collaboration::preview_proposal(&project, &indexed.proposals["resolve_delete"].draft)
            .unwrap();
    assert_eq!(preview.conflicts.len(), 1);
    let conflict = &preview.conflicts[0];
    let expected_revision = revision;
    collaboration::apply_proposal_with_resolutions(
        &mut project,
        &mut revision,
        ApplyProposalCommand {
            expected_revision,
            expected_baseline: preview.expected_baseline,
            proposal_id: "resolve_delete".into(),
        },
        &[ProposalResolution {
            path: conflict.path.clone(),
            location: conflict.location.clone(),
            value: None,
        }],
    )
    .unwrap();
    let merged: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    assert!(!merged["placements"].as_object().unwrap().contains_key("p1"));

    let mut text_project = self::project("resolve_text");
    let entry = text_project.entry.clone();
    let base = text_project.document(&entry).unwrap().to_owned();
    let proposed = base.replacen("\"甲\"", "\"提议\"", 1);
    let mut revision = Revision::default();
    let baseline = text_project.content_baseline();
    let expected_revision = revision;
    collaboration::write_proposal(
        &mut text_project,
        &mut revision,
        ProposalCommand {
            expected_revision,
            expected_baseline: baseline,
            draft: content_proposal("resolve_text", "world.wl", &base, &proposed),
        },
    )
    .unwrap();
    let current = base.replacen("\"甲\"", "\"当前\"", 1);
    text_project.set_text(&entry, current).unwrap();
    let indexed = collaboration::build_proposal_index(&text_project);
    let preview =
        collaboration::preview_proposal(&text_project, &indexed.proposals["resolve_text"].draft)
            .unwrap();
    let conflict = &preview.conflicts[0];
    let resolved = base.replacen("\"甲\"", "\"已解决\"", 1);
    let expected_revision = revision;
    collaboration::apply_proposal_with_resolutions(
        &mut text_project,
        &mut revision,
        ApplyProposalCommand {
            expected_revision,
            expected_baseline: preview.expected_baseline,
            proposal_id: "resolve_text".into(),
        },
        &[ProposalResolution {
            path: conflict.path.clone(),
            location: conflict.location.clone(),
            value: Some(resolved.clone()),
        }],
    )
    .unwrap();
    assert_eq!(text_project.document(&entry).unwrap(), resolved.as_str());
    assert!(!text_project.compile().has_errors());
}

#[test]
fn array_conflict_resolution_replaces_the_whole_array_value() {
    let mut project = project("resolve_array");
    let path = project.root.join(".world/maps/city.json");
    let base =
        String::from_utf8(project.authoring_document(&path).unwrap().bytes().to_vec()).unwrap();
    let draft = proposal("resolve_array", base, map_json(0, 0, &["a"], true));
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
    project
        .set_authoring_document(&path, map_json(0, 0, &["b", "a"], true).into_bytes())
        .unwrap();
    let indexed = collaboration::build_proposal_index(&project);
    let preview =
        collaboration::preview_proposal(&project, &indexed.proposals["resolve_array"].draft)
            .unwrap();
    assert_eq!(preview.conflicts.len(), 1, "{:?}", preview.conflicts);
    assert_eq!(preview.conflicts[0].location, "/layer_order");

    let conflict = &preview.conflicts[0];
    let expected_revision = revision;
    collaboration::apply_proposal_with_resolutions(
        &mut project,
        &mut revision,
        ApplyProposalCommand {
            expected_revision,
            expected_baseline: preview.expected_baseline,
            proposal_id: "resolve_array".into(),
        },
        &[ProposalResolution {
            path: conflict.path.clone(),
            location: conflict.location.clone(),
            value: Some("[\"b\"]".into()),
        }],
    )
    .unwrap();
    let merged: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    assert_eq!(merged["layer_order"], serde_json::json!(["b"]));
}

use super::*;

#[test]
fn import_is_one_revision_one_undo_and_worker_typed_candidate() {
    let (mut p, mut r) = project("import");
    let before = p.authoring_document(&path(&p)).unwrap().bytes().to_vec();
    let original = r;
    let plan = preview_batch(
        &p,
        r,
        batch(
            &p,
            r,
            vec![
                SceneOp::EnableScene,
                SceneOp::ImportSvg {
                    layer_id: "svg".into(),
                    title: "矢量".into(),
                    source: source(),
                },
            ],
        ),
    )
    .unwrap();
    assert_eq!(r, original);
    assert_eq!(p.authoring_document(&path(&p)).unwrap().bytes(), before);
    assert!(plan
        .normalized_batch()
        .operations
        .iter()
        .all(|op| !matches!(op, SceneOp::ImportSvg { .. })));
    let normalized = preview_batch(&p, r, plan.normalized_batch().clone()).unwrap();
    assert_eq!(plan.document_after(), normalized.document_after());
    let public = serde_json::to_value(&plan).unwrap();
    for field in ["before", "after", "path", "baseline", "normalized"] {
        assert!(public.get(field).is_none());
    }
    let result = apply_batch(&mut p, &mut r, &plan).unwrap();
    assert_eq!(r, original.next_presentation());
    assert_eq!(result.undo_record.changes.len(), 1);
    let s = scene(&p);
    assert!(s.nodes.values().any(|n| n.clip_rect.is_some()));
    assert!(s
        .nodes
        .values()
        .any(|n| matches!(n.geometry, SceneGeometry::Path { .. })));
    let expected = r;
    presentation_commands::undo(&mut p, &mut r, expected, &result.undo_record).unwrap();
    assert_eq!(p.authoring_document(&path(&p)).unwrap().bytes(), before);
}

#[test]
fn errors_cancel_and_stale_plans_never_mutate() {
    let (mut p, mut r) = project("atomic");
    let before = p.content_baseline();
    let invalid = batch(
        &p,
        r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("a", 10.0),
                index: None,
            },
            SceneOp::Insert {
                node: point("a", 20.0),
                index: None,
            },
        ],
    );
    assert!(preview_batch(&p, r, invalid).is_err());
    assert_eq!(before, p.content_baseline());
    let request = batch(
        &p,
        r,
        vec![
            SceneOp::EnableScene,
            SceneOp::ImportSvg {
                layer_id: "svg".into(),
                title: "绘图".into(),
                source: source(),
            },
        ],
    );
    let mut calls = 0;
    let error =
        preview_batch_with_control(&p, r, request.clone(), &SceneLimits::default(), &mut |_| {
            calls += 1;
            calls < 3
        })
        .unwrap_err();
    assert_eq!(error.code, "SCENE_CANCELLED");
    assert_eq!(before, p.content_baseline());
    let plan = preview_batch(&p, r, request).unwrap();
    let expected = r;
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("b", 10.0),
                index: None,
            },
        ],
    );
    let current = p.content_baseline();
    assert_ne!(r, expected);
    assert_eq!(
        apply_batch(&mut p, &mut r, &plan).unwrap_err().code,
        "SCENE_STALE"
    );
    assert_eq!(current, p.content_baseline());
}

#[test]
fn broken_story_does_not_block_unrelated_geometry() {
    let (mut p, mut r) = project("broken");
    let entry = p.entry.clone();
    p.set_text(&entry, "event broken\n  goto absent\n".into())
        .unwrap();
    assert!(compile_snapshot(&p).has_errors());
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("a", 10.0),
                index: None,
            },
        ],
    );
    assert!(scene(&p).nodes.contains_key("a"));
}

#[test]
fn unchanged_unresolved_link_can_move_but_new_unresolved_link_is_rejected() {
    let (mut p, mut r) = project("links");
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("a", 10.0),
                index: None,
            },
        ],
    );
    let path = path(&p);
    let mut raw: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&path).unwrap().bytes()).unwrap();
    raw["scene"]["nodes"]["a"]["target_ref"] =
        serde_json::json!({"kind":"character","id":"absent"});
    p.set_authoring_document(&path, serde_json::to_vec(&raw).unwrap())
        .unwrap();
    let mut node = scene(&p).nodes["a"].clone();
    node.transform = Affine([1.0, 0.0, 0.0, 1.0, 1.0, 0.0]);
    apply(&mut p, &mut r, vec![SceneOp::Update { node: node.clone() }]);
    node.target_ref = Some(worldline_core::TargetRef::new("character", "another"));
    let before = p.content_baseline();
    assert_eq!(
        preview_batch(&p, r, batch(&p, r, vec![SceneOp::Update { node }]))
            .unwrap_err()
            .code,
        "SCENE_REFERENCE"
    );
    assert_eq!(before, p.content_baseline());
}

#[test]
fn entity_creation_and_binding_commit_together() {
    let (mut p, mut r) = project("entity");
    let manifest = p.root.join(".world/project.json");
    let mut raw: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&manifest).unwrap().bytes()).unwrap();
    raw["language_version"] = "1.10".into();
    p.set_authoring_document(&manifest, serde_json::to_vec(&raw).unwrap())
        .unwrap();
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("a", 10.0),
                index: None,
            },
        ],
    );
    let before = p.clone();
    let request = SceneEntityRequest {
        expected_baseline: p.content_baseline(),
        expected_revision: r,
        expected_documents: BTreeMap::from([(
            p.root.join(".world/maps/../maps/map.json"),
            baseline(&p).into_values().next().unwrap(),
        )]),
        map_id: "map".into(),
        node_id: "a".into(),
        path: p.entry.clone(),
        draft: worldline_core::authoring::EntityDraft {
            id: "tower".into(),
            entity_type: "place".into(),
            display: "灯塔".into(),
            ..Default::default()
        },
    };
    let plan = preview_entity_binding(&p, r, request).unwrap();
    assert_eq!(before.content_baseline(), p.content_baseline());
    assert_eq!(plan.source_changes.len(), 1);
    apply_entity_binding(&mut p, &mut r, &plan).unwrap();
    assert!(compile_snapshot(&p)
        .analysis
        .catalog
        .entities
        .contains_key("tower"));
    assert_eq!(
        scene(&p).nodes["a"].target_ref.as_ref().unwrap().id,
        "tower"
    );
    assert!(p.restore(before));
    assert!(scene(&p).nodes["a"].target_ref.is_none());
}

#[test]
fn modified_public_plan_summary_is_rejected_without_commit() {
    let (mut p, mut r) = project("tampered");
    let before = p.content_baseline();
    let mut plan = preview_batch(
        &p,
        r,
        batch(
            &p,
            r,
            vec![
                SceneOp::EnableScene,
                SceneOp::Insert {
                    node: point("a", 10.0),
                    index: None,
                },
            ],
        ),
    )
    .unwrap();
    plan.affected_nodes.clear();
    assert_eq!(
        apply_batch(&mut p, &mut r, &plan).unwrap_err().code,
        "SCENE_STALE"
    );
    assert_eq!(before, p.content_baseline());
}

#[test]
fn document_baseline_paths_use_registered_identity_and_reject_alias_duplicates() {
    let (mut p, mut r) = project("path-identity");
    let initial = p.content_baseline();
    let mut request = batch(&p, r, vec![SceneOp::EnableScene]);
    let hash = request.expected_documents.pop_first().unwrap().1;
    let alias = p.root.join(".world/maps/../maps/map.json");
    request.expected_documents.insert(alias, hash.clone());
    let plan = preview_batch(&p, r, request.clone()).unwrap();
    assert_eq!(initial, p.content_baseline());
    request.expected_documents.insert(path(&p), hash);
    assert_eq!(
        preview_batch(&p, r, request.clone()).unwrap_err().code,
        "SCENE_CONFLICT"
    );
    assert_eq!(initial, p.content_baseline());
    request.expected_documents.remove(&path(&p));
    *request.expected_documents.values_mut().next().unwrap() = "wrong-hash".into();
    assert_eq!(
        preview_batch(&p, r, request).unwrap_err().code,
        "SCENE_STALE"
    );
    assert_eq!(initial, p.content_baseline());
    apply_batch(&mut p, &mut r, &plan).unwrap();
    assert!(map(&p).scene.is_some());
}

#[cfg(windows)]
#[test]
fn windows_verbatim_document_baseline_keeps_the_same_registered_identity() {
    let (mut p, mut r) = project("windows-path-identity");
    p.save().unwrap();
    let mut request = batch(&p, r, vec![SceneOp::EnableScene]);
    let (native, hash) = request.expected_documents.pop_first().unwrap();
    let verbatim = std::fs::canonicalize(native).unwrap();
    assert!(verbatim.to_string_lossy().starts_with(r"\\?\"));
    request.expected_documents.insert(verbatim, hash);
    let before = p.content_baseline();
    let plan = preview_batch(&p, r, request).unwrap();
    assert_eq!(before, p.content_baseline());
    apply_batch(&mut p, &mut r, &plan).unwrap();
    assert!(map(&p).scene.is_some());
    std::fs::remove_dir_all(p.root).unwrap();
}

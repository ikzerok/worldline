use super::*;

#[test]
fn nested_svg_rejection_has_location_and_zero_project_changes() {
    let (p, r) = project("nested");
    let before = p.content_baseline();
    let request = batch(
        &p,
        r,
        vec![
            SceneOp::EnableScene,
            SceneOp::ImportSvg {
                layer_id: "svg".into(),
                title: "输入".into(),
                source: "<svg viewBox='0 0 10 10'>\n<svg id='inner' viewBox='0 0 1 1'/></svg>"
                    .into(),
            },
        ],
    );
    let error = preview_batch(&p, r, request).unwrap_err();
    assert_eq!(error.code, "SCENE_SVG_PROFILE");
    assert!(error.line.is_some());
    assert_eq!(before, p.content_baseline());
    assert!(map(&p).scene.is_none());
}

#[test]
fn cumulative_affine_and_viewport_budgets_reject_zero_commit() {
    let (mut p, mut r) = project("numeric");
    apply(&mut p, &mut r, vec![SceneOp::EnableScene]);
    let before = p.content_baseline();
    let mut ops = Vec::new();
    for i in 0..31 {
        let mut n = SceneNode::new(
            format!("g{i}"),
            "places",
            SceneGeometry::Group {
                children: Vec::new(),
            },
        );
        n.parent_id = (i > 0).then(|| format!("g{}", i - 1));
        n.transform = Affine([1e9, 0.0, 0.0, 1e9, 0.0, 0.0]);
        ops.push(SceneOp::Insert {
            node: n,
            index: None,
        });
    }
    let mut pnt = point("p", 1.0);
    pnt.parent_id = Some("g30".into());
    ops.push(SceneOp::Insert {
        node: pnt,
        index: None,
    });
    assert_eq!(
        preview_batch(&p, r, batch(&p, r, ops)).unwrap_err().code,
        "SCENE_NUMERIC"
    );
    assert_eq!(before, p.content_baseline());
    assert!(view_box_transform([0.0, 0.0, 1e-100, 1e-100], 200.0, 100.0, "none").is_err());
    assert!(Affine([1e-7, 0.0, 0.0, 1e-7, 0.0, 0.0]).inverse().is_some());
    assert!(Affine([1e-12, 0.0, 0.0, 1e-12, 0.0, 0.0])
        .inverse()
        .is_none());
    let mut s = MapScene::new(200.0, 100.0);
    let mut n = point("x", 1e9);
    n.transform = Affine([1e9, 0.0, 0.0, 1e9, 0.0, 0.0]);
    s.nodes.insert("x".into(), n);
    s.root_order.insert("places".into(), vec!["x".into()]);
    assert!(scene_to_safe_svg(&s, 200.0, 100.0).is_err());
    assert!(project_scene(&s, 0.25).is_err());
}

#[test]
fn unknown_schema_or_feature_is_read_only_and_preserves_exact_bytes() {
    for (name, value) in [
        ("schema", serde_json::json!(2)),
        ("feature", serde_json::json!(["future.scene"])),
    ] {
        let (mut p, mut r) = project(name);
        apply(&mut p, &mut r, vec![SceneOp::EnableScene]);
        p.save().unwrap();
        let file = path(&p);
        let mut raw: serde_json::Value =
            serde_json::from_slice(p.authoring_document(&file).unwrap().bytes()).unwrap();
        if name == "schema" {
            raw["scene"]["schema_version"] = value;
        } else {
            raw["scene"]["required_features"] = value;
        }
        let bytes = serde_json::to_vec_pretty(&raw).unwrap();
        std::fs::write(&file, &bytes).unwrap();
        let mut reopened = Project::open(&p.root).unwrap();
        assert!(reopened.authoring_document(&file).unwrap().is_read_only());
        assert!(reopened
            .set_authoring_document(&file, b"{}".to_vec())
            .is_err());
        assert_eq!(reopened.authoring_document(&file).unwrap().bytes(), bytes);
    }
}

#[test]
fn unknown_geometry_fields_survive_update_copy_but_not_delete_recreate() {
    let (mut p, mut r) = project("optional");
    let mut node = point("a", 10.0);
    node.geometry = SceneGeometry::Path {
        segments: vec![
            PathSegment::Move { to: [0.0, 0.0] },
            PathSegment::Line { to: [20.0, 20.0] },
        ],
    };
    apply(
        &mut p,
        &mut r,
        vec![SceneOp::EnableScene, SceneOp::Insert { node, index: None }],
    );
    let file = path(&p);
    let mut raw: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&file).unwrap().bytes()).unwrap();
    raw["scene"]["nodes"]["a"]["geometry"]["future"] = "kept".into();
    raw["scene"]["nodes"]["a"]["geometry"]["segments"][1]["optional"] = 17.into();
    p.set_authoring_document(&file, serde_json::to_vec(&raw).unwrap())
        .unwrap();
    let mut node = scene(&p).nodes["a"].clone();
    node.name = "changed".into();
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::Update { node },
            SceneOp::Duplicate {
                node_ids: vec!["a".into()],
                id_prefix: "copy".into(),
                offset: [1.0, 1.0],
            },
        ],
    );
    let raw: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&file).unwrap().bytes()).unwrap();
    for id in ["a", "copy_1"] {
        assert_eq!(raw["scene"]["nodes"][id]["geometry"]["future"], "kept");
        assert_eq!(
            raw["scene"]["nodes"][id]["geometry"]["segments"][1]["optional"],
            17
        );
    }
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::Delete {
                node_ids: vec!["a".into()],
            },
            SceneOp::Insert {
                node: point("a", 20.0),
                index: None,
            },
        ],
    );
    let raw: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&file).unwrap().bytes()).unwrap();
    assert!(raw["scene"]["nodes"]["a"]["geometry"]
        .get("future")
        .is_none());
}

#[test]
fn disk_conflict_and_locked_ancestor_block_commit_or_unlock() {
    let (mut p, mut r) = project("locked");
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("a", 20.0),
                index: None,
            },
            SceneOp::Group {
                group_id: "g".into(),
                node_ids: vec!["a".into()],
                name: "组".into(),
            },
        ],
    );
    let mut n = scene(&p).nodes["a"].clone();
    n.locked = true;
    apply(&mut p, &mut r, vec![SceneOp::Update { node: n }]);
    let mut g = scene(&p).nodes["g"].clone();
    g.locked = true;
    apply(&mut p, &mut r, vec![SceneOp::Update { node: g }]);
    let mut n = scene(&p).nodes["a"].clone();
    n.locked = false;
    assert_eq!(
        preview_batch(&p, r, batch(&p, r, vec![SceneOp::Update { node: n }]))
            .unwrap_err()
            .code,
        "SCENE_LOCKED"
    );
    p.save().unwrap();
    let mut extra = point("extra", 10.0);
    extra.layer_id = "places".into();
    let plan = preview_batch(
        &p,
        r,
        batch(
            &p,
            r,
            vec![SceneOp::Insert {
                node: extra,
                index: None,
            }],
        ),
    )
    .unwrap();
    std::fs::write(path(&p), b"external").unwrap();
    let before = p.content_baseline();
    assert_eq!(
        apply_batch(&mut p, &mut r, &plan).unwrap_err().code,
        "SCENE_CONFLICT"
    );
    assert_eq!(before, p.content_baseline());
}

#[test]
fn duplicate_unknown_payload_is_budgeted_before_amplification() {
    let (mut p, mut r) = project("payload-copy");
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
    let file = path(&p);
    let mut raw: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&file).unwrap().bytes()).unwrap();
    raw["scene"]["nodes"]["a"]["geometry"]["unknown_payload"] = "x".repeat(3 * 1024 * 1024).into();
    p.set_authoring_document(&file, serde_json::to_vec(&raw).unwrap())
        .unwrap();
    let before = p.content_baseline();
    let operations = (0..20)
        .map(|i| SceneOp::Duplicate {
            node_ids: vec!["a".into()],
            id_prefix: format!("copy{i}"),
            offset: [0.0, 0.0],
        })
        .collect();
    assert_eq!(
        preview_batch(&p, r, batch(&p, r, operations))
            .unwrap_err()
            .code,
        "SCENE_LIMIT"
    );
    assert_eq!(before, p.content_baseline());
}

#[test]
fn long_identity_cannot_amplify_before_document_budget_checks() {
    let long = "a".repeat(1024 * 1024);
    let source = format!(
        "<svg id='{long}' viewBox='0 0 100 100'>{}</svg>",
        "<rect width='1' height='1'/>".repeat(20)
    );
    assert_eq!(
        svg_import::preview_scene(&source).unwrap_err().code,
        "SCENE_LIMIT"
    );
    let (mut p, mut r) = project("long-prefix");
    let mut ops = vec![SceneOp::EnableScene];
    for i in 0..20 {
        ops.push(SceneOp::Insert {
            node: point(&format!("p{i}"), 10.0),
            index: None,
        });
    }
    apply(&mut p, &mut r, ops);
    let before = p.content_baseline();
    let ids = scene(&p).nodes.keys().cloned().collect();
    let request = batch(
        &p,
        r,
        vec![SceneOp::Group {
            group_id: long,
            node_ids: ids,
            name: "组".into(),
        }],
    );
    assert_eq!(
        preview_batch(&p, r, request).unwrap_err().code,
        "SCENE_LIMIT"
    );
    assert_eq!(before, p.content_baseline());
}

#[test]
fn repeated_grouping_is_bounded_before_a_later_recursive_operation() {
    let (p, r) = project("depth-ops");
    let mut ops = vec![
        SceneOp::EnableScene,
        SceneOp::Insert {
            node: point("p", 1.0),
            index: None,
        },
    ];
    let mut id = "p".to_owned();
    for i in 0..100 {
        let group = format!("g{i}");
        ops.push(SceneOp::Group {
            group_id: group.clone(),
            node_ids: vec![id],
            name: String::new(),
        });
        id = group;
    }
    ops.push(SceneOp::Duplicate {
        node_ids: vec![id],
        id_prefix: "copy".into(),
        offset: [0.0, 0.0],
    });
    assert_eq!(
        preview_batch(&p, r, batch(&p, r, ops)).unwrap_err().code,
        "SCENE_LIMIT"
    );
    assert!(map(&p).scene.is_none());
}

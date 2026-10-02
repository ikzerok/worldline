use super::*;

#[test]
fn group_duplicate_order_ungroup_move_save_and_reopen() {
    let (mut p, mut r) = project("structure");
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("a", 10.0),
                index: None,
            },
            SceneOp::Insert {
                node: point("b", 30.0),
                index: None,
            },
            SceneOp::Group {
                group_id: "group".into(),
                node_ids: vec!["b".into(), "a".into()],
                name: "组".into(),
            },
        ],
    );
    assert!(
        matches!(&scene(&p).nodes["group"].geometry,SceneGeometry::Group{children} if children==&vec!["a".to_string(),"b".to_string()])
    );
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::Duplicate {
                node_ids: vec!["group".into()],
                id_prefix: "copy".into(),
                offset: [7.0, 3.0],
            },
            SceneOp::Ungroup {
                node_id: "group".into(),
            },
        ],
    );
    let path = path(&p);
    let mut raw: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&path).unwrap().bytes()).unwrap();
    raw["layers"]["other"] =
        serde_json::json!({"title":"另一层","visible_default":true,"locked":false});
    raw["layer_order"]
        .as_array_mut()
        .unwrap()
        .push("other".into());
    p.set_authoring_document(&path, serde_json::to_vec(&raw).unwrap())
        .unwrap();
    let before = node_world_transform(&scene(&p), "a").unwrap();
    apply(
        &mut p,
        &mut r,
        vec![SceneOp::MoveToLayer {
            node_ids: vec!["a".into()],
            layer_id: "other".into(),
        }],
    );
    assert_eq!(node_world_transform(&scene(&p), "a").unwrap(), before);
    assert_eq!(scene(&p).nodes["a"].layer_id, "other");
    p.save().unwrap();
    let reopened = Project::open(&p.root).unwrap();
    assert_eq!(scene(&p), scene(&reopened));
}

#[test]
fn ordinary_groups_work_but_actual_compositing_boundaries_reject() {
    let (mut p, mut r) = project("composite");
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("a", 10.0),
                index: None,
            },
            SceneOp::Group {
                group_id: "g".into(),
                node_ids: vec!["a".into()],
                name: "组".into(),
            },
        ],
    );
    let mut group = scene(&p).nodes["g"].clone();
    group.style.opacity = Some(0.5);
    apply(&mut p, &mut r, vec![SceneOp::Update { node: group }]);
    let before = p.content_baseline();
    assert_eq!(
        preview_batch(
            &p,
            r,
            batch(
                &p,
                r,
                vec![SceneOp::Ungroup {
                    node_id: "g".into()
                }]
            )
        )
        .unwrap_err()
        .code,
        "SCENE_COMPOSITING_BOUNDARY"
    );
    assert_eq!(before, p.content_baseline());
}

#[test]
fn root_slice_clip_projection_and_exchange_are_consistent() {
    let preview = svg_import::preview_scene(&source()).unwrap();
    let root = &preview.scene.nodes[&preview.scene.root_order["svg"][0]];
    let clip = root.clip_rect.unwrap();
    assert!((clip[1] - 100.0 / 3.0).abs() < 1e-8);
    assert!((clip[3] - 100.0 / 3.0).abs() < 1e-8);
    let target = MapScene::new(200.0, 100.0);
    let transform =
        import_view_transform(&preview.scene, preview.width, preview.height, &target).unwrap();
    assert!((transform.point([clip[0], clip[1]])[1] - 100.0 / 6.0).abs() < 1e-8);
    let mut s = preview.scene;
    let count = s.nodes.len();
    for _ in 0..5 {
        let svg = scene_to_safe_svg(&s, 300.0, 100.0).unwrap();
        assert!(svg.contains("clipPath"));
        s = svg_import::preview_scene(&svg).unwrap().scene;
        assert_eq!(s.nodes.len(), count);
    }
    let primitives = project_scene(&s, 0.1).unwrap();
    assert!(primitives.iter().all(|p| !p.clips.is_empty()));
    assert!(primitives.iter().any(|p| !p.paths.is_empty()));
}

#[test]
fn exact_public_selection_ignores_hidden_and_does_not_expand_group() {
    let (mut p, mut r) = project("public");
    let mut a = point("secret_id_a", 10.0);
    a.visible = false;
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: a,
                index: None,
            },
            SceneOp::Insert {
                node: point("secret_id_b", 60.0),
                index: None,
            },
            SceneOp::Group {
                group_id: "g".into(),
                node_ids: vec!["secret_id_a".into(), "secret_id_b".into()],
                name: "private".into(),
            },
        ],
    );
    let m = map(&p);
    let selected = BTreeSet::from(["secret_id_a".into()]);
    let links = BTreeMap::from([(
        "secret_id_a".into(),
        ScenePublicLink {
            href: Some("../objects/p1.html".into()),
            anchor: "marker-1".into(),
            label: "A & B".into(),
        },
    )]);
    let svg = to_safe_svg_with_links(&m, Some(&selected), &links).unwrap();
    assert_eq!(svg.matches("<circle").count(), 1);
    assert!(svg.contains("A &amp; B"));
    assert!(svg.contains("id=\"marker-1\""));
    assert!(!svg.contains("secret_id"));
    assert!(!svg.contains("private"));
    let group = to_safe_svg(&m, Some(&BTreeSet::from(["g".into()]))).unwrap();
    assert!(!group.contains("<circle"));
    assert!(
        to_safe_svg_layers_with_links(&m, Some(&BTreeSet::new()), &BTreeMap::new())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unknown_style_payload_is_not_amplified_by_inheritance() {
    let mut parent = SceneStyle::default();
    parent
        .extra
        .insert("payload".into(), "x".repeat(1024 * 1024).into());
    parent.fill = Some("red".into());
    let inherited = SceneStyle::default().inherited(&parent);
    assert!(inherited.extra.is_empty());
    assert_eq!(inherited.fill.as_deref(), Some("red"));
}

#[test]
fn noncontiguous_group_preserves_selected_order_and_warns_before_commit() {
    let (mut p, mut r) = project("noncontiguous");
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("a", 10.0),
                index: None,
            },
            SceneOp::Insert {
                node: point("middle", 20.0),
                index: None,
            },
            SceneOp::Insert {
                node: point("b", 30.0),
                index: None,
            },
            SceneOp::Insert {
                node: point("last", 40.0),
                index: None,
            },
        ],
    );
    let before = p.content_baseline();
    let plan = preview_batch(
        &p,
        r,
        batch(
            &p,
            r,
            vec![SceneOp::Group {
                group_id: "g".into(),
                node_ids: vec!["b".into(), "a".into()],
                name: "组".into(),
            }],
        ),
    )
    .unwrap();
    assert_eq!(before, p.content_baseline());
    assert!(plan
        .diagnostics
        .iter()
        .any(|d| d.code == "SCENE_GROUP_REORDER"));
    assert!(plan.affected_nodes.contains(&"middle".into()));
    apply_batch(&mut p, &mut r, &plan).unwrap();
    let s = scene(&p);
    assert_eq!(s.root_order["places"], vec!["middle", "g", "last"]);
    assert!(
        matches!(&s.nodes["g"].geometry,SceneGeometry::Group{children} if children==&vec!["a".to_string(),"b".to_string()])
    );
}

#[test]
fn deleting_an_empty_scene_layer_removes_its_root_order_entry() {
    let (mut p, mut r) = project("empty-layer");
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("a", 10.0),
                index: None,
            },
            SceneOp::Delete {
                node_ids: vec!["a".into()],
            },
        ],
    );
    let command = presentation_commands::CommandEnvelope {
        expected_revision: r,
        expected_documents: baseline(&p),
        command: presentation_commands::Command::DeleteLayer {
            map_id: "map".into(),
            layer_id: "places".into(),
        },
    };
    presentation_commands::apply(&mut p, &mut r, command).unwrap();
    let index = p.map_index();
    assert!(index.maps.contains_key("map"), "{:?}", index.diagnostics);
    assert!(!scene(&p).root_order.contains_key("places"));
}

#[test]
fn public_svg_omits_unselected_layers_and_private_clip_numbering() {
    let (mut p, mut r) = project("public-skeleton");
    let create = presentation_commands::CommandEnvelope {
        expected_revision: r,
        expected_documents: baseline(&p),
        command: presentation_commands::Command::CreateLayer {
            map_id: "map".into(),
            layer_id: "private_layer".into(),
            title: "私有层".into(),
            visible_default: true,
            locked: false,
        },
    };
    presentation_commands::apply(&mut p, &mut r, create).unwrap();
    let mut private = SceneNode::new(
        "a_private",
        "private_layer",
        SceneGeometry::Group {
            children: Vec::new(),
        },
    );
    private.extra.insert("svg_root".into(), true.into());
    private.clip_rect = Some([0.0, 0.0, 100.0, 100.0]);
    let mut public = SceneNode::new(
        "z_public",
        "places",
        SceneGeometry::Group {
            children: Vec::new(),
        },
    );
    public.extra.insert("svg_root".into(), true.into());
    public.clip_rect = Some([0.0, 0.0, 100.0, 100.0]);
    let mut child = point("child", 10.0);
    child.parent_id = Some("z_public".into());
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: private,
                index: None,
            },
            SceneOp::Insert {
                node: public,
                index: None,
            },
            SceneOp::Insert {
                node: child,
                index: None,
            },
        ],
    );
    let selected = BTreeSet::from(["child".into()]);
    let m = map(&p);
    let layers = to_safe_svg_layers_with_links(&m, Some(&selected), &BTreeMap::new()).unwrap();
    assert_eq!(layers.keys().cloned().collect::<Vec<_>>(), vec!["places"]);
    let svg = to_safe_svg(&m, Some(&selected)).unwrap();
    assert!(svg.contains("wl-viewport-0"));
    assert!(!svg.contains("wl-viewport-1"));
    assert!(!svg.contains("private"));
}

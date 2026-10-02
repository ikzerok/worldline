use super::*;
fn old(p: &mut Project, r: &mut Revision, id: &str, x: f64) {
    let envelope = presentation_commands::CommandEnvelope {
        expected_revision: *r,
        expected_documents: baseline(p),
        command: presentation_commands::Command::CreatePlacement {
            map_id: "map".into(),
            placement_id: id.into(),
            layer_id: "places".into(),
            target_ref: None,
            geometry: worldline_core::MapGeometry::Point { position: [x, 0.5] },
            annotation: "说明".into(),
            role: "reference".into(),
            label_override: Some("自定义".into()),
        },
    };
    presentation_commands::apply(p, r, envelope).unwrap();
}

#[test]
fn migration_preserves_identity_metadata_order_unknowns_and_undo() {
    let (mut p, mut r) = project("migration");
    old(&mut p, &mut r, "a", 0.1);
    old(&mut p, &mut r, "b", 0.2);
    old(&mut p, &mut r, "c", 0.3);
    let file = path(&p);
    let mut raw: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&file).unwrap().bytes()).unwrap();
    raw["placements"]["b"]["custom"] = serde_json::json!({"keep":true});
    raw["placements"]["b"]["geometry"]["geometry_extra"] = 9.into();
    p.set_authoring_document(&file, serde_json::to_vec(&raw).unwrap())
        .unwrap();
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("q", 90.0),
                index: None,
            },
        ],
    );
    let before = p.content_baseline();
    assert_eq!(
        preview_batch(
            &p,
            r,
            batch(
                &p,
                r,
                vec![SceneOp::MigratePlacements {
                    node_ids: vec!["b".into()]
                }]
            )
        )
        .unwrap_err()
        .code,
        "SCENE_MIGRATION_ORDER"
    );
    assert_eq!(before, p.content_baseline());
    let result = apply(
        &mut p,
        &mut r,
        vec![SceneOp::MigratePlacements {
            node_ids: vec!["b".into(), "c".into()],
        }],
    );
    let m = map(&p);
    assert_eq!(m.placements.len(), 1);
    let s = m.scene.unwrap();
    assert_eq!(s.root_order["places"], vec!["b", "c", "q"]);
    assert_eq!(s.nodes["b"].annotation, "说明");
    assert_eq!(s.nodes["b"].label_override.as_deref(), Some("自定义"));
    assert_eq!(s.nodes["b"].extra["custom"]["keep"], true);
    assert!(matches!(
        s.nodes["b"].geometry,
        SceneGeometry::Point {
            position: [40.0, 50.0]
        }
    ));
    let raw: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&file).unwrap().bytes()).unwrap();
    assert_eq!(raw["scene"]["nodes"]["b"]["geometry"]["geometry_extra"], 9);
    assert!(raw["placements"].get("b").is_none());
    p.save().unwrap();
    assert!(scene(&Project::open(&p.root).unwrap())
        .nodes
        .contains_key("b"));
    let expected = r;
    presentation_commands::undo(&mut p, &mut r, expected, &result.undo_record).unwrap();
    assert!(map(&p).placements.contains_key("b"));
    assert!(!scene(&p).nodes.contains_key("b"));
}

#[test]
fn whole_map_export_preserves_legacy_and_scene_in_layer_order() {
    let (mut p, mut r) = project("export");
    old(&mut p, &mut r, "legacy", 0.25);
    assert!(map_to_safe_svg(&map(&p), None).unwrap().contains("<circle"));
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("vector", 80.0),
                index: None,
            },
        ],
    );
    let output = map_to_safe_svg(&map(&p), None).unwrap();
    assert_eq!(output.matches("<circle").count(), 2);
    assert!(output.find("cx=\"50\"").unwrap() < output.find("cx=\"80\"").unwrap());
}

#[test]
fn migrated_id_keeps_comments_references_and_safe_refactor_attached() {
    use worldline_core::collaboration::{
        self, AnchorStatus, CommentAnchor, CommentCommand, CommentDraft,
    };
    use worldline_core::TargetRef;
    let (mut p, mut r) = project("refs");
    let manifest = p.root.join(".world/project.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&manifest).unwrap().bytes()).unwrap();
    value["language_version"] = "1.10".into();
    p.set_authoring_document(&manifest, serde_json::to_vec(&value).unwrap())
        .unwrap();
    let entry = p.entry.clone();
    let original = p.document(&entry).unwrap().to_owned();
    p.set_text(&entry,format!("{original}\nentity tower kind place as \"tower\"\n  description \"tower 原文不得改\"\n")).unwrap();
    assert!(
        !compile_snapshot(&p).has_errors(),
        "{:?}",
        compile_snapshot(&p).diagnostics
    );
    old(&mut p, &mut r, "marker", 0.2);
    let file = path(&p);
    let mut value: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&file).unwrap().bytes()).unwrap();
    value["placements"]["marker"]["target_ref"] =
        serde_json::json!({"kind":"entity","id":"tower","optional":"tower"});
    value["placements"]["marker"]["scope_refs"] =
        serde_json::json!([{"kind":"entity","id":"tower","extra":"tower"}]);
    value["placements"]["marker"]["label_override"] = "tower".into();
    p.set_authoring_document(&file, serde_json::to_vec_pretty(&value).unwrap())
        .unwrap();
    let anchor = CommentAnchor::MapPlacement {
        map_id: "map".into(),
        placement_id: "marker".into(),
    };
    let expected_revision = r;
    let expected_baseline = p.content_baseline();
    collaboration::write_comment(
        &mut p,
        &mut r,
        CommentCommand {
            expected_revision,
            expected_baseline,
            original: None,
            draft: CommentDraft {
                id: "note".into(),
                author: "作者".into(),
                body: "批注".into(),
                anchor: anchor.clone(),
                resolved: false,
            },
        },
    )
    .unwrap();
    let comment_index =
        collaboration::build_comment_index(&p, &compile_snapshot(&p), &p.map_index());
    let comments_before = comment_index
        .comments
        .values()
        .map(|comment| {
            (
                comment.path.clone(),
                p.authoring_document(&comment.path)
                    .unwrap()
                    .bytes()
                    .to_vec(),
            )
        })
        .collect::<Vec<_>>();
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::MigratePlacements {
                node_ids: vec!["marker".into()],
            },
        ],
    );
    let compiled = compile_snapshot(&p);
    assert_eq!(
        collaboration::comment_anchor_status(&p, &compiled, &p.map_index(), &anchor),
        AnchorStatus::Attached
    );
    for (path, bytes) in comments_before {
        assert_eq!(p.authoring_document(&path).unwrap().bytes(), bytes);
    }
    let impact = p.deletion_impact(&TargetRef::new("entity", "tower"));
    assert!(impact.complete, "{:?}", impact.diagnostics);
    assert_eq!(impact.map_scopes.len(), 1);
    let rename = p
        .plan_rename_target(&TargetRef::new("entity", "tower"), "beacon")
        .unwrap();
    p.apply_rename_plan(&rename).unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&file).unwrap().bytes()).unwrap();
    let node = &value["scene"]["nodes"]["marker"];
    assert_eq!(node["target_ref"]["id"], "beacon");
    assert_eq!(node["scope_refs"][0]["id"], "beacon");
    assert_eq!(node["target_ref"]["optional"], "tower");
    assert_eq!(node["scope_refs"][0]["extra"], "tower");
    assert_eq!(node["label_override"], "tower");
    assert!(p.document(&entry).unwrap().contains("tower 原文不得改"));
    p.save().unwrap();
    let p = Project::open(&p.root).unwrap();
    assert_eq!(
        collaboration::comment_anchor_status(&p, &compile_snapshot(&p), &p.map_index(), &anchor),
        AnchorStatus::Attached
    );
}

use super::*;

#[test]
fn all_three_slots_match_independent_core_projection_and_preview_dtos() {
    let source = "let ready = true\r\nevent start after ready\r\n  choice once \"出发🙂\" if true enable ready disabled \"暂不可选\" // 留下说明\r\n    -> END\r\n";
    let manifest = "{\"schema_version\":1,\"language_version\":\"1.12\",\"required_features\":[],\"author_note\":\"keep\"}\n";
    for (slot, line) in [
        ("choice_display", json!(3)),
        ("choice_enable", json!(3)),
        ("event_after", Value::Null),
    ] {
        let mut fixture = Fixture::with_source(source, Some(manifest));
        // 与正式 project.open 同生命周期：open 的首次 refresh 装载文档并推进代次，
        // 然后 compile；open_read_only 的 snapshot 代次为 0，不能冒充同一打开流程。
        let mut core = Project::open(&fixture.root).unwrap();
        let compiled = core.compile();
        assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
        let selected = json!({"schema_version":1,"owner":{"target":{"kind":"event","id":"start"},"choice_line":line},"slot":slot});
        let core_selector = parse_condition_selector(&selected.to_string()).unwrap();
        let projection = core.condition_authoring(&core_selector).unwrap();
        let generation = projection.generation;
        let query = rpc(
            &mut fixture.server,
            "condition.authoring.query",
            json!({"project_id":fixture.id,"selector":selected}),
        );
        // 同一路径、同一原稿，身份/来源/诊断/基线全量相等，无需删除字段。
        assert_eq!(
            query["result"],
            json!({"ok":true,"projection":projection,
            "baseline":core.content_baseline(),"applied":false,"saved":false,"error":null})
        );
        assert_eq!(query["result"]["projection"]["selector"], selected);
        assert_eq!(query["result"]["projection"]["support"], "supported");
        let request = json!({"schema_version":1,"expected_baseline":projection.baseline,
            "expected_generation":projection.generation,"selector":selected,
            "replacement":{"kind":"bool","value":false}});
        let core_request = parse_condition_authoring_request(&request.to_string()).unwrap();
        let plan = core.preview_condition_authoring(&core_request).unwrap();
        let preview = fixture.preview(&request);
        assert_eq!(
            preview["result"],
            json!({"ok":true,"operation":"preview","plan":plan,
            "baseline":core.content_baseline(),"applied":false,"saved":false,"error":null})
        );
        assert!(plan.changed);
        assert_eq!(fixture.memory(), source);
        assert_eq!(
            core.document(&fixture.root.join("world.wl")).unwrap(),
            source
        );
        assert_eq!(
            fs::read(fixture.root.join("world.wl")).unwrap(),
            source.as_bytes()
        );
        let core_applied = core
            .apply_condition_authoring(&core_request, &plan.plan_digest)
            .unwrap();
        assert_eq!(json!(core_applied), json!(plan));
        let applied = fixture.apply(&request, &json!(plan.plan_digest));
        assert_eq!(
            applied["result"],
            json!({"ok":true,"operation":"apply","plan":core_applied,
            "baseline":core.content_baseline(),"applied":true,"saved":false,"error":null})
        );
        let patch = plan.patch.unwrap();
        let mut expected = source.to_owned();
        expected.replace_range(patch.byte_range, &patch.after);
        assert_eq!(fixture.memory(), expected);
        assert_eq!(
            core.document(&fixture.root.join("world.wl")).unwrap(),
            expected
        );
        assert_eq!(
            fs::read(fixture.root.join("world.wl")).unwrap(),
            source.as_bytes()
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join(".world/project.json")).unwrap(),
            manifest
        );
        let reopened = rpc(
            &mut fixture.server,
            "condition.authoring.query",
            json!({"project_id":fixture.id,"selector":selected}),
        );
        let core_after = core.condition_authoring(&core_selector).unwrap();
        assert_eq!(core_after.generation, generation);
        assert_eq!(
            reopened["result"],
            json!({"ok":true,"projection":core_after,
            "baseline":core.content_baseline(),"applied":false,"saved":false,"error":null})
        );
        assert_eq!(reopened["result"]["projection"]["generation"], generation);
        assert_eq!(reopened["result"]["projection"]["selector"], selected);
        assert_eq!(
            reopened["result"]["projection"]["tree"],
            json!(core_request.replacement)
        );
        assert_eq!(
            reopened["result"]["baseline"],
            applied["result"]["baseline"]
        );
        assert_eq!(
            fixture.server.projects[&fixture.id]
                .scene_revision
                .content_generation,
            1
        );
    }
}

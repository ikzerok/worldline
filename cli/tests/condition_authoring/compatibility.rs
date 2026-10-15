use super::*;

#[test]
fn failed_save_reports_applied_memory_and_read_only_query_does_not_recover_journal() {
    let fixture = Fixture::new();
    let request = fixture.request();
    let (_, preview) = fixture.run("preview", request_args(&request));
    let args = fixture.arguments(
        "apply",
        apply_args(
            &request,
            preview["plan"]["plan_digest"].as_str().unwrap(),
            true,
        ),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_wl"))
        .args(args)
        .env("WORLDLINE_SAVE_FAIL_PHASE", "first")
        .env_remove("WORLDLINE_SAVE_FAIL_THREAD")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let failure: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(failure["error"]["code"], "SAVE_FAILED", "{failure}");
    assert_eq!(failure["stage"], "save");
    assert_eq!(failure["applied"], true);
    assert_eq!(failure["saved"], false);
    assert!(failure["notice"].as_str().unwrap().contains("不能视为回滚"));
    let journals: Vec<_> = fs::read_dir(fixture.0.join(".world/.transactions"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("journal.json"))
        .collect();
    assert_eq!(journals.len(), 1);
    let original_journal = fs::read(&journals[0]).unwrap();
    let original_source = fs::read(fixture.0.join("world.wl")).unwrap();
    let (code, refused) = fixture.run(
        "query",
        vec!["--selector-json".into(), selector().to_string()],
    );
    assert_eq!(code, 2, "{refused}");
    assert_eq!(refused["error"]["code"], "IO_ERROR");
    assert_eq!(refused["applied"], false);
    assert_eq!(fs::read(&journals[0]).unwrap(), original_journal);
    assert_eq!(
        fs::read(fixture.0.join("world.wl")).unwrap(),
        original_source
    );
}

#[test]
fn explicit_enable_and_event_after_slots_use_core_patches_and_preserve_other_fields() {
    let source = "let ready = true\r\nevent start after ready\r\n  choice once \"出发🙂\" if true enable ready disabled \"暂不可选\" // 留下说明\r\n    -> END\r\n";
    let manifest = "{\"schema_version\":1,\"language_version\":\"1.12\",\"required_features\":[],\"author_note\":\"keep\"}\n";
    for (slot, line) in [
        ("choice_display", json!(3)),
        ("choice_enable", json!(3)),
        ("event_after", Value::Null),
    ] {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("world.wl"), source).unwrap();
        fs::create_dir(fixture.0.join(".world")).unwrap();
        fs::write(fixture.0.join(".world/project.json"), manifest).unwrap();
        let selected = json!({"schema_version":1,"owner":{"target":{"kind":"event","id":"start"},"choice_line":line},"slot":slot});
        let (code, query) = fixture.run(
            "query",
            vec!["--selector-json".into(), selected.to_string()],
        );
        assert_eq!(code, 0, "{query}");
        let project = fixture.project();
        let core_projection = project
            .condition_authoring(&parse_condition_selector(&selected.to_string()).unwrap())
            .unwrap();
        assert_eq!(query["projection"], json!(core_projection));
        assert_eq!(query["baseline"], project.content_baseline());
        assert_eq!(query["projection"]["support"], "supported");
        let request = json!({"schema_version":1,"expected_baseline":query["baseline"],
            "expected_generation":query["projection"]["generation"],"selector":selected,
            "replacement":{"kind":"bool","value":false}});
        let core = fixture
            .project()
            .preview_condition_authoring(
                &parse_condition_authoring_request(&request.to_string()).unwrap(),
            )
            .unwrap();
        let (code, preview) = fixture.run("preview", request_args(&request));
        assert_eq!(code, 0, "{preview}");
        assert_eq!(preview["plan"], json!(core));
        let (code, applied) = fixture.run("apply", apply_args(&request, &core.plan_digest, true));
        assert_eq!(code, 0, "{applied}");
        let patch = core.patch.unwrap();
        let mut expected = source.to_owned();
        expected.replace_range(patch.byte_range, &patch.after);
        assert_eq!(
            fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
            expected
        );
        assert_eq!(
            fs::read_to_string(fixture.0.join(".world/project.json")).unwrap(),
            manifest
        );
    }
}

#[test]
fn empty_slot_and_old_language_enable_are_readable_but_cannot_be_inserted() {
    let fixture = Fixture::new();
    for (slot, line, support) in [
        ("event_after", Value::Null, "empty"),
        ("choice_enable", json!(4), "version_unsupported"),
    ] {
        let selected = json!({"schema_version":1,"owner":{"target":{"kind":"event","id":"start"},"choice_line":line},"slot":slot});
        let (code, query) = fixture.run(
            "query",
            vec!["--selector-json".into(), selected.to_string()],
        );
        assert_eq!(code, 0, "{query}");
        assert_eq!(query["projection"]["support"], support);
        let request = json!({"schema_version":1,"expected_baseline":query["baseline"],
            "expected_generation":query["projection"]["generation"],"selector":selected,
            "replacement":{"kind":"bool","value":true}});
        let (code, failed) = fixture.run("preview", request_args(&request));
        assert_eq!(code, 1, "{failed}");
        assert_eq!(failed["applied"], false);
        assert_eq!(failed["error"]["code"], "CONDITION_AUTHORING_REJECTED");
    }
    assert_eq!(
        fs::read(fixture.0.join("world.wl")).unwrap(),
        SOURCE.as_bytes()
    );
    assert!(!fixture.0.join(".world/project.json").exists());
}

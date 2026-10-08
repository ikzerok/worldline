use super::*;

#[test]
fn explicit_overlay_is_read_only_and_rebuilds_same_plan_for_separate_save() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("world.wl"), DISK).unwrap();
    let input = ReconciliationInput {
        schema_version: 1,
        files: vec![ReconciliationInputFile {
            path: "world.wl".into(),
            baseline: Some(BASE.as_bytes().to_vec()),
            local: Some(LOCAL.as_bytes().to_vec()),
        }],
    };
    let mut project = Project::open_reconciliation_input(&fixture.root, &input).unwrap();
    let expected = plan(
        &project,
        ReconciliationChoice::Manual {
            text: MERGED.into(),
        },
    );
    assert!(expected.can_apply, "{:?}", expected.blockers);
    assert_eq!(fs::read(&project.entry).unwrap(), DISK.as_bytes());
    let second = Project::open_reconciliation_input(&fixture.root, &input).unwrap();
    let current = plan(
        &second,
        ReconciliationChoice::Manual {
            text: MERGED.into(),
        },
    );
    assert_eq!(expected.plan_digest, current.plan_digest);
    project.apply_reconciliation(&expected).unwrap();
    assert_eq!(fs::read(&project.entry).unwrap(), DISK.as_bytes());
    project.save().unwrap();
    assert_eq!(fs::read(&project.entry).unwrap(), MERGED.as_bytes());
}

#[test]
fn overlay_never_takes_over_unregistered_json_or_trusts_disk_bytes() {
    let fixture = Fixture::new();
    let input = ReconciliationInput {
        schema_version: 1,
        files: vec![ReconciliationInputFile {
            path: "ordinary.json".into(),
            baseline: None,
            local: Some(br#"{"schema_version":1}"#.to_vec()),
        }],
    };
    assert!(Project::open_reconciliation_input(&fixture.root, &input).is_err());
    assert!(!fixture.root.join("ordinary.json").exists());
}

#[test]
fn source_input_dto_rejects_unknown_fields_and_preserves_missing_empty() {
    let input: ReconciliationInput = serde_json::from_str(
        r#"{"schema_version":1,"files":[{"path":"world.wl","baseline":null,"local":[] }]}"#,
    )
    .unwrap();
    assert_eq!(input.files[0].baseline, None);
    assert_eq!(input.files[0].local, Some(Vec::new()));
    assert!(serde_json::from_str::<ReconciliationInput>(
        r#"{"schema_version":1,"files":[],"disk":[]}"#
    )
    .is_err());
}

#[test]
fn direct_rust_paths_are_bounded_before_recursive_canonicalization() {
    let fixture = Fixture::new();
    let project = fixture.conflict();
    let session = project.capture_reconciliation().unwrap();
    for path in [
        format!("{}.wl", "a".repeat(4096)),
        format!("{}file.wl", "a/".repeat(129)),
    ] {
        let input = ReconciliationRequest {
            choices: vec![ReconciliationDecision {
                path: path.clone().into(),
                choice: ReconciliationChoice::Delete,
            }],
            allow_incomplete_source: false,
        };
        let error = project
            .preview_reconciliation(&session, &input)
            .unwrap_err();
        assert!(error.contains("路径超过"), "{error}");
        let overlay = ReconciliationInput {
            schema_version: 1,
            files: vec![ReconciliationInputFile {
                path: path.clone().into(),
                baseline: None,
                local: Some(Vec::new()),
            }],
        };
        assert!(Project::open_reconciliation_input(&fixture.root, &overlay).is_err());
        assert!(Project::open_reconciliation_input(
            Path::new(&path),
            &ReconciliationInput {
                schema_version: 1,
                files: vec![]
            }
        )
        .is_err());
    }
    assert_eq!(project.document(&project.entry).unwrap(), LOCAL);
    assert_eq!(fs::read(&project.entry).unwrap(), DISK.as_bytes());
}

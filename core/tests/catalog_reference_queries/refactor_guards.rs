use super::*;

#[test]
fn valid_v1_and_v2_saved_queries_are_not_upgraded_by_reads_or_reference_rename() {
    for sorted in [false, true] {
        let mut project = fixture();
        let mut old_query = query(json!({"type":"string","value":"shared"}));
        if sorted {
            old_query.set_sort(Some(CatalogQuerySort {
                field: CatalogSortField::Name,
                direction: CatalogSortDirection::Ascending,
            }));
        }
        let version = if sorted { 2 } else { 1 };
        project
            .save_saved_query(draft(old_query.clone()), &project.content_baseline())
            .unwrap();
        let path = project.root.join(".world/queries/destinations.json");
        let before = project.authoring_document(&path).unwrap().bytes().to_vec();
        let baseline = project.content_baseline();
        let loaded = project.saved_query_index();
        assert!(loaded.diagnostics.is_empty());
        assert_eq!(
            loaded.queries["destinations"].draft.query.schema_version,
            version
        );
        assert_eq!(
            ids(&project, &loaded.queries["destinations"].draft.query),
            ["record_d"]
        );
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(project.authoring_document(&path).unwrap().bytes(), before);
        let plan = project
            .plan_rename_target(&TargetRef::new("entity", "shared"), "renamed")
            .unwrap();
        project.apply_rename_plan(&plan).unwrap();
        assert_eq!(project.authoring_document(&path).unwrap().bytes(), before);
        assert_eq!(
            project.saved_query_index().queries["destinations"]
                .draft
                .query,
            old_query
        );
        fs::remove_dir_all(project.root).unwrap();
    }
}

#[test]
fn uninterpretable_saved_queries_never_authorize_a_partial_reference_rename() {
    for mode in ["version", "capability", "value_type", "future_feature"] {
        let mut project = fixture();
        project
            .save_saved_query(
                draft(query(reference("entity", "shared"))),
                &project.content_baseline(),
            )
            .unwrap();
        let path = project.root.join(".world/queries/destinations.json");
        let mut document: Value =
            serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
        match mode {
            "version" => document["query"]["schema_version"] = json!(99),
            "capability" => document["required_features"] = json!([]),
            "value_type" => {
                document["query"]["filters"][1]["values"][0]["equals"]["type"] =
                    json!("future_reference")
            }
            "future_feature" => document["required_features"]
                .as_array_mut()
                .unwrap()
                .push(json!("future.query.v9")),
            _ => unreachable!(),
        }
        if mode == "future_feature" {
            project.save().unwrap();
            fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
            project = Project::open(&project.root).unwrap();
        } else {
            project
                .set_authoring_document(&path, serde_json::to_vec_pretty(&document).unwrap())
                .unwrap();
        }
        let sources = project.sources();
        let baseline = project.content_baseline();
        let before = project.authoring_document(&path).unwrap().bytes().to_vec();
        let error = project
            .plan_rename_target(&TargetRef::new("entity", "shared"), "renamed")
            .unwrap_err();
        assert!(error.contains("查询定义"), "{mode}: {error}");
        assert_eq!(project.sources(), sources);
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(project.authoring_document(&path).unwrap().bytes(), before);
        fs::remove_dir_all(project.root).unwrap();
    }
}

#[test]
fn stale_and_external_reference_rename_plans_preserve_all_current_drafts() {
    let mut project = fixture();
    project
        .save_saved_query(
            draft(query(reference("entity", "shared"))),
            &project.content_baseline(),
        )
        .unwrap();
    project.save().unwrap();
    let path = project.root.join(".world/queries/destinations.json");
    let source = project.root.join("world.wl");
    let plan = project
        .plan_rename_target(&TargetRef::new("entity", "shared"), "renamed")
        .unwrap();
    project
        .set_text(
            &source,
            format!("{}// 未保存的新稿\n", project.document(&source).unwrap()),
        )
        .unwrap();
    let sources = project.sources();
    let before = project.authoring_document(&path).unwrap().bytes().to_vec();
    let baseline = project.content_baseline();
    assert!(project.apply_rename_plan(&plan).is_err());
    assert_eq!(project.sources(), sources);
    assert_eq!(project.authoring_document(&path).unwrap().bytes(), before);
    assert_eq!(project.content_baseline(), baseline);
    let current_plan = project
        .plan_rename_target(&TargetRef::new("entity", "shared"), "renamed")
        .unwrap();
    fs::write(&path, b"external query update").unwrap();
    assert!(project.apply_rename_plan(&current_plan).is_err());
    assert_eq!(project.sources(), sources);
    assert_eq!(project.authoring_document(&path).unwrap().bytes(), before);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(fs::read(&path).unwrap(), b"external query update");
    fs::remove_dir_all(project.root).unwrap();
}

use super::common::*;

#[test]
fn markdown_import_cli_requires_review_digest_and_explicit_loss_confirmation() {
    let (project, source) = temp_markdown_import_fixture("review-apply");
    let baseline = worldline_core::project::Project::open(&project)
        .unwrap()
        .content_baseline();
    let preview_args = vec![
        "markdown".into(),
        "import".into(),
        "preview".into(),
        project.to_string_lossy().into_owned(),
        "--source".into(),
        source.to_string_lossy().into_owned(),
        "--baseline".into(),
        baseline.clone(),
        "--json".into(),
    ];
    let (code, preview) = run_dynamic(preview_args);
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["operation"], "preview");
    assert_eq!(preview["plan"]["can_apply"], false);
    assert!(!project.join(".world/markdown-imports").exists());

    let digest = preview["plan"]["plan_digest"].as_str().unwrap();
    let mut apply_args = vec![
        "markdown".into(),
        "import".into(),
        "apply".into(),
        project.to_string_lossy().into_owned(),
        "--source".into(),
        source.to_string_lossy().into_owned(),
        "--baseline".into(),
        baseline.clone(),
        "--plan-digest".into(),
        digest.to_string(),
        "--json".into(),
    ];
    let (code, rejected) = run_dynamic(apply_args.clone());
    assert_eq!(code, 1, "{rejected}");
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["error"]["code"], "CONFIRMATION_REQUIRED");
    assert!(!project.join(".world/markdown-imports").exists());

    apply_args.push("--accept-losses".into());
    let (code, applied) = run_dynamic(apply_args);
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["operation"], "apply");
    assert_eq!(applied["plan"]["can_apply"], true);
    assert!(!applied["changed_files"].as_array().unwrap().is_empty());
    assert_eq!(applied["baseline"], baseline);
    assert_eq!(
        applied["new_baseline"],
        worldline_core::project::Project::open(&project)
            .unwrap()
            .content_baseline()
    );
    assert!(project.join(".world/markdown-imports").exists());
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

#[test]
fn markdown_import_cli_rejects_a_stale_source_without_writing() {
    let (project, source) = temp_markdown_import_fixture("stale-source");
    let baseline = worldline_core::project::Project::open(&project)
        .unwrap()
        .content_baseline();
    let (code, preview) = run_dynamic(vec![
        "markdown".into(),
        "import".into(),
        "preview".into(),
        project.to_string_lossy().into_owned(),
        "--source".into(),
        source.to_string_lossy().into_owned(),
        "--baseline".into(),
        baseline.clone(),
        "--json".into(),
    ]);
    assert_eq!(code, 0, "{preview}");
    std::fs::write(source.join("harbor.md"), "# Changed\nchanged **text**\n").unwrap();

    let (code, rejected) = run_dynamic(vec![
        "markdown".into(),
        "import".into(),
        "apply".into(),
        project.to_string_lossy().into_owned(),
        "--source".into(),
        source.to_string_lossy().into_owned(),
        "--baseline".into(),
        baseline,
        "--plan-digest".into(),
        preview["plan"]["plan_digest"].as_str().unwrap().into(),
        "--accept-losses".into(),
        "--json".into(),
    ]);
    assert_eq!(code, 1, "{rejected}");
    assert_eq!(rejected["error"]["code"], "STALE_PLAN");
    assert!(!project.join(".world/markdown-imports").exists());
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

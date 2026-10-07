use serde_json::{json, Value};
use std::fs;
use worldline_core::{presentation_commands::Revision, project::Project};
fn invoke(args: Vec<String>) -> (i32, Value) {
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::new())).unwrap();
    (code, serde_json::from_slice(&out).unwrap())
}
#[test]
fn chapter_cli_uses_same_plan_and_explicit_save_without_orphans() {
    let root =
        std::env::temp_dir().join(format!("wl-manuscript-chapter-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut project = Project::new(&root);
    project.save().unwrap();
    let request = json!({"schema_version":1,"expected_baseline":project.content_baseline(),"expected_revision":Revision::default(),
        "book":{"kind":"new","id":"book","title":"书稿"},"chapter":{"id":"one","title":"第一章"},
        "source":{"kind":"new_event","id":"first","storyline":"main","destination":{"kind":"new_active_source","relative_path":"chapter.wl"}}});
    let args = vec![
        "manuscript-chapter".into(),
        "preview".into(),
        root.to_string_lossy().into_owned(),
        "--request-json".into(),
        request.to_string(),
        "--json".into(),
    ];
    let (code, preview) = invoke(args.clone());
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["saved"], false);
    assert!(!root.join("chapter.wl").exists());
    let core_request = serde_json::from_value(request.clone()).unwrap();
    let core_plan = project
        .preview_manuscript_chapter_create(Revision::default(), &core_request)
        .unwrap();
    assert_eq!(preview["plan"], json!(core_plan));
    let mut apply = args.clone();
    apply[1] = "apply".into();
    apply.extend([
        "--plan-digest".into(),
        preview["plan"]["plan_digest"].as_str().unwrap().into(),
    ]);
    let (code, memory) = invoke(apply.clone());
    assert_eq!(code, 0, "{memory}");
    assert_eq!(memory["saved"], false);
    assert!(!root.join("chapter.wl").exists());
    apply.push("--save".into());
    let (code, saved) = invoke(apply.clone());
    assert_eq!(code, 0, "{saved}");
    assert_eq!(saved["saved"], true);
    assert_eq!(
        Project::open(&root)
            .unwrap()
            .manuscript_index("book")
            .unwrap()
            .entries
            .len(),
        1
    );
    let (code, repeated) = invoke(apply);
    assert_eq!(code, 1);
    assert_eq!(repeated["error"]["code"], "STALE_BASELINE");
    let mut invalid = args.clone();
    invalid[4] = request
        .to_string()
        .replacen("{", "{\"schema_version\":1,", 1);
    assert_eq!(invoke(invalid).0, 2);
    let mut invalid = args;
    invalid.push("--save".into());
    assert_eq!(invoke(invalid).0, 2);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn chapter_cli_will_not_recover_a_pending_journal_on_preview() {
    let root = std::env::temp_dir().join(format!("wl-chapter-cli-journal-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut project = Project::new(&root);
    project.save().unwrap();
    let request = json!({"schema_version":1,"expected_baseline":project.content_baseline(),"expected_revision":Revision::default(),
        "book":{"kind":"new","id":"b","title":"书"},"chapter":{"id":"c","title":"章"},"source":{"kind":"existing","target":{"kind":"event","id":"start"}}});
    fs::create_dir_all(root.join(".world/.transactions/pending")).unwrap();
    let (code, result) = invoke(vec![
        "manuscript-chapter".into(),
        "preview".into(),
        root.to_string_lossy().into_owned(),
        "--request-json".into(),
        request.to_string(),
        "--json".into(),
    ]);
    assert_eq!(code, 2);
    assert_eq!(result["error"]["code"], "IO_ERROR");
    assert!(root.join(".world/.transactions/pending").exists());
    assert!(!root.join(".world/manuscripts").exists());
    fs::remove_dir_all(root).unwrap();
}

use serde_json::{json, Value};
use std::{
    fs,
    io::Cursor,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::{localization::*, project::Project};

#[path = "localization_workbench/compatibility.rs"]
mod compatibility;

static NEXT: AtomicU64 = AtomicU64::new(0);
const SOURCE: &str = "let traveler = \"Ari\"\nevent start\n  Hello {traveler}! #wl-localization:greeting\n  choice \"Continue\" #wl-localization:go\n    -> END\n";

fn fixture(name: &str, source: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "v033-cli-locale-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.localization.v1"]}"#).unwrap();
    root
}

fn run(args: &[String], input: &str) -> (i32, Vec<Value>) {
    let mut output = Vec::new();
    let code = wl::run(args, &mut output, &mut Cursor::new(input)).unwrap();
    let text = String::from_utf8(output).unwrap();
    (
        code,
        text.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect(),
    )
}

fn author(
    root: &std::path::Path,
    kind: &str,
    stage: Option<&str>,
    request: Value,
    digest: Option<&str>,
    save: bool,
) -> (i32, Value) {
    let mut args = vec!["localization".into(), kind.into()];
    if let Some(stage) = stage {
        args.push(stage.into());
    }
    args.extend([
        root.to_string_lossy().into_owned(),
        "--request-json".into(),
        request.to_string(),
        "--json".into(),
    ]);
    if let Some(digest) = digest {
        args.extend(["--plan-digest".into(), digest.into()]);
    }
    if save {
        args.push("--save".into());
    }
    let (code, lines) = run(&args, "");
    assert_eq!(lines.len(), 1);
    (code, lines[0].clone())
}

fn translations(project: &Project) -> LocalizationEditDraft {
    let page = project
        .query_localization_catalog(&LocalizationCatalogQuery::default())
        .unwrap();
    LocalizationEditDraft {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh-Hans".into(),
        source_baseline: page.source_baseline,
        edits: page
            .entries
            .into_iter()
            .map(|entry| LocalizationEdit {
                id: entry.id.unwrap(),
                source_revision: entry.source_revision.unwrap(),
                translation_parts: entry
                    .source_parts
                    .into_iter()
                    .map(|part| match part {
                        LocalizationPart::Text { text } => LocalizationPart::Text {
                            text: if text == "Continue" {
                                "继续".into()
                            } else {
                                text.replace("Hello ", "你好，")
                            },
                        },
                        other => other,
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn localized(name: &str) -> PathBuf {
    let root = fixture(name, SOURCE);
    let mut project = Project::open(&root).unwrap();
    let draft = translations(&project);
    let plan = project.preview_localization_edit(&draft).unwrap();
    assert!(plan.can_apply);
    project
        .apply_localization_edit(&draft, &plan.plan_digest)
        .unwrap();
    project.save().unwrap();
    root
}

#[test]
fn catalog_and_id_assignment_keep_memory_apply_separate_from_disk() {
    let source = "event start\n  Unassigned 中文🙂\n  -> END\n";
    let root = fixture("ids", source);
    let (code, catalog) = author(
        &root,
        "catalog",
        None,
        json!(LocalizationCatalogQuery::default()),
        None,
        false,
    );
    assert_eq!(code, 0);
    assert_eq!(catalog["page"]["total"], 1);
    let entry = &catalog["page"]["entries"][0];
    assert_eq!(entry["status"], "missing_id");
    let request = json!({"schema_version":1,"source_baseline":catalog["page"]["source_baseline"],"assignments":[{
        "source":entry["source"],"source_revision":entry["source_revision"],"expected_id":null,"id":"opening"
    }]});
    let (code, preview) = author(&root, "ids", Some("preview"), request.clone(), None, false);
    assert_eq!(code, 0);
    assert_eq!(preview["plan"]["can_apply"], true);
    let digest = preview["plan"]["plan_digest"].as_str().unwrap();
    let (code, candidate) = author(
        &root,
        "ids",
        Some("apply"),
        request.clone(),
        Some(digest),
        false,
    );
    assert_eq!(code, 0);
    assert_eq!(candidate["applied"], true);
    assert_eq!(candidate["saved"], false);
    assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), source);
    let (code, saved) = author(&root, "ids", Some("apply"), request, Some(digest), true);
    assert_eq!(code, 0);
    assert_eq!(saved["saved"], true);
    assert!(fs::read_to_string(root.join("world.wl"))
        .unwrap()
        .contains("#wl-localization:opening"));
}

#[test]
fn typed_edit_rejects_stale_plan_and_persists_only_with_explicit_save() {
    let root = fixture("edit", SOURCE);
    let project = Project::open(&root).unwrap();
    let request = json!(translations(&project));
    let (_, preview) = author(&root, "edit", Some("preview"), request.clone(), None, false);
    let digest = preview["plan"]["plan_digest"].as_str().unwrap();
    assert_eq!(preview["plan"]["can_apply"], true);
    let (_, applied) = author(
        &root,
        "edit",
        Some("apply"),
        request.clone(),
        Some(digest),
        false,
    );
    assert_eq!(applied["applied"], true);
    assert_eq!(applied["saved"], false);
    assert!(!root.join(".world/localization/zh-Hans.json").exists());
    let (code, bad) = author(
        &root,
        "edit",
        Some("apply"),
        request.clone(),
        Some("stale"),
        true,
    );
    assert_eq!(code, 1);
    assert_eq!(bad["applied"], false);
    let (code, saved) = author(&root, "edit", Some("apply"), request, Some(digest), true);
    assert_eq!(code, 0);
    assert_eq!(saved["saved"], true);
    assert!(
        fs::read_to_string(root.join(".world/localization/zh-Hans.json"))
            .unwrap()
            .contains("你好")
    );
}

#[test]
fn localized_play_replay_report_and_comparison_use_real_runtime() {
    let root = localized("runtime");
    let trace_path = root.parent().unwrap().join(format!(
        "locale-trace-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let args = vec![
        "play".into(),
        root.to_string_lossy().into_owned(),
        "--json".into(),
        "--seed".into(),
        "71".into(),
    ];
    let (source_code, source) = run(&args, "0\n");
    assert_eq!(source_code, 0);
    assert_eq!(source[0]["outputs"][0]["content"], "Hello Ari!");
    assert!(source[0]["outputs"][0].get("localization").is_none());
    let mut translated_args = args.clone();
    translated_args.extend([
        "--locale".into(),
        "zh-Hans".into(),
        "--trace-output".into(),
        trace_path.to_string_lossy().into_owned(),
    ]);
    let (code, translated) = run(&translated_args, "0\n");
    assert_eq!(code, 0);
    assert_eq!(translated[0]["outputs"][0]["content"], "你好，Ari!");
    assert_eq!(
        translated[0]["outputs"][0]["localization"]["source_content"],
        "Hello Ari!"
    );
    assert_eq!(translated[0]["choices"][0]["label"], "继续");
    assert_eq!(translated[0]["state"], source[0]["state"]);
    let trace = fs::read_to_string(&trace_path).unwrap();
    let (code, replay) = run(
        &[
            "replay".into(),
            root.to_string_lossy().into_owned(),
            "--trace-json".into(),
            trace.clone(),
            "--json".into(),
        ],
        "",
    );
    assert_eq!(code, 0, "{replay:?}");
    let (code, report) = run(
        &[
            "playthrough-report".into(),
            root.to_string_lossy().into_owned(),
            "--trace-json".into(),
            trace.clone(),
            "--json".into(),
        ],
        "",
    );
    assert_eq!(code, 0, "{report:?}");
    assert_eq!(report[0]["ok"], true);
    let (code, comparison) = run(
        &[
            "route-compare".into(),
            root.to_string_lossy().into_owned(),
            "--left-trace-json".into(),
            trace.clone(),
            "--right-trace-json".into(),
            trace,
            "--json".into(),
        ],
        "",
    );
    assert_eq!(code, 0, "{comparison:?}");
    assert_eq!(comparison[0]["ok"], true);
}

#[test]
fn locale_argument_and_duplicate_json_rejections_are_not_silent() {
    let root = fixture("invalid", SOURCE);
    let mut out = Vec::new();
    let err = wl::run(
        &[
            "play".into(),
            root.to_string_lossy().into_owned(),
            "--locale-fallback".into(),
            "source".into(),
        ],
        &mut out,
        &mut Cursor::new(""),
    )
    .unwrap_err();
    assert!(err.contains("--locale"));
    let args = vec![
        "localization".into(),
        "catalog".into(),
        root.to_string_lossy().into_owned(),
        "--request-json".into(),
        "{\"schema_version\":1,\"schema_version\":1,\"limit\":50}".into(),
        "--json".into(),
    ];
    let (code, result) = run(&args, "");
    assert_eq!(code, 2);
    assert_eq!(result[0]["error"]["code"], "INVALID_PARAMS");
    let (code, failure) = run(
        &[
            "play".into(),
            root.to_string_lossy().into_owned(),
            "--locale".into(),
            "missing".into(),
            "--json".into(),
        ],
        "",
    );
    assert_eq!(code, 1);
    assert_eq!(failure[0]["type"], "run_error");
}

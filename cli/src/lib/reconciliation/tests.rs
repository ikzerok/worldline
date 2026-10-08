use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

#[test]
fn wire_budget_counts_byte_arrays_escaped_text_and_final_newline() {
    let bytes = vec![255u8; 10];
    assert_eq!(serde_json::to_vec(&bytes).unwrap().len(), 41);
    assert_eq!(budget::encode_limited(&bytes, 42).unwrap().len(), 42);
    assert!(budget::encode_limited(&bytes, 41).is_err());
    let escaped = "\0".repeat(100);
    assert!(budget::encode_limited(&escaped, 600).is_err());
    assert_eq!(budget::encode_limited(&escaped, 603).unwrap().len(), 603);
    let large = vec![255u8; budget::MAX_RESPONSE / 4];
    assert!(budget::encode(&large).is_err());
}

#[test]
fn whole_plan_output_limit_refuses_save_before_any_adoption_or_disk_write() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = std::env::temp_dir().join(format!(
        "reconciliation-cli-budget-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let root = parent.join("workspace");
    fs::create_dir_all(&root).unwrap();
    let source = root.join("world.wl");
    let base = b"event start\n  -> END\n".to_vec();
    let local = format!("event start\n  -> END\n//{}", "draft".repeat(1000)).into_bytes();
    let disk = b"event start\n  disk\n  -> END\n".to_vec();
    fs::write(&source, &disk).unwrap();
    let input = ReconciliationInput {
        schema_version: 1,
        files: vec![
            worldline_core::project::reconciliation::ReconciliationInputFile {
                path: "world.wl".into(),
                baseline: Some(base),
                local: Some(local),
            },
        ],
    };
    let request = ReconciliationRequest {
        choices: vec![
            worldline_core::project::reconciliation::ReconciliationDecision {
                path: "world.wl".into(),
                choice: worldline_core::project::reconciliation::ReconciliationChoice::Local,
            },
        ],
        allow_incomplete_source: false,
    };
    let project = Project::open_reconciliation_input(&root, &input).unwrap();
    let plan = project
        .preview_reconciliation(&project.capture_reconciliation().unwrap(), &request)
        .unwrap();
    assert!(plan.can_apply, "{:?}", plan.blockers);
    let input_path = parent.join("input.json");
    let request_path = parent.join("request.json");
    fs::write(&input_path, serde_json::to_vec(&input).unwrap()).unwrap();
    fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    let args = vec![
        "save".into(),
        root.display().to_string(),
        "--input".into(),
        input_path.display().to_string(),
        "--request".into(),
        request_path.display().to_string(),
        "--plan-digest".into(),
        plan.plan_digest,
        "--json".into(),
    ];
    let mut out = Vec::new();
    assert_eq!(execute(&args, &mut out, 4096).unwrap(), 1);
    let response: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(response["error"]["code"], "OUTPUT_LIMIT");
    assert_eq!(response["applied"], false);
    assert_eq!(fs::read(&source).unwrap(), disk);
    assert!(!root.join(".world/.transactions").exists());
    let _ = fs::remove_dir_all(parent);
}

use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, io::Cursor, path::PathBuf};

struct Fixture {
    parent: PathBuf,
    root: PathBuf,
    input: PathBuf,
    request: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::temp_dir().join(format!(
            "worldline-reconciliation-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = parent.join("workspace");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("world.wl"), "event start\n  磁盘\n  -> END\n").unwrap();
        let input = parent.join("input.json");
        let request = parent.join("request.json");
        fs::write(&input, json!({"schema_version":1,"files":[{"path":"world.wl","baseline":b"event start\n  base\n  -> END\n".to_vec(),"local":b"event start\n  local\n  -> END\n".to_vec()}]}).to_string()).unwrap();
        fs::write(
            &request,
            json!({"choices":[{"path":"world.wl","choice":{"kind":"local"}}]}).to_string(),
        )
        .unwrap();
        Self {
            parent,
            root,
            input,
            request,
        }
    }
    fn args(&self, operation: &str, digest: Option<&str>) -> Vec<String> {
        let mut args = vec![
            "reconciliation".into(),
            operation.into(),
            self.root.display().to_string(),
            "--input".into(),
            self.input.display().to_string(),
            "--json".into(),
        ];
        if operation != "capture" {
            args.extend(["--request".into(), self.request.display().to_string()]);
        }
        if let Some(digest) = digest {
            args.extend(["--plan-digest".into(), digest.into()]);
        }
        args
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.parent);
    }
}
fn run(args: &[String]) -> (i32, Value) {
    let mut out = Vec::new();
    let code = wl::run(args, &mut out, &mut Cursor::new("")).unwrap();
    (code, serde_json::from_slice(&out).unwrap())
}

#[test]
fn cli_preview_and_apply_do_not_save_and_separate_save_rechecks_same_plan() {
    let fixture = Fixture::new();
    let disk = fs::read(fixture.root.join("world.wl")).unwrap();
    let (code, preview) = run(&fixture.args("preview", None));
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["plan"]["can_apply"], true, "{preview}");
    let digest = preview["plan"]["plan_digest"].as_str().unwrap();
    let (code, applied) = run(&fixture.args("apply", Some(digest)));
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["applied"], true);
    assert_eq!(applied["saved"], false);
    assert_eq!(fs::read(fixture.root.join("world.wl")).unwrap(), disk);
    let (code, saved) = run(&fixture.args("save", Some(digest)));
    assert_eq!(code, 0, "{saved}");
    assert_eq!(saved["saved"], true);
    assert!(fs::read_to_string(fixture.root.join("world.wl"))
        .unwrap()
        .contains("local"));
}

#[test]
fn cli_new_external_change_or_forged_digest_refuses_without_writing() {
    let fixture = Fixture::new();
    let (_, preview) = run(&fixture.args("preview", None));
    let digest = preview["plan"]["plan_digest"].as_str().unwrap();
    assert_eq!(run(&fixture.args("save", Some("0000000000000000"))).0, 1);
    fs::write(fixture.root.join("world.wl"), "event latest\n  -> END\n").unwrap();
    assert_eq!(run(&fixture.args("save", Some(digest))).0, 1);
    assert!(fs::read_to_string(fixture.root.join("world.wl"))
        .unwrap()
        .contains("latest"));
}

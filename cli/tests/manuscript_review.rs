use serde_json::{json, Value};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::{manuscript::review_projection, project::Project, TargetRef};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "wl-review-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("world.wl"),
            "event start\n  if true\n    甲😀。\n  else\n    乙。\n  -> END\n",
        )
        .unwrap();
        Self(root)
    }
    fn run(&self, target: Value) -> (i32, Value) {
        let args = vec![
            "manuscript-review".into(),
            self.0.display().to_string(),
            "--target-json".into(),
            target.to_string(),
            "--json".into(),
        ];
        let mut out = Vec::new();
        let code = wl::run(&args, &mut out, &mut Cursor::new("")).unwrap();
        assert!(out.len() <= 1024 * 1024 + 4096);
        (code, serde_json::from_slice(&out).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cli_shares_core_projection_and_is_read_only() {
    let fixture = Fixture::new();
    let before = std::fs::read(fixture.0.join("world.wl")).unwrap();
    let project = Project::open_read_only(&fixture.0).unwrap();
    let compiled = project.compile_read_only().unwrap();
    let expected = review_projection(&compiled, &TargetRef::new("event", "start")).unwrap();
    let (code, value) = fixture.run(json!({"kind":"event","id":"start"}));
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["review"], json!(expected));
    assert_eq!(value["review"]["complete"], true);
    assert_eq!(std::fs::read(fixture.0.join("world.wl")).unwrap(), before);
    assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
}

#[test]
fn cli_errors_have_no_navigable_success_and_no_saves() {
    let fixture = Fixture::new();
    let (code, value) = fixture.run(json!({"kind":"event","id":"missing"}));
    assert_eq!(code, 1);
    assert_eq!(value["ok"], false);
    assert!(value["review"].is_null());
    let (code, _) = fixture.run(json!({"kind":"event","id":"start","extra":true}));
    assert_eq!(code, 2);
    std::fs::write(fixture.0.join("world.wl"), "event start\n  if (\n").unwrap();
    let (code, value) = fixture.run(json!({"kind":"event","id":"start"}));
    assert_eq!(code, 1);
    assert_eq!(value["error"]["code"], "compile_failed");
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
        "event start\n  if (\n"
    );
}

#[test]
fn cli_pending_transaction_is_not_recovered() {
    let fixture = Fixture::new();
    let transaction = fixture.0.join(".world/.transactions/save-pending");
    std::fs::create_dir_all(&transaction).unwrap();
    std::fs::write(transaction.join("marker"), "keep transaction bytes").unwrap();
    let (code, value) = fixture.run(json!({"kind":"event","id":"start"}));
    assert_ne!(code, 0, "{value}");
    assert_eq!(
        std::fs::read_to_string(transaction.join("marker")).unwrap(),
        "keep transaction bytes"
    );
}

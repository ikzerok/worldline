use std::{fs, io::Cursor, path::PathBuf};
use worldline_runtime::{decode_replay_trace, encode_replay_trace, MAX_REPLAY_EXCHANGE_BYTES};

struct Workspace(PathBuf);
impl Workspace {
    fn new(label: &str, source: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("wl-trace-exchange-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("story.wl"), source).unwrap();
        Self(root)
    }
    fn args(&self, exchange: bool, name: &str) -> Vec<String> {
        let mut args = vec![
            "play".into(),
            self.0.join("story.wl").display().to_string(),
            "--seed=31".into(),
            "--json".into(),
            format!("--trace-output={}", self.0.join(name).display()),
        ];
        if exchange {
            args.push("--trace-exchange".into());
        }
        args
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn explicit_cli_exchange_is_canonical_and_legacy_output_stays_pretty() {
    let w = Workspace::new(
        "normal",
        "event start\n  海雾。\n  choice \"继续\"\n    -> END\n",
    );
    for exchange in [false, true] {
        let name = if exchange {
            "exchange.json"
        } else {
            "legacy.json"
        };
        let mut output = Vec::new();
        assert_eq!(
            wl::run(
                &w.args(exchange, name),
                &mut output,
                &mut Cursor::new("0\n")
            )
            .unwrap(),
            0
        );
        let bytes = fs::read(w.0.join(name)).unwrap();
        let trace = decode_replay_trace(&bytes).unwrap();
        assert!(trace.complete);
        let compact = encode_replay_trace(&trace).unwrap();
        if exchange {
            assert_eq!(bytes, compact.as_bytes());
        } else {
            assert!(bytes.len() > compact.len());
            assert!(bytes.contains(&b'\n'));
        }
    }
    let args = [
        "check".to_owned(),
        w.0.join("story.wl").display().to_string(),
        "--trace-exchange".into(),
    ];
    assert!(wl::run(&args, &mut Vec::new(), &mut Cursor::new("")).is_err());
    let args = [
        "play".to_owned(),
        w.0.join("story.wl").display().to_string(),
        "--trace-exchange".into(),
    ];
    assert!(wl::run(&args, &mut Vec::new(), &mut Cursor::new(""))
        .unwrap_err()
        .contains("搭配"));
    let mut duplicate = w.args(true, "duplicate.json");
    duplicate.push("--trace-exchange".into());
    assert!(wl::run(&duplicate, &mut Vec::new(), &mut Cursor::new(""))
        .unwrap_err()
        .contains("重复"));
    assert!(!w.0.join("duplicate.json").exists());
    let existing = w.0.join("existing.json");
    fs::write(&existing, "已有目标").unwrap();
    assert!(wl::run(
        &w.args(true, "existing.json"),
        &mut std::io::sink(),
        &mut Cursor::new("0\n")
    )
    .is_err());
    assert_eq!(fs::read_to_string(existing).unwrap(), "已有目标");
}

#[test]
fn oversized_explicit_exchange_preserves_existing_target_and_default_still_exports() {
    let line = "雾".repeat(2000);
    let source = format!(
        "event start\n{}  -> END\n",
        format!("  {line}\n").repeat(750)
    );
    let w = Workspace::new("large", &source);
    let target = w.0.join("trace.json");
    fs::write(&target, b"previous trace bytes").unwrap();
    let error = wl::run(
        &w.args(true, "trace.json"),
        &mut std::io::sink(),
        &mut Cursor::new(""),
    )
    .unwrap_err();
    assert!(error.contains("上限"), "{error}");
    assert_eq!(fs::read(&target).unwrap(), b"previous trace bytes");
    assert!(wl::run(
        &w.args(true, "absent.json"),
        &mut std::io::sink(),
        &mut Cursor::new("")
    )
    .is_err());
    assert!(!w.0.join("absent.json").exists());
    assert_eq!(
        wl::run(
            &w.args(false, "trace.json"),
            &mut std::io::sink(),
            &mut Cursor::new("")
        )
        .unwrap(),
        0
    );
    assert!(fs::metadata(&target).unwrap().len() > MAX_REPLAY_EXCHANGE_BYTES as u64);
    assert!(
        serde_json::from_slice::<worldline_runtime::ReplayTrace>(&fs::read(&target).unwrap())
            .is_ok()
    );
    assert!(!fs::read_dir(&w.0).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".wl-trace-")));
}

#[test]
fn failed_exchange_delivery_preserves_destination_directory_and_cleans_staging() {
    let w = Workspace::new("write-failure", "event start\n  文字\n  -> END\n");
    fs::create_dir(w.0.join("target")).unwrap();
    fs::write(w.0.join("target/sentinel"), "保留").unwrap();
    assert!(wl::run(
        &w.args(true, "target"),
        &mut std::io::sink(),
        &mut Cursor::new("")
    )
    .is_err());
    assert_eq!(
        fs::read_to_string(w.0.join("target/sentinel")).unwrap(),
        "保留"
    );
    assert!(!fs::read_dir(&w.0).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".wl-trace-")));
}

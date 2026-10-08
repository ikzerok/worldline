use super::*;
use std::collections::BTreeMap;
use std::io::{BufRead, Read};
use std::path::Path;

struct InjectingReader {
    input: Cursor<Vec<u8>>,
    inject_after: u64,
    inject: Option<Box<dyn FnOnce()>>,
}
impl InjectingReader {
    fn inject_if_ready(&mut self) {
        if self.input.position() >= self.inject_after {
            if let Some(inject) = self.inject.take() {
                inject();
            }
        }
    }
}
impl Read for InjectingReader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.inject_if_ready();
        self.input.read(bytes)
    }
}
impl BufRead for InjectingReader {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.inject_if_ready();
        self.input.fill_buf()
    }
    fn consume(&mut self, amount: usize) {
        self.input.consume(amount);
    }
}
fn exchange(requests: Vec<Value>, inject: impl FnOnce() + 'static) -> Vec<Value> {
    let mut reader = InjectingReader {
        input: Cursor::new(
            requests
                .iter()
                .map(|request| format!("{request}\n"))
                .collect::<String>()
                .into_bytes(),
        ),
        inject_after: format!("{}\n", requests.first().expect("opening request")).len() as u64,
        inject: Some(Box::new(inject)),
    };
    let mut output = Vec::new();
    assert_eq!(worldline_agent::run(&mut reader, &mut output), 0);
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn query() -> Value {
    json!({"schema_version":1,"filters":[{"dimension":"kind","values":["entity"]}]})
}
fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut output = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        for item in std::fs::read_dir(dir).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                output.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    output
}
fn fingerprint(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
fn journal(root: &Path, before: &[u8], after: &[u8]) -> PathBuf {
    let path = root.join(".world/.transactions/scope-read-only/journal.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "version":1,"status":"prepared","files":[{"path":"world.wl",
            "before":fingerprint(before),"after":fingerprint(after),"payload":after}]
        }))
        .unwrap(),
    )
    .unwrap();
    path
}
#[test]
fn project_scope_never_recovers_a_real_pending_journal_or_changes_its_applied_snapshot() {
    let fixture = Fixture::new();
    let before = std::fs::read(fixture.0.join("world.wl")).unwrap();
    let after = [
        b"entity recovered kind place\n".as_slice(),
        before.as_slice(),
    ]
    .concat();
    let recorded = std::rc::Rc::new(std::cell::RefCell::new(None));
    let observed = recorded.clone();
    let root = fixture.0.clone();
    let before_inject = before.clone();
    let after_inject = after.clone();
    let rows = exchange(
        vec![
            request(1, "project.open", json!({"path":fixture.0})),
            request(
                2,
                "catalog.scope",
                json!({"project_id":"p1","query":query()}),
            ),
            request(
                3,
                "catalog.scope",
                json!({"project_id":"p1","query":query()}),
            ),
            request(
                4,
                "catalog.scope",
                json!({"path":fixture.0,"query":query()}),
            ),
        ],
        move || {
            journal(&root, &before_inject, &after_inject);
            *observed.borrow_mut() = Some(files(&root));
        },
    );
    assert!(
        recorded.borrow().is_some(),
        "journal injection must occur after the opening request"
    );
    assert_eq!(rows[1]["result"]["ok"], true, "{:?}", rows[1]);
    assert_eq!(rows[1]["result"]["scope"]["counts"]["matching_objects"], 2);
    assert_eq!(rows[1]["result"]["source_mode"], "applied_project_snapshot");
    assert_eq!(rows[1]["result"]["refreshed"], false);
    assert_eq!(rows[1]["result"]["conflicts"], Value::Null);
    assert_eq!(
        rows[1]["result"]["workspace_revision"],
        rows[0]["result"]["baseline"]
    );
    assert_eq!(rows[1]["result"]["scope"], rows[2]["result"]["scope"]);
    assert_eq!(
        rows[3]["result"]["ok"], false,
        "path read refuses the pending journal"
    );
    assert_eq!(rows[3]["result"]["error"]["code"], "IO_ERROR");
    assert_eq!(
        files(&fixture.0),
        recorded.borrow().as_ref().unwrap().clone()
    );
    assert_eq!(std::fs::read(fixture.0.join("world.wl")).unwrap(), before);
    // Positive control: exactly the same journal is valid and actually recoverable.
    let control = Fixture::new();
    let journal_path = journal(&control.0, &before, &after);
    let recovered = worldline_core::project::Project::open(&control.0).unwrap();
    assert_eq!(std::fs::read(control.0.join("world.wl")).unwrap(), after);
    assert!(recovered
        .document(&recovered.entry)
        .unwrap()
        .contains("entity recovered"));
    assert!(!journal_path.exists());
}
#[test]
fn external_edits_require_an_explicit_existing_analyze_before_scope_changes() {
    let fixture = Fixture::new();
    let original = std::fs::read(fixture.0.join("world.wl")).unwrap();
    let changed = [
        b"entity external kind place\n".as_slice(),
        original.as_slice(),
    ]
    .concat();
    let path = fixture.0.join("world.wl");
    let injected = changed.clone();
    let rows = exchange(
        vec![
            request(1, "project.open", json!({"path":fixture.0})),
            request(
                2,
                "catalog.scope",
                json!({"project_id":"p1","query":query()}),
            ),
            request(3, "project.analyze", json!({"project_id":"p1"})),
            request(
                4,
                "catalog.scope",
                json!({"project_id":"p1","query":query()}),
            ),
        ],
        move || {
            std::fs::write(path, injected).unwrap();
        },
    );
    assert_eq!(
        std::fs::read(fixture.0.join("world.wl")).unwrap(),
        changed,
        "external mutation must actually occur before checking session semantics"
    );
    assert_eq!(rows[1]["result"]["scope"]["counts"]["matching_objects"], 2);
    assert_eq!(
        rows[1]["result"]["workspace_revision"],
        rows[0]["result"]["baseline"]
    );
    assert_eq!(rows[3]["result"]["scope"]["counts"]["matching_objects"], 3);
    assert_ne!(
        rows[3]["result"]["workspace_revision"],
        rows[1]["result"]["workspace_revision"]
    );
    assert_eq!(std::fs::read(fixture.0.join("world.wl")).unwrap(), changed);
}

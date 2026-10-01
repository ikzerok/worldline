//! 注册表身份缓存的等价与线性打开次数回归，不使用墙钟阈值。
use super::{identity_cache, manifest_path, parse_registry, Registry};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex, MutexGuard,
    },
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
// 这些真实句柄额度测试彼此串行，计数仍是线程局部，不干扰其他测试。
static IDENTITY_TESTS: Mutex<()> = Mutex::new(());

struct Workspace {
    root: PathBuf,
    manifest: Value,
    _guard: MutexGuard<'static, ()>,
}

impl Workspace {
    fn new(count: usize) -> Self {
        let guard = IDENTITY_TESTS
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = std::env::temp_dir().join(format!(
            "worldline-registry-identities-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(root.join(".world/comments")).unwrap();
        let mut manifest = json!({
            "schema_version": 1,
            "language_version": "1.13",
            "required_features": [],
            "comments": {},
        });
        for index in 0..count {
            let path = format!(".world/comments/doc_{index:03}.json");
            fs::write(root.join(&path), "{}").unwrap();
            manifest["comments"][format!("c{index:03}")] = json!(path);
        }
        Self {
            root,
            manifest,
            _guard: guard,
        }
    }

    fn path(&self, index: usize) -> PathBuf {
        self.root
            .join(format!(".world/comments/doc_{index:03}.json"))
    }

    fn save_manifest(&self) -> Vec<u8> {
        let bytes = serde_json::to_vec(&self.manifest).unwrap();
        fs::write(self.root.join(".world/project.json"), &bytes).unwrap();
        bytes
    }

    fn parse(&self) -> Registry {
        let bytes = self.save_manifest();
        identity_cache::reset_counts();
        parse_registry(&self.root, &bytes)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn shared_path_count(registry: &Registry) -> usize {
    registry
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.code == "WS004" && diagnostic.message.contains("共用展示文档路径")
        })
        .count()
}

#[test]
fn three_hundred_valid_registrations_open_each_identity_once() {
    let workspace = Workspace::new(300);
    let registry = workspace.parse();
    assert!(
        registry.diagnostics.is_empty(),
        "{:?}",
        registry.diagnostics
    );
    assert_eq!(registry.comments.len(), 300);
    assert_eq!(registry.documents.len(), 301);
    assert_eq!(identity_cache::counts(), (301, 0));
    // 新的一次解析不借用前一次的身份或句柄。
    let again = workspace.parse();
    assert_eq!(again.comments.len(), 300);
    assert_eq!(identity_cache::counts(), (301, 0));
}

#[test]
fn cached_handles_reject_hardlinks_including_the_manifest() {
    let mut workspace = Workspace::new(300);
    workspace.save_manifest();
    fs::hard_link(
        workspace.path(0),
        workspace.root.join(".world/comments/alias.json"),
    )
    .unwrap();
    fs::hard_link(
        workspace.root.join(".world/project.json"),
        workspace.root.join(".world/comments/manifest_alias.json"),
    )
    .unwrap();
    workspace.manifest["comments"]["z_alias"] = json!(".world/comments/alias.json");
    workspace.manifest["comments"]["z_manifest"] = json!(".world/comments/manifest_alias.json");
    let registry = workspace.parse();
    assert_eq!(shared_path_count(&registry), 2);
    assert_eq!(registry.comments.len(), 300);
    assert_eq!(registry.documents.len(), 301);
    assert_eq!(identity_cache::counts(), (303, 0));
}

#[test]
fn literal_and_case_aliases_are_rejected_before_opening_identity() {
    let mut workspace = Workspace::new(1);
    workspace.manifest["comments"]["z_same"] = json!(".world/comments/doc_000.json");
    workspace.manifest["comments"]["z_case"] = json!(".world/comments/DOC_000.json");
    let registry = workspace.parse();
    assert_eq!(shared_path_count(&registry), 2);
    assert_eq!(registry.comments.len(), 1);
    assert_eq!(identity_cache::counts(), (2, 0));
}

#[test]
fn missing_new_document_remains_registered_and_permanently_degrades_this_parse() {
    let mut workspace = Workspace::new(1);
    workspace.manifest["comments"]["c001"] = json!(".world/comments/not_saved_yet.json");
    workspace.manifest["comments"]["c002"] = json!(".world/comments/later.json");
    workspace.manifest["comments"]["z_alias"] = json!(".world/comments/alias.json");
    fs::write(workspace.root.join(".world/comments/later.json"), "{}").unwrap();
    fs::hard_link(
        workspace.path(0),
        workspace.root.join(".world/comments/alias.json"),
    )
    .unwrap();
    let registry = workspace.parse();
    assert_eq!(registry.comments.len(), 3);
    assert_eq!(shared_path_count(&registry), 1);
    let missing = registry.comments.get("c001").unwrap();
    assert!(registry.documents.contains_key(missing));
    assert!(!missing.exists());
    assert!(!registry.read_only(missing));
    let (opens, comparisons) = identity_cache::counts();
    assert_eq!(opens, 3); // manifest + existing + first failed open，仅尝试一次退化。
    assert!(comparisons > 0);
}

#[test]
fn missing_manifest_starts_in_pairwise_mode() {
    let mut workspace = Workspace::new(2);
    workspace.manifest["comments"]["z_alias"] = json!(".world/comments/alias.json");
    fs::hard_link(
        workspace.path(0),
        workspace.root.join(".world/comments/alias.json"),
    )
    .unwrap();
    let bytes = serde_json::to_vec(&workspace.manifest).unwrap();
    assert!(!workspace.root.join(".world/project.json").exists());
    identity_cache::reset_counts();
    let registry = parse_registry(&workspace.root, &bytes);
    assert_eq!(registry.comments.len(), 2);
    assert_eq!(shared_path_count(&registry), 1);
    assert_eq!(identity_cache::counts().0, 1);
    assert!(identity_cache::counts().1 > 0);
    assert!(registry
        .documents
        .contains_key(&manifest_path(&workspace.root)));
}

#[test]
fn limit_includes_manifest_and_preserves_pairwise_alias_detection() {
    let mut workspace = Workspace::new(512);
    workspace.manifest["comments"]["z_alias"] = json!(".world/comments/alias.json");
    fs::hard_link(
        workspace.path(0),
        workspace.root.join(".world/comments/alias.json"),
    )
    .unwrap();
    let registry = workspace.parse();
    assert_eq!(registry.comments.len(), 512);
    assert_eq!(shared_path_count(&registry), 1);
    assert_eq!(identity_cache::counts().0, 512); // manifest占一个；第512个文档开始逐对。
    assert!(identity_cache::counts().1 >= 512);
}

#[test]
fn unknown_capability_and_version_still_protect_all_registered_bytes() {
    for change in [
        json!({"required_features":["future.required.v9"]}),
        json!({"schema_version":99}),
    ] {
        let mut workspace = Workspace::new(3);
        for (key, value) in change.as_object().unwrap() {
            workspace.manifest[key] = value.clone();
        }
        fs::write(workspace.path(0), [0xff, 0xfe, b'{']).unwrap();
        let before = fs::read(workspace.path(0)).unwrap();
        let registry = workspace.parse();
        assert_eq!(registry.documents.len(), 4);
        assert!(registry.documents.values().all(|read_only| *read_only));
        assert!(registry
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.code, "WS002" | "WS003")));
        assert_eq!(fs::read(workspace.path(0)).unwrap(), before);
        assert_eq!(identity_cache::counts(), (4, 0));
    }
}

#[test]
fn refreshed_paths_do_not_reuse_an_old_identity_cache() {
    let workspace = Workspace::new(2);
    assert!(workspace.parse().diagnostics.is_empty());
    fs::remove_file(workspace.path(1)).unwrap();
    fs::hard_link(workspace.path(0), workspace.path(1)).unwrap();
    let linked = workspace.parse();
    assert_eq!(linked.comments.len(), 1);
    assert_eq!(shared_path_count(&linked), 1);
    fs::remove_file(workspace.path(1)).unwrap();
    fs::write(workspace.path(1), "独立新文件").unwrap();
    let replaced = workspace.parse();
    assert_eq!(replaced.comments.len(), 2);
    assert!(replaced.diagnostics.is_empty());
    assert_eq!(identity_cache::counts(), (3, 0));
}

#[test]
fn invalid_boundary_and_extension_do_not_enter_the_identity_cache() {
    let mut workspace = Workspace::new(1);
    workspace.manifest["comments"]["z_escape"] = json!("../escape.json");
    workspace.manifest["comments"]["z_extension"] = json!(".world/comments/wrong.wl");
    let registry = workspace.parse();
    assert_eq!(registry.comments.len(), 1);
    assert_eq!(
        registry
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "WS004")
            .count(),
        2
    );
    assert_eq!(identity_cache::counts(), (2, 0));
}

#[test]
fn opened_handles_are_dropped_before_pairwise_fallback() {
    let workspace = Workspace::new(1);
    workspace.save_manifest();
    let manifest = manifest_path(&workspace.root);
    let mut cache = identity_cache::IdentityCache::new(&manifest);
    assert_eq!(cache.cached_handle_count(), Some(1));
    let paths = [manifest];
    assert!(!cache.duplicates(paths.iter(), &workspace.path(0)));
    assert_eq!(cache.cached_handle_count(), Some(2));
    assert!(!cache.duplicates(paths.iter(), &workspace.root.join("missing.json")));
    assert_eq!(cache.cached_handle_count(), None);
    assert!(!cache.duplicates(paths.iter(), &workspace.path(0)));
    assert_eq!(cache.cached_handle_count(), None);
}

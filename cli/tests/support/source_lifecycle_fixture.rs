use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::project::Project;
use worldline_core::source_lifecycle::{SourceLifecyclePlan, SourceLifecycleRequest};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    pub root: PathBuf,
}

impl Fixture {
    pub fn new(label: &str, explicit: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "source-lifecycle-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(".world")).unwrap();
        let mut manifest = json!({"schema_version":1,"language_version":"1.9",
            "entry":"world.wl","required_features":[],"extension":{"keep":true}});
        if explicit {
            manifest["required_features"] = json!(["workspace.source_sets.v1"]);
            manifest["source_config"] = json!({"mode":"explicit",
                "active":["world.wl","old.wl"],"archived":["archived.wl"]});
        }
        std::fs::write(root.join(".world/project.json"), manifest.to_string()).unwrap();
        std::fs::write(
            root.join("world.wl"),
            "include \"old.wl\"\nevent start\n  原稿\n  -> END\n",
        )
        .unwrap();
        std::fs::write(root.join("old.wl"), "character guide\n").unwrap();
        std::fs::write(root.join("archived.wl"), "character hidden\n").unwrap();
        Self { root }
    }

    pub fn project(&self) -> Project {
        Project::open(&self.root).unwrap()
    }

    pub fn plan(&self, request: &Value) -> SourceLifecyclePlan {
        self.project()
            .preview_source_lifecycle(&self.request(request))
            .unwrap()
    }

    pub fn request(&self, request: &Value) -> SourceLifecycleRequest {
        serde_json::from_value(request.clone()).unwrap()
    }

    pub fn bytes(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        collect(&self.root, &self.root, &mut files);
        files
    }

    pub fn journal_count(&self) -> usize {
        let directory = self.root.join(".world/.transactions");
        std::fs::read_dir(directory)
            .map(|entries| entries.filter_map(Result::ok).count())
            .unwrap_or(0)
    }
}

fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        let relative = path.strip_prefix(root).unwrap();
        if relative == Path::new(".world/.transactions") {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, files);
        } else {
            files.insert(relative.to_path_buf(), std::fs::read(&path).unwrap());
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::markdown_import::MarkdownImportRequest;
use worldline_core::project::Project;
use worldline_core::workspace_snapshot::Files;

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    pub(super) source: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "worldline-markdown-import-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let source = root.join("external");
        fs::create_dir_all(root.join("project/.world")).unwrap();
        fs::create_dir_all(source.join("media")).unwrap();
        fs::write(root.join("project/world.wl"), "event start\n  -> END\n").unwrap();
        fs::write(
            root.join("project/.world/project.json"),
            r#"{"schema_version":1,"language_version":"1.10","required_features":[],"extension":{"keep":true}}"#,
        )
        .unwrap();
        fs::write(
            source.join("harbor.md"),
            "---\nid: harbor\ntitle: 雾港\nkind: place\n---\n# 雾港\n灯塔与[[原样保留]]海面。\n\n[岸线](coast.md#shore)\n\n![地图](media/map.bin)\n\n```js\nrun()\n```\n",
        )
        .unwrap();
        fs::write(source.join("coast.md"), "# 海岸\n\n## Shore\n海浪拍岸。\n").unwrap();
        fs::write(source.join("media/map.bin"), [0, 1, 255]).unwrap();
        Self { root, source }
    }

    pub(super) fn project(&self) -> Project {
        Project::open(&self.root.join("project")).unwrap()
    }

    pub(super) fn request(&self, project: &Project) -> MarkdownImportRequest {
        MarkdownImportRequest {
            source_root: self.source.clone(),
            expected_baseline: project.content_baseline(),
            id_overrides: BTreeMap::new(),
            namespace: None,
            accept_losses: false,
            allow_language_upgrade: false,
        }
    }

    pub(super) fn source_files(&self) -> Files {
        Files::from([
            (
                PathBuf::from("harbor.md"),
                fs::read(self.source.join("harbor.md")).unwrap(),
            ),
            (
                PathBuf::from("coast.md"),
                fs::read(self.source.join("coast.md")).unwrap(),
            ),
            (
                PathBuf::from("media/map.bin"),
                fs::read(self.source.join("media/map.bin")).unwrap(),
            ),
        ])
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

pub const GOOD: &str = "event start\n  正文\n  -> END\n";
pub const BAD: &str = "character guide\ncharacter guide\nevent start\n  -> missing\n";

pub struct Fixture {
    pub root: PathBuf,
}
impl Fixture {
    pub fn new(source: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "problems-protocol-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(".world")).unwrap();
        std::fs::write(root.join("world.wl"), source).unwrap();
        std::fs::write(
            root.join(".world/project.json"),
            r#"{"schema_version":1,"language_version":"1.10","required_features":[]}"#,
        )
        .unwrap();
        Self { root }
    }
    pub fn report(&self) -> worldline_core::problems::ProblemsReport {
        worldline_core::project::Project::open(&self.root)
            .unwrap()
            .problems_report(&Default::default())
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

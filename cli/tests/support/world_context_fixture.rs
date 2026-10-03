use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
pub const SOURCE: &str = "character a as \"甲\"\n  property mentor = ref(\"character\", \"b\")\ncharacter b as \"乙\"\nperiod night\nevent first during night with b\n  [[character:b|乙]]\n  -> END\nevent second during night follows first\n  -> END\n";
pub struct Fixture {
    pub root: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "world-context-contract-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(".world")).unwrap();
        std::fs::write(root.join("world.wl"), SOURCE).unwrap();
        std::fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.13","required_features":["content.object_refs.v1","content.character_refs.v1"]}"#).unwrap();
        Self { root }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

use std::{fs, path::PathBuf};
use worldline_core::{catalog::TargetRef, compile_path, project::Project};
use worldline_runtime::Story;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "wl-workspace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[path = "workspace/maps.rs"]
mod maps;
#[path = "workspace/project.rs"]
mod project;
#[path = "workspace/recovery.rs"]
mod recovery;
#[path = "workspace/refresh.rs"]
mod refresh;

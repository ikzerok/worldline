use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::project::{CheckpointLimits, Project};
pub(super) struct TempWorkspace(PathBuf);

impl TempWorkspace {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "worldline-checkpoints-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".world/maps")).unwrap();
        fs::write(
            root.join("world.wl"),
            "event start\n  第一版正文。\n  -> END\n",
        )
        .unwrap();
        fs::write(
            root.join(".world/project.json"),
            br#"{"schema_version":1,"maps":{"raw":".world/maps/raw.json"}}"#,
        )
        .unwrap();
        fs::write(root.join(".world/maps/raw.json"), br#"{"unknown":true}"#).unwrap();
        fs::write(root.join("notes.bin"), [0, 17, 255]).unwrap();
        Self(root)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }

    pub(super) fn open(&self) -> Project {
        Project::open(self.path()).unwrap()
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn default_limits() -> CheckpointLimits {
    CheckpointLimits::default()
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn test_checksum(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn test_files_digest(files: &std::collections::BTreeMap<String, Vec<u8>>) -> String {
    fn mix(hash: &mut u64, bytes: &[u8]) {
        for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            *hash ^= u64::from(*byte);
            *hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    let mut hash = 0xcbf29ce484222325u64;
    mix(&mut hash, b"worldline-checkpoint-files-v1");
    for (path, bytes) in files {
        mix(&mut hash, path.as_bytes());
        mix(&mut hash, bytes);
    }
    format!("{hash:016x}")
}

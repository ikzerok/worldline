#[cfg(not(target_arch = "wasm32"))]
use super::*;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
impl Project {
    pub fn export_reader_site_with_progress(
        &self,
        selection: &ReaderExportSelection,
        expected_plan_digest: &str,
        destination: &Path,
        progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
    ) -> Result<(), String> {
        let files =
            self.build_reader_export_with_progress(selection, expected_plan_digest, progress)?;
        publish(self, files, destination, progress)
    }

    pub fn export_reader_profile(
        &self,
        profile: &ReaderPublicationProfile,
        expected_plan_digest: &str,
        destination: &Path,
    ) -> Result<(), String> {
        self.export_reader_profile_with_progress(
            profile,
            expected_plan_digest,
            destination,
            &mut |_| true,
        )
    }

    pub fn export_reader_profile_with_progress(
        &self,
        profile: &ReaderPublicationProfile,
        expected_plan_digest: &str,
        destination: &Path,
        progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
    ) -> Result<(), String> {
        let files =
            self.build_reader_profile_with_progress(profile, expected_plan_digest, progress)?;
        publish(self, files, destination, progress)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish(
    project: &Project,
    files: BTreeMap<PathBuf, Vec<u8>>,
    destination: &Path,
    progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
) -> Result<(), String> {
    let destination = crate::compiler::source_path(destination);
    match std::fs::symlink_metadata(&destination) {
        Ok(_) => return Err("导出目标已存在，请选择新的文件夹名称".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("无法检查导出目标：{error}")),
    }
    if destination.starts_with(crate::compiler::source_path(&project.root)) {
        return Err("阅读包目标必须位于当前工作区之外".into());
    }
    let parent = destination.parent().ok_or("导出目录缺少父目录")?;
    if !parent.is_dir() {
        return Err("阅读包目标的父目录必须已存在".into());
    }
    static NEXT_STAGE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut staging = None;
    for _ in 0..100 {
        let sequence = NEXT_STAGE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".worldline-reader-export-{}-{sequence}",
            std::process::id()
        ));
        match std::fs::create_dir(&candidate) {
            Ok(()) => {
                staging = Some(candidate);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("无法建立阅读包暂存目录：{error}")),
        }
    }
    let staging = staging.ok_or("无法分配唯一的阅读包暂存目录")?;
    let result = (|| -> Result<(), String> {
        let total = files.len();
        for (index, (relative, bytes)) in files.into_iter().enumerate() {
            super::progress::report(progress, "write", index, total)?;
            super::site::validate_output_path(&relative)?;
            let target = staging.join(relative);
            std::fs::create_dir_all(target.parent().ok_or("输出路径缺少父目录")?)
                .map_err(|e| e.to_string())?;
            std::fs::write(target, bytes).map_err(|e| e.to_string())?;
        }
        super::progress::report(progress, "publish", 0, 1)?;
        super::native_rename::rename_new(&staging, &destination)
            .map_err(|e| format!("原子发布失败：{e}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

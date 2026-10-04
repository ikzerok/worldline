//! 比较等查询的私有加载入口，不运行恢复或迁移。
use super::*;
impl Project {
    pub fn open_read_only(path: &Path) -> Result<Self, String> {
        let entry = entry_path(path);
        let root = entry.parent().ok_or("工作区缺少主目录")?.to_path_buf();
        ensure_no_transaction(&root)?;
        let paths = crate::file_access::workspace_files_limited(&root, 4096)
            .map_err(|error| error.to_string())?;
        let mut files = crate::workspace_snapshot::Files::new();
        let mut remaining = 64 * 1024 * 1024;
        for path in paths {
            let path = crate::file_access::within(&root, &path)?;
            let bytes = crate::file_access::read_limited(&path, remaining)
                .map_err(|error| error.to_string())?;
            remaining = remaining
                .checked_sub(bytes.len())
                .ok_or("只读工作区超过64 MiB")?;
            let relative = path.strip_prefix(&root).map_err(|_| "只读工作区文件越界")?;
            files.insert(relative.to_path_buf(), bytes);
        }
        ensure_no_transaction(&root)?;
        let relative = entry
            .strip_prefix(&root)
            .map_err(|_| "只读工作区入口越界")?;
        if !files.contains_key(relative) {
            return Err("只读工作区入口不存在".into());
        }
        Self::from_snapshot(&root, relative, &files)
    }
    pub fn compile_read_only(&self) -> Result<CompileResult, String> {
        if !self.authoring_diagnostics.is_empty() {
            return Err("工作区清单含错误，无法建立比较快照".into());
        }
        if !self.recovery_conflicts.is_empty() {
            return Err("工程存在未解决保存事务，比较不会自动恢复".into());
        }
        ensure_no_transaction(&self.root)?;
        Ok(self.compile_problems_snapshot())
    }
}
fn ensure_no_transaction(root: &Path) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    if crate::storage::has_unresolved_transactions(root)? {
        return Err("工程存在未完成保存事务，比较不会自动恢复，请先正常打开并处理".into());
    }
    #[cfg(target_arch = "wasm32")]
    let _ = root;
    Ok(())
}

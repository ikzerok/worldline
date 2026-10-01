//! 导航的只读外部基线检查；已应用但未保存的新稿可以导航，不要求保存。
use super::*;

impl Project {
    pub fn verify_source_navigation(&self, path: &Path, expected: &str) -> Result<(), String> {
        let path = crate::file_access::within(&self.root, path)?;
        let document = self
            .documents
            .get(&path)
            .filter(|document| !document.is_deleted())
            .ok_or("来源文件已删除或移动，请重新运行后定位")?;
        if document.text != expected {
            return Err("来源内容已变化，请重新运行后定位".into());
        }
        if !self.recovery_conflicts.is_empty() {
            return Err("工作区仍有恢复冲突，无法确认来源".into());
        }
        // Validates link/junction boundaries too. An unsaved new workspace may not exist yet.
        match crate::file_access::workspace_files(&self.root) {
            Ok(_) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && document.saved.is_none() => {}
            Err(error) => return Err(format!("无法确认来源文件边界：{error}")),
        }
        let disk = match crate::file_access::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("无法读取来源基线：{error}")),
        };
        if disk.as_deref() != document.saved.as_ref().map(|text| text.as_bytes()) {
            return Err("来源已被外部修改、移动或删除；请先刷新并处理冲突".into());
        }
        Ok(())
    }
}

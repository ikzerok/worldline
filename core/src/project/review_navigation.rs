//! 审稿来源导航的一次只读库存/保存基线观察，不刷新、不恢复、不要求先保存。
use super::*;

impl Project {
    /// 先调用一次本守卫，再以全部当前 WritingBuffer 编译结果校验 ReviewSource。
    /// 浏览器只检查已导入快照；不能观察未重新导入的宿主磁盘变动。
    pub fn verify_review_navigation(&self) -> Result<(), String> {
        if !self.authoring_diagnostics.is_empty() {
            return Err("工作区清单或登记文档尚未确认，不能定位旧审稿；请先处理诊断".into());
        }
        let files = crate::source_lifecycle::safety::inventory(self)
            .map_err(|failure| format!("审稿来源需要刷新：{}", failure.message))?;
        let manifest = crate::workspace_documents::manifest_path(&self.root);
        if files.binary_search(&manifest).is_ok()
            && !self.authoring_documents.contains_key(&manifest)
        {
            return Err("磁盘新增工程清单尚未载入；请先刷新并重新审稿".into());
        }
        self.source_disk_baselines_match_inventory(&files)
            .map_err(|failure| format!("审稿来源需要刷新：{}", failure.message))
    }
}

#[cfg(test)]
mod tests;

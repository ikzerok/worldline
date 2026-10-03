use super::*;
use crate::{project::Project, CompileResult};

impl CompileResult {
    /// 精确稳定身份查找，不消耗广域目录候选预算。
    pub fn lookup_world_object(&self, target: &TargetRef) -> Result<CatalogObject, WorldContextError> {
        self.analysis.catalog.object(target).cloned()
            .ok_or_else(|| WorldContextError::UnknownTarget(target.clone()))
    }

    pub fn world_context_snapshot(&self) -> String {
        let mut parts = vec![serde_json::to_vec(&self.options).expect("编译选项可序列化")];
        for (file, source) in &self.sources {
            parts.push(file.to_string_lossy().as_bytes().to_vec());
            parts.push(source.as_bytes().to_vec());
        }
        // 源集合相同仍须区分显式加载/默认入口及实际执行次序。
        for (event, file) in self.program.events.iter().zip(&self.program.event_files) {
            parts.push(file.as_bytes().to_vec());
            parts.push(event.name.as_bytes().to_vec());
        }
        parts.push(crate::fingerprint_program(&self.program).to_le_bytes().to_vec());
        digest(parts)
    }
}
impl Project {
    pub fn lookup_world_object(&self, target: &TargetRef) -> Result<CatalogObject, WorldContextError> {
        self.compile_current().lookup_world_object(target)
    }

    pub fn query_world_context(
        &self, target: &TargetRef, options: WorldContextOptions,
    ) -> Result<WorldContextResult, WorldContextError> {
        self.query_world_context_cancellable(target, options, || false)
    }

    pub fn query_world_context_cancellable<F: FnMut() -> bool>(
        &self, target: &TargetRef, options: WorldContextOptions, mut cancelled: F,
    ) -> Result<WorldContextResult, WorldContextError> {
        if cancelled() { return Err(WorldContextError::Cancelled); }
        let baseline = self.content_baseline();
        let compiled = self.compile_current();
        let mut result = compiled.query_world_context_cancellable(target, options, cancelled)?;
        result.content_baseline = Some(baseline);
        Ok(result)
    }

    pub fn compare_temporal_events(
        &self, left: &str, right: &str, expected_baseline: Option<&str>,
    ) -> Result<crate::timeline::TemporalComparison, String> {
        if expected_baseline.is_some_and(|baseline| baseline != self.content_baseline()) {
            return Err("STALE_BASELINE: 稿件已变化，请重新查询时间证据".into());
        }
        Ok(self.compile_current().analysis.timeline.compare(left, right))
    }
}

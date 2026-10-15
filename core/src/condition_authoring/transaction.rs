use super::*;
use crate::project::Project;
use std::path::Path;

impl Project {
    pub fn preview_condition_authoring(
        &self,
        request: &ConditionAuthoringRequest,
    ) -> Result<ConditionAuthoringPreview, String> {
        self.prepare_condition_authoring(request)
            .map(|(_, preview)| preview)
    }
    pub fn apply_condition_authoring(
        &mut self,
        request: &ConditionAuthoringRequest,
        plan_digest: &str,
    ) -> Result<ConditionAuthoringPreview, String> {
        let (candidate, preview) = self.prepare_condition_authoring(request)?;
        if preview.plan_digest != plan_digest {
            return Err("条件作者预览摘要过期或不符；原稿未改变".into());
        }
        *self = candidate;
        Ok(preview)
    }
    fn prepare_condition_authoring(
        &self,
        request: &ConditionAuthoringRequest,
    ) -> Result<(Project, ConditionAuthoringPreview), String> {
        super::protocol::validate_request(request)?;
        super::protocol::check_workspace(self)?;
        if request.expected_baseline != self.content_baseline()
            || request.expected_generation != self.search_refresh_generation()
        {
            return Err("条件作者基线或刷新代次已过期；请保留输入并重新预览".into());
        }
        let guard = super::protocol::guard(self)?;
        let (compiled, exceeded) =
            crate::expression::condition::scoped(|| self.compile_problems_snapshot());
        if exceeded {
            return Err("工程表达式超过作者解析预算；原稿未改变".into());
        }
        let before = self.condition_authoring_from_compile(&compiled, &request.selector)?;
        let location = before
            .location
            .clone()
            .ok_or("源码槽尚不存在；请用事件/选择表单显式新增，不能猜测插入位置")?;
        let after_source = super::draft::replacement_source(
            &compiled,
            &before,
            &Some(request.replacement.clone()),
        )?;
        let mut candidate = self.clone();
        let changed = after_source != before.source;
        let relative = Path::new(&location.file)
            .strip_prefix(&self.root)
            .map_err(|_| "条件来源越过工作区")?
            .to_path_buf();
        let patch = if changed {
            let source = self.document(Path::new(&location.file))?;
            if source.get(location.byte_range.clone()) != Some(before.source.as_str()) {
                return Err("条件原始范围与来源快照不一致".into());
            }
            let mut source = source.to_owned();
            source.replace_range(location.byte_range.clone(), &after_source);
            candidate.set_text(Path::new(&location.file), source)?;
            Some(ConditionSourcePatch {
                file: relative.clone(),
                byte_range: location.byte_range,
                before: before.source.clone(),
                after: after_source,
            })
        } else {
            None
        };
        super::protocol::check_workspace(&candidate)?;
        let (after_compiled, exceeded) =
            crate::expression::condition::scoped(|| candidate.compile_problems_snapshot());
        if exceeded {
            return Err("候选工程表达式超过作者解析预算；原稿未改变".into());
        }
        let after =
            candidate.condition_authoring_from_compile(&after_compiled, &request.selector)?;
        if after.tree.as_ref() != Some(&request.replacement) || !after.writable {
            return Err("候选条件与请求不一致或不再可写；原稿未改变".into());
        }
        let mut preview = ConditionAuthoringPreview {
            schema_version: 1,
            expected_baseline: request.expected_baseline.clone(),
            plan_digest: String::new(),
            changed,
            before_fingerprint: compiled.analysis.fingerprint.to_string(),
            after_fingerprint: after_compiled.analysis.fingerprint.to_string(),
            runtime_changed: compiled.analysis.fingerprint != after_compiled.analysis.fingerprint,
            before,
            after,
            patch,
            changed_files: if changed { vec![relative] } else { Vec::new() },
            has_errors: after_compiled.has_errors(),
            diagnostics: after_compiled.diagnostics,
        };
        preview.plan_digest = super::protocol::digest(&(request, &guard, &preview))?;
        super::protocol::measure(&preview, MAX_CONDITION_OUTPUT_BYTES)?;
        if guard != super::protocol::guard(self)? {
            return Err("条件预览期间来源或外部库存变化；原稿未改变".into());
        }
        Ok((candidate, preview))
    }
}

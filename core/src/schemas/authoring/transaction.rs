use super::{
    model::*,
    projection::{check_workspace_budget, object},
    protocol::{measure, validate_request},
    source::changed_sources,
};
use crate::{project::Project, schemas::SchemaDecl};

impl Project {
    pub fn preview_schema_authoring(
        &self,
        request: &SchemaAuthoringRequest,
    ) -> Result<SchemaAuthoringPreview, String> {
        self.prepare_schema_authoring(request)
            .map(|(_, preview)| preview)
    }

    /// 显式应用作者草稿，普通实例约束错误不等同于来源或权限错误；不自动保存。
    pub fn apply_schema_authoring(
        &mut self,
        request: &SchemaAuthoringRequest,
        plan_digest: &str,
    ) -> Result<SchemaAuthoringPreview, String> {
        let (candidate, preview) = self.prepare_schema_authoring(request)?;
        if preview.plan_digest != plan_digest {
            return Err("schema 作者预览已过期或摘要不符；原稿未改变".into());
        }
        *self = candidate;
        Ok(preview)
    }

    fn prepare_schema_authoring(
        &self,
        request: &SchemaAuthoringRequest,
    ) -> Result<(Project, SchemaAuthoringPreview), String> {
        validate_request(request)?;
        check_workspace_budget(self)?;
        if request.expected_baseline != self.content_baseline() {
            return Err("schema 作者草稿基线已过期，请保留输入并重新预览".into());
        }
        self.ensure_workspace_writable()?;
        let inventory = crate::source_lifecycle::safety::inventory(self).map_err(|e| e.message)?;
        self.source_disk_baselines_match_inventory(&inventory)
            .map_err(|e| e.message)?;
        let guard = self
            .source_lifecycle_guard(&inventory)
            .map_err(|e| e.message)?;
        let compiled = self.compile_current();
        let before = self.schema_authoring_from_compile(&compiled, &request.target)?;
        if !before.writable {
            return Err(format!(
                "schema 作者来源不可安全编辑：{}",
                before.read_only_reasons.join("；")
            ));
        }
        let next_schema = resulting_schema(request, &before)?;
        let sources = changed_sources(
            self,
            &compiled,
            request,
            next_schema,
            before.binding.as_ref(),
        )?;
        let mut candidate = self.clone();
        let mut changed_files = Vec::new();
        for (path, source) in sources {
            crate::source_lifecycle::safety::writable_path(&path)?;
            candidate.set_text(&path, source)?;
            changed_files.push(
                path.strip_prefix(&self.root)
                    .map_err(|_| "候选源码越过工作区")?
                    .to_path_buf(),
            );
        }
        check_workspace_budget(&candidate)?;
        let after_compiled = candidate.compile_current();
        let after = candidate.schema_authoring_from_compile(&after_compiled, &request.target)?;
        if !after.writable {
            return Err(format!(
                "候选稿含来源或声明歧义；原稿未改变：{}",
                after.read_only_reasons.join("；")
            ));
        }
        verify_result(request, &after_compiled, next_schema)?;
        let mut preview = SchemaAuthoringPreview {
            schema_version: 1,
            expected_baseline: request.expected_baseline.clone(),
            plan_digest: String::new(),
            changed: !changed_files.is_empty(),
            before_fingerprint: compiled.analysis.fingerprint.to_string(),
            after_fingerprint: after_compiled.analysis.fingerprint.to_string(),
            runtime_changed: compiled.analysis.fingerprint != after_compiled.analysis.fingerprint,
            before,
            after,
            changed_files,
            has_errors: after_compiled.has_errors(),
            diagnostics: after_compiled.diagnostics,
        };
        // 摘要包括全部正式投影和诊断；附件等外部库存变化也不能重用旧计划。
        measure(&preview, MAX_SCHEMA_AUTHORING_OUTPUT_BYTES - 128)?;
        let bytes = serde_json::to_vec(&(request, &guard, &preview)).map_err(|e| e.to_string())?;
        preview.plan_digest = crate::presentation_commands::document_hash(&bytes);
        measure(&preview, MAX_SCHEMA_AUTHORING_OUTPUT_BYTES)?;
        let current_inventory =
            crate::source_lifecycle::safety::inventory(self).map_err(|e| e.message)?;
        self.source_disk_baselines_match_inventory(&current_inventory)
            .map_err(|e| e.message)?;
        if guard
            != self
                .source_lifecycle_guard(&current_inventory)
                .map_err(|e| e.message)?
        {
            return Err("schema 预览期间工作区来源或外部库存变化；原稿未改变".into());
        }
        for path in &preview.changed_files {
            crate::source_lifecycle::safety::writable_path(&self.root.join(path))?;
        }
        Ok((candidate, preview))
    }
}

fn resulting_schema<'a>(
    request: &SchemaAuthoringRequest,
    before: &'a SchemaAuthoringProjection,
) -> Result<Option<&'a SchemaDecl>, String> {
    let selected = match &request.binding {
        SchemaBindingMutation::Keep => before.schema.as_ref(),
        SchemaBindingMutation::Unbind => {
            if before.binding.is_none() {
                return Err("对象尚未绑定，不能解绑".into());
            }
            None
        }
        SchemaBindingMutation::Bind { schema_id } | SchemaBindingMutation::Change { schema_id } => {
            let is_bind = matches!(request.binding, SchemaBindingMutation::Bind { .. });
            if is_bind != before.binding.is_none() {
                return Err("绑定状态已改变，请明确选择 Bind 或 Change".into());
            }
            let selected = before
                .compatible_schemas
                .iter()
                .find(|s| &s.id == schema_id)
                .ok_or("所选 schema 不存在或不适用于此完整对象身份")?;
            Some(selected)
        }
    };
    if selected.is_none() && !request.fields.is_empty() {
        return Err("字段操作需要明确的操作后 schema；解绑不隐式删除属性".into());
    }
    Ok(selected)
}

fn verify_result(
    request: &SchemaAuthoringRequest,
    compiled: &crate::CompileResult,
    schema: Option<&SchemaDecl>,
) -> Result<(), String> {
    let target = object(&compiled.program, &request.target)?;
    let actual: Vec<_> = compiled
        .program
        .schema_bindings
        .iter()
        .filter(|b| b.target == request.target)
        .collect();
    if actual.len() != usize::from(schema.is_some())
        || actual.first().map(|b| b.schema_id.as_str()) != schema.map(|s| s.id.as_str())
    {
        return Err("候选绑定结果与显式请求不一致；原稿未改变".into());
    }
    for mutation in &request.fields {
        let (key, expected) = match mutation {
            SchemaFieldMutation::Set { key, value, .. } => (key, Some(value)),
            SchemaFieldMutation::Remove { key, .. } => (key, None),
        };
        if target
            .properties
            .iter()
            .find(|p| &p.name == key)
            .map(|p| &p.value)
            != expected
        {
            return Err("候选属性无法无损表达请求值；原稿未改变".into());
        }
    }
    Ok(())
}

use super::*;
use crate::{project::Project, CompileResult};

impl Project {
    pub fn preview_condition_draft(
        &self,
        compiled: &CompileResult,
        input: &ConditionDraftInput,
        replacement: &Option<ConditionNode>,
    ) -> Result<ConditionDraftPreview, String> {
        let guard = super::protocol::guard(self)?;
        let before = self.condition_draft_from_compile(compiled, input)?;
        let after_source = replacement_source(compiled, &before, replacement)?;
        let mut preview = ConditionDraftPreview {
            schema_version: 1,
            plan_digest: String::new(),
            changed: after_source != input.source,
            before,
            after_source,
            replacement: replacement.clone(),
        };
        preview.plan_digest = super::protocol::digest(&(input, &guard, &preview))?;
        if guard != super::protocol::guard(self)? {
            return Err("条件预览期间工作区变化；原输入已保留".into());
        }
        Ok(preview)
    }
    pub fn apply_condition_draft(
        &self,
        compiled: &CompileResult,
        input: &ConditionDraftInput,
        preview: &ConditionDraftPreview,
    ) -> Result<String, String> {
        let current = self.preview_condition_draft(compiled, input, &preview.replacement)?;
        if super::protocol::digest(&current)? != super::protocol::digest(preview)? {
            return Err("条件草稿或预览已过期；保留输入并重新预览".into());
        }
        Ok(current.after_source)
    }
}
pub(super) fn replacement_source(
    compiled: &CompileResult,
    before: &ConditionProjection,
    replacement: &Option<ConditionNode>,
) -> Result<String, String> {
    if !before.writable {
        return Err(format!(
            "条件不可结构化编辑：{}",
            before
                .reasons
                .iter()
                .chain(&before.read_only_reasons)
                .cloned()
                .collect::<Vec<_>>()
                .join("；")
        ));
    }
    if let Some(tree) = replacement {
        super::tree::validate_budget(tree)?;
    }
    if before.tree == *replacement {
        return Ok(before.source.clone());
    }
    let source = replacement
        .as_ref()
        .map(super::tree::render)
        .transpose()?
        .unwrap_or_default();
    if replacement.is_none() {
        return Ok(source);
    }
    let mut candidate = before.clone();
    candidate.source = source.clone();
    candidate.tree = None;
    candidate.diagnostics.clear();
    candidate.reasons.clear();
    candidate.support = ConditionSupport::Empty;
    super::projection::classify(compiled, &mut candidate);
    if candidate.support != ConditionSupport::Supported || candidate.tree != *replacement {
        return Err(format!(
            "候选条件不能按正式语言无损往返：{}",
            candidate
                .reasons
                .iter()
                .cloned()
                .chain(candidate.diagnostics.iter().map(|d| d.message.clone()))
                .collect::<Vec<_>>()
                .join("；")
        ));
    }
    Ok(source)
}

use super::*;
use crate::{project::Project, CompileResult, Severity};

impl Project {
    pub fn condition_authoring(
        &self,
        selector: &ConditionSelector,
    ) -> Result<ConditionProjection, String> {
        super::protocol::validate_selector(selector)?;
        super::protocol::check_workspace(self)?;
        let (result, exceeded) = crate::expression::condition::scoped(|| {
            self.condition_authoring_from_compile(&self.compile_problems_snapshot(), selector)
        });
        let mut result = result?;
        if exceeded {
            result.writable = false;
            result
                .read_only_reasons
                .push("工程中表达式超过条件查询解析预算；保留原文，只读查看".into());
        }
        Ok(result)
    }
    pub fn condition_authoring_from_compile(
        &self,
        compiled: &CompileResult,
        selector: &ConditionSelector,
    ) -> Result<ConditionProjection, String> {
        super::protocol::validate_selector(selector)?;
        super::protocol::snapshot(self, compiled)?;
        let (source, location) = super::source::locate(compiled, selector)?;
        projection(self, compiled, selector.clone(), source, location, 0)
    }
    pub fn condition_draft_from_compile(
        &self,
        compiled: &CompileResult,
        input: &ConditionDraftInput,
    ) -> Result<ConditionProjection, String> {
        super::protocol::version(input.schema_version)?;
        super::protocol::measure(input, MAX_CONDITION_REQUEST_BYTES)?;
        let selector = ConditionSelector {
            schema_version: input.schema_version,
            owner: input.owner.clone(),
            slot: input.slot,
        };
        super::protocol::validate_selector(&selector)?;
        super::protocol::snapshot(self, compiled)?;
        super::source::event(compiled, &input.owner)?;
        projection(
            self,
            compiled,
            selector,
            input.source.clone(),
            None,
            input.generation,
        )
    }
}
fn projection(
    project: &Project,
    compiled: &CompileResult,
    selector: ConditionSelector,
    source: String,
    location: Option<ConditionSource>,
    input_generation: u64,
) -> Result<ConditionProjection, String> {
    let mut result = ConditionProjection {
        schema_version: 1,
        selector,
        baseline: project.content_baseline(),
        generation: project.search_refresh_generation(),
        input_generation,
        source,
        location,
        support: ConditionSupport::Empty,
        tree: None,
        reasons: Vec::new(),
        diagnostics: Vec::new(),
        writable: false,
        read_only_reasons: Vec::new(),
    };
    if let Err(reason) = super::protocol::guard(project) {
        result.read_only_reasons.push(reason);
    }
    let (_, file) = super::source::event(compiled, &result.selector.owner)?;
    let path = result
        .location
        .as_ref()
        .map_or(file, |location| &location.file);
    if let Err(reason) = crate::source_lifecycle::safety::writable_path(std::path::Path::new(path))
    {
        result.read_only_reasons.push(reason);
    }
    if compiled.has_errors() {
        result
            .read_only_reasons
            .push("当前工程含正式编译错误；保留原稿，修复后重新打开条件表单".into());
    }
    classify(compiled, &mut result);
    result.writable = result.read_only_reasons.is_empty()
        && matches!(
            result.support,
            ConditionSupport::Empty | ConditionSupport::Supported
        );
    super::protocol::measure(&result, MAX_CONDITION_OUTPUT_BYTES)?;
    Ok(result)
}
pub(super) fn classify(compiled: &CompileResult, result: &mut ConditionProjection) {
    let reject = |result: &mut ConditionProjection, support, reason: &str| {
        result.support = support;
        result.reasons.push(reason.into());
    };
    if result.selector.slot == ConditionSlot::ChoiceEnable
        && !compiled.options.language_version.supports_language_112()
    {
        reject(
            result,
            ConditionSupport::VersionUnsupported,
            "可选条件需要显式语言 1.12；不会自动升级",
        );
        return;
    }
    if result.source.len() > MAX_CONDITION_SOURCE_BYTES {
        reject(
            result,
            ConditionSupport::BudgetExceeded,
            "条件超过 16 KiB 表单预算；完整高级源码已保留",
        );
        return;
    }
    if result.source.trim().is_empty() {
        return;
    }
    if result.source.contains(['\n', '\r']) {
        reject(result, ConditionSupport::Invalid, "条件必须为单行表达式");
        return;
    }
    if !crate::lexer::comment_source_spans(&result.source).is_empty() {
        reject(
            result,
            ConditionSupport::Advanced,
            "条件内有注释；保留完整高级源码",
        );
        return;
    }
    let (file, line, base) = result
        .location
        .as_ref()
        .map(|s| (s.file.as_str(), s.line, s.column - 1))
        .unwrap_or(("condition-draft.wl", 1, 0));
    let parsed = crate::expression::condition::parse(
        &result.source,
        file,
        line,
        base,
        &mut result.diagnostics,
    );
    let Ok((expression, provenance)) = parsed else {
        reject(
            result,
            ConditionSupport::BudgetExceeded,
            "条件超过正式解析 token/嵌套预算；完整高级源码已保留",
        );
        return;
    };
    if result
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        reject(
            result,
            ConditionSupport::Invalid,
            "条件含正式解析错误；不能编辑占位表达式",
        );
        return;
    }
    if !super::tree::expression_budget(&expression) {
        reject(
            result,
            ConditionSupport::BudgetExceeded,
            "条件超过 128 节点或 24 层预算；完整高级源码已保留",
        );
        return;
    }
    result
        .diagnostics
        .extend(crate::analysis::condition_diagnostics(
            &compiled.program,
            &compiled.analysis.symbols,
            &expression,
            &provenance,
        ));
    if result
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        reject(
            result,
            ConditionSupport::Invalid,
            "条件引用或类型无效；请保留原文并修复",
        );
        return;
    }
    result.tree = super::tree::project(&expression, compiled);
    if result.tree.is_some() {
        result.support = ConditionSupport::Supported;
    } else {
        reject(
            result,
            ConditionSupport::Advanced,
            "条件不在本版表单子集；完整高级源码已保留",
        );
    }
}

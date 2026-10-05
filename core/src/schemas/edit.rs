use super::{SchemaField, SchemaIndex, SchemaInstance};
use crate::{
    catalog::TargetRef, diagnostic::DiagnosticSourceRole, project::Project,
    source_edit::SourceEditRequest, Diagnostic,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct SchemaFieldChange {
    pub schema_id: String,
    /// None 表示 schema 本身的新增、删除或适用范围/closed 变化。
    pub field_id: Option<String>,
    pub change: String,
    pub before: Option<SchemaField>,
    pub after: Option<SchemaField>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaInstanceImpact {
    pub target: TargetRef,
    pub before_schema_ids: Vec<String>,
    pub after_schema_ids: Vec<String>,
    pub before_diagnostics: Vec<Diagnostic>,
    pub after_diagnostics: Vec<Diagnostic>,
}

/// 影响集合不完整的稳定分类；顺序与协议展示顺序一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaIncompleteReason {
    SourceLoading,
    Syntax,
    AmbiguousDeclaration,
    SchemaDefinition,
}

impl SchemaIncompleteReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::SourceLoading => "源码未完整加载（缺失、不可读、越界或 include 异常）",
            Self::Syntax => "源码存在词法或解析错误",
            Self::AmbiguousDeclaration => "对象身份、属性或声明存在歧义",
            Self::SchemaDefinition => "schema 声明或绑定不完整",
        }
    }

    fn from_diagnostic(diagnostic: &Diagnostic) -> Option<Self> {
        match diagnostic.code {
            "A105" => Some(Self::SourceLoading),
            // 附件的 A109 指向声明；编译器的 A109 指向加载目标或入口文档。
            "A109"
                if matches!(
                    diagnostic.source_role(),
                    Some(DiagnosticSourceRole::Target | DiagnosticSourceRole::Document)
                ) =>
            {
                Some(Self::SourceLoading)
            }
            code if code.starts_with('P') || code.starts_with('L') => Some(Self::Syntax),
            "A104" | "A211" | "A212" | "A220" => Some(Self::AmbiguousDeclaration),
            "SCH001" | "SCH002" | "SCH003" => Some(Self::SchemaDefinition),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaEditPreview {
    pub schema_version: u32,
    pub path: PathBuf,
    pub expected_baseline: String,
    pub plan_digest: String,
    pub changed: bool,
    /// 来源/语法/声明/绑定不完整时仍保留计划和源码，不推断无影响。
    pub complete: bool,
    pub incomplete_reasons: Vec<SchemaIncompleteReason>,
    pub before_diagnostics: Vec<Diagnostic>,
    pub after_diagnostics: Vec<Diagnostic>,
    pub field_changes: Vec<SchemaFieldChange>,
    pub instance_impacts: Vec<SchemaInstanceImpact>,
}

impl Project {
    pub fn schema_index(&self) -> SchemaIndex {
        let compiled = self.compile_current();
        let mut index = super::validate(&compiled.program, &compiled.analysis.catalog);
        index.diagnostics = compiled.diagnostics;
        index
    }

    pub fn preview_schema_edit(
        &self,
        request: &SourceEditRequest,
    ) -> Result<SchemaEditPreview, String> {
        self.prepare_schema_edit(request)
            .map(|(_, preview)| preview)
    }

    /// 允许保存无效草稿；不执行任何实例迁移。快照恢复提供整笔撤销/重做。
    pub fn apply_schema_edit(
        &mut self,
        request: &SourceEditRequest,
        plan_digest: &str,
    ) -> Result<SchemaEditPreview, String> {
        let (candidate, preview) = self.prepare_schema_edit(request)?;
        if preview.plan_digest != plan_digest {
            return Err("schema 影响预览已过期或摘要不符；原稿未改变".into());
        }
        *self = candidate;
        Ok(preview)
    }

    fn prepare_schema_edit(
        &self,
        request: &SourceEditRequest,
    ) -> Result<(Project, SchemaEditPreview), String> {
        if !self.language_version_kind().supports_language_112() {
            return Err("持续 schema 编辑需要显式语言 1.12；旧工程不自动升级".into());
        }
        self.ensure_workspace_writable()?;
        self.checkpoint_disk_baselines_match()?;
        let (candidate, source_preview) = self.prepare_source_edit(request)?;
        let before = self.schema_index();
        let after = candidate.schema_index();
        let field_changes = changes(&before, &after);
        let changed_schemas: BTreeSet<_> =
            field_changes.iter().map(|c| c.schema_id.clone()).collect();
        let impacts = impacts(&before, &after, &changed_schemas);
        let incomplete_reasons: Vec<_> = before
            .diagnostics
            .iter()
            .chain(&after.diagnostics)
            .filter_map(SchemaIncompleteReason::from_diagnostic)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok((
            candidate,
            SchemaEditPreview {
                schema_version: 1,
                path: request.path.clone(),
                expected_baseline: request.expected_baseline.clone(),
                plan_digest: source_preview.plan_digest,
                changed: source_preview.changed,
                complete: incomplete_reasons.is_empty(),
                incomplete_reasons,
                before_diagnostics: before.diagnostics,
                after_diagnostics: after.diagnostics,
                field_changes,
                instance_impacts: impacts,
            },
        ))
    }
}

fn changes(before: &SchemaIndex, after: &SchemaIndex) -> Vec<SchemaFieldChange> {
    let old: BTreeMap<_, _> = before.schemas.iter().map(|s| (&s.id, s)).collect();
    let new: BTreeMap<_, _> = after.schemas.iter().map(|s| (&s.id, s)).collect();
    let ids: BTreeSet<_> = old.keys().chain(new.keys()).copied().collect();
    let mut changes = Vec::new();
    for id in ids {
        let old = old.get(id).copied();
        let new = new.get(id).copied();
        let change = match (old, new) {
            (None, Some(_)) => Some("schema_added"),
            (Some(_), None) => Some("schema_removed"),
            (Some(a), Some(b))
                if a.kind != b.kind || a.entity_type != b.entity_type || a.closed != b.closed =>
            {
                Some("schema_constraints_changed")
            }
            _ => None,
        };
        if let Some(change) = change {
            changes.push(SchemaFieldChange {
                schema_id: id.clone(),
                field_id: None,
                change: change.into(),
                before: None,
                after: None,
            });
        }
        let old_fields: BTreeMap<_, _> = old
            .into_iter()
            .flat_map(|s| &s.fields)
            .map(|f| (&f.id, f))
            .collect();
        let new_fields: BTreeMap<_, _> = new
            .into_iter()
            .flat_map(|s| &s.fields)
            .map(|f| (&f.id, f))
            .collect();
        for field_id in old_fields
            .keys()
            .chain(new_fields.keys())
            .copied()
            .collect::<BTreeSet<_>>()
        {
            let old = old_fields.get(field_id).copied();
            let new = new_fields.get(field_id).copied();
            let change = match (old, new) {
                (None, Some(_)) => "added",
                (Some(_), None) => "removed",
                (Some(a), Some(b)) if a.key != b.key => "renamed",
                (Some(a), Some(b)) if a.value_type != b.value_type => "type_changed",
                (Some(a), Some(b)) if a.required != b.required => "constraints_changed",
                _ => continue,
            };
            changes.push(SchemaFieldChange {
                schema_id: id.clone(),
                field_id: Some(field_id.clone()),
                change: change.into(),
                before: old.cloned(),
                after: new.cloned(),
            });
        }
    }
    changes
}

fn reports(index: &SchemaIndex, target: &TargetRef) -> (Vec<String>, Vec<Diagnostic>) {
    let reports: Vec<&SchemaInstance> = index
        .instances
        .iter()
        .filter(|i| &i.target == target)
        .collect();
    let schemas = reports.iter().map(|i| i.schema_id.clone()).collect();
    let diagnostics = reports
        .iter()
        .flat_map(|i| &i.diagnostics)
        .cloned()
        .collect();
    (schemas, diagnostics)
}

fn impacts(
    before: &SchemaIndex,
    after: &SchemaIndex,
    changed_schemas: &BTreeSet<String>,
) -> Vec<SchemaInstanceImpact> {
    let targets: BTreeSet<_> = before
        .instances
        .iter()
        .chain(&after.instances)
        .map(|i| i.target.clone())
        .collect();
    targets
        .into_iter()
        .filter_map(|target| {
            let (before_schema_ids, before_diagnostics) = reports(before, &target);
            let (after_schema_ids, after_diagnostics) = reports(after, &target);
            if before_schema_ids == after_schema_ids
                && !before_schema_ids
                    .iter()
                    .any(|id| changed_schemas.contains(id))
                && serde_json::to_value(&before_diagnostics).ok()
                    == serde_json::to_value(&after_diagnostics).ok()
            {
                return None;
            }
            Some(SchemaInstanceImpact {
                target,
                before_schema_ids,
                after_schema_ids,
                before_diagnostics,
                after_diagnostics,
            })
        })
        .collect()
}

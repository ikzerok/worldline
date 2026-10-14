use super::{
    model::*,
    protocol::{measure, validate_target, Budget},
};
use crate::{
    ast::{Loc, Program, Property},
    catalog::{CatalogObject, TargetRef},
    project::Project,
    schemas::{SchemaDecl, SchemaIncompleteReason, SchemaIndex, SchemaType},
    CompileResult, Diagnostic,
};
use std::{collections::BTreeSet, path::Path};

pub(super) struct Object<'a> {
    pub file: &'a str,
    pub loc: Loc,
    pub properties: &'a [Property],
    pub entity_type: Option<&'a str>,
}

pub(super) fn object<'a>(program: &'a Program, target: &TargetRef) -> Result<Object<'a>, String> {
    let mut found = Vec::new();
    macro_rules! collect {
        ($values:expr, $id:ident, $subtype:expr) => {
            for value in ($values).iter().filter(|v| v.$id == target.id) {
                found.push(Object {
                    file: &value.file,
                    loc: value.loc,
                    properties: &value.properties,
                    entity_type: ($subtype)(value),
                });
            }
        };
    }
    match target.kind.as_str() {
        "world" => collect!(&program.worlds, name, |_: &'a crate::ast::WorldDecl| None),
        "character" => collect!(
            &program.characters,
            name,
            |_: &'a crate::ast::CharacterDecl| None
        ),
        "entity" => collect!(&program.entities, name, |v: &'a crate::ast::EntityDecl| {
            Some(v.entity_type.as_str())
        }),
        "relation" => collect!(&program.relations, id, |_: &'a crate::ast::RelationDef| {
            None
        }),
        _ => return Err("不支持的 schema 作者对象 kind".into()),
    }
    if found.len() != 1 {
        return Err(format!(
            "对象 {}:{} 不存在或声明身份不唯一；不能猜测编辑目标",
            target.kind, target.id
        ));
    }
    Ok(found.remove(0))
}

impl SchemaIndex {
    /// 复用同一次分析保存的验证结果，保留重复绑定的实例与诊断顺序。
    pub fn from_compile(compiled: &CompileResult) -> Self {
        let mut index = compiled.analysis.schemas.clone();
        index.diagnostics = compiled.diagnostics.clone();
        index
    }
}

pub(super) fn check_workspace_budget(project: &Project) -> Result<(), String> {
    if project
        .documents
        .len()
        .saturating_add(project.authoring_documents.len())
        > 4096
    {
        return Err("schema 作者工作区超过 4096 文件预算".into());
    }
    let mut bytes = 0usize;
    for (path, doc) in &project.documents {
        if doc.is_deleted()
            || project
                .source_selection()
                .is_some_and(|s| !s.is_active(path))
        {
            continue;
        }
        bytes = bytes.saturating_add(doc.text.len());
        if bytes > MAX_SCHEMA_AUTHORING_WORKSPACE_BYTES {
            return Err("schema 作者活动源码超过 32 MiB 预算".into());
        }
    }
    Ok(())
}

pub(super) fn matches_snapshot(project: &Project, compiled: &CompileResult) -> bool {
    if project.compile_options() != compiled.options {
        return false;
    }
    let sources = project.documents.iter().filter(|(path, document)| {
        !document.is_deleted() && project.source_selection().is_none_or(|s| s.is_active(path))
    });
    sources.clone().count() == compiled.sources.len()
        && sources
            .into_iter()
            .all(|(path, document)| compiled.sources.get(path) == Some(&document.text))
}

impl Project {
    pub fn schema_authoring(
        &self,
        target: &TargetRef,
    ) -> Result<SchemaAuthoringProjection, String> {
        check_workspace_budget(self)?;
        self.schema_authoring_from_compile(&self.compile_current(), target)
    }

    pub fn schema_authoring_from_compile(
        &self,
        compiled: &CompileResult,
        target: &TargetRef,
    ) -> Result<SchemaAuthoringProjection, String> {
        validate_target(target)?;
        check_workspace_budget(self)?;
        if !matches_snapshot(self, compiled) {
            return Err("schema 编译来源或能力已过期；请重新取得当前完整快照".into());
        }
        let mut budget = Budget::new();
        budget.charge(target)?;
        let object_source = object(&compiled.program, target)?;
        let object = compiled
            .analysis
            .catalog
            .objects
            .iter()
            .find(|object| &object.target == target)
            .ok_or("对象目录与声明来源不一致")?;
        budget.charge(object)?;
        if object.file != object_source.file || object.line != object_source.loc.line {
            return Err("对象目录与声明来源不一致；不能猜测编辑目标".into());
        }
        let bindings: Vec<_> = compiled
            .program
            .schema_bindings
            .iter()
            .filter(|binding| &binding.target == target)
            .collect();
        let binding = (bindings.len() == 1).then(|| bindings[0]);
        budget.charge(&binding)?;
        let schema = binding.as_ref().and_then(|binding| {
            let mut matches = compiled
                .program
                .schemas
                .iter()
                .filter(|s| s.id == binding.schema_id);
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        });
        budget.charge(&schema)?;
        let compatible_schemas = compatible_schemas(compiled, target, &object_source, &mut budget)?;
        let incomplete_reasons: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter_map(SchemaIncompleteReason::from_diagnostic)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let diagnostics = object_diagnostics(compiled, &object_source, &bindings, schema);
        budget.charge(&diagnostics)?;
        let mut candidate_count = 0;
        let mut fields = Vec::new();
        if let Some(schema) = schema {
            check_schema_budget(schema)?;
            for field in &schema.fields {
                let property = object_source
                    .properties
                    .iter()
                    .find(|p| p.name == field.key);
                budget.charge(field)?;
                budget.charge(&property.map(|p| &p.value))?;
                budget.charge(&object_source.file)?;
                budget.charge(&schema.file)?;
                let field_diagnostics = diagnostics
                    .iter()
                    .filter(|d| {
                        let constraint = d.related.iter().any(|(file, span)| {
                            file == &schema.file && span.line == field.loc.line
                        });
                        let property = property.is_some_and(|p| {
                            d.file == object_source.file && d.span.line == p.loc.line
                        });
                        constraint || property
                    })
                    .copied()
                    .collect::<Vec<_>>();
                budget.charge(&field_diagnostics)?;
                let reference_candidates = reference_candidates(
                    compiled,
                    &field.value_type,
                    &mut candidate_count,
                    &mut budget,
                )?;
                fields.push(SchemaAuthoringField {
                    id: field.id.clone(),
                    key: field.key.clone(),
                    value_type: field.value_type.clone(),
                    required: field.required,
                    present: property.is_some(),
                    value: property.map(|p| p.value.clone()),
                    property_source: property.map(|p| source(object_source.file, p.loc)),
                    constraint_source: source(&schema.file, field.loc),
                    diagnostics: field_diagnostics.into_iter().cloned().collect(),
                    reference_candidates,
                });
            }
        }
        let mut read_only_reasons: Vec<String> = incomplete_reasons
            .iter()
            .map(|r| r.label().into())
            .collect();
        if !self.language_version_kind().supports_language_112() {
            read_only_reasons
                .push("持续 schema 作者表单需要显式语言 1.12；旧工程不自动升级".into());
        }
        if let Err(reason) = self.ensure_workspace_writable() {
            read_only_reasons.push(reason);
        }
        for file in std::iter::once(object_source.file).chain(binding.map(|b| b.file.as_str())) {
            if let Err(reason) = crate::source_lifecycle::safety::writable_path(Path::new(file)) {
                if !read_only_reasons.contains(&reason) {
                    read_only_reasons.push(reason);
                }
            }
        }
        if !self.recovery_conflicts().is_empty() {
            read_only_reasons.push("工程存在未解决的保存事务冲突".into());
        }
        let projection = SchemaAuthoringProjection {
            schema_version: 1,
            baseline: self.content_baseline(),
            target: target.clone(),
            object: object.clone(),
            binding: binding.cloned(),
            schema: schema.cloned(),
            compatible_schemas,
            fields,
            diagnostics: diagnostics.into_iter().cloned().collect(),
            complete: incomplete_reasons.is_empty(),
            incomplete_reasons,
            writable: read_only_reasons.is_empty(),
            read_only_reasons,
        };
        measure(&projection, MAX_SCHEMA_AUTHORING_OUTPUT_BYTES)?;
        Ok(projection)
    }
}

fn source(file: &str, loc: Loc) -> SchemaAuthoringSource {
    SchemaAuthoringSource {
        file: file.into(),
        line: loc.line,
        column: loc.column,
    }
}
fn check_schema_budget(schema: &SchemaDecl) -> Result<(), String> {
    if schema.fields.len() > MAX_SCHEMA_AUTHORING_FIELDS {
        return Err("schema 字段超过 256 项预算".into());
    }
    measure(schema, MAX_SCHEMA_AUTHORING_OUTPUT_BYTES)
}
fn compatible_schemas(
    compiled: &CompileResult,
    target: &TargetRef,
    object: &Object<'_>,
    budget: &mut Budget,
) -> Result<Vec<SchemaDecl>, String> {
    let mut schemas = Vec::new();
    for schema in &compiled.program.schemas {
        if schema.kind != target.kind
            || schema
                .entity_type
                .as_deref()
                .is_some_and(|t| Some(t) != object.entity_type)
        {
            continue;
        }
        check_schema_budget(schema)?;
        if schemas.len() == MAX_SCHEMA_AUTHORING_SCHEMAS {
            return Err("相容 schema 超过 256 项预算".into());
        }
        budget.charge(schema)?;
        schemas.push(schema.clone());
    }
    Ok(schemas)
}
fn reference_candidates(
    compiled: &CompileResult,
    value_type: &SchemaType,
    count: &mut usize,
    budget: &mut Budget,
) -> Result<Vec<CatalogObject>, String> {
    let SchemaType::Ref {
        target_kind,
        entity_type,
    } = value_type
    else {
        return Ok(Vec::new());
    };
    if !crate::catalog::is_object_reference_kind(target_kind, compiled.options) {
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    for object in &compiled.analysis.catalog.objects {
        if object.target.kind != *target_kind {
            continue;
        }
        if entity_type.as_ref().is_some_and(|kind| {
            compiled
                .analysis
                .catalog
                .entities
                .get(&object.target.id)
                .is_none_or(|e| &e.entity_type != kind)
        }) {
            continue;
        }
        if *count == MAX_SCHEMA_AUTHORING_CANDIDATES {
            return Err("schema 引用候选累计超过 10000 项预算".into());
        }
        budget.charge(object)?;
        *count += 1;
        candidates.push(object.clone());
    }
    Ok(candidates)
}
fn object_diagnostics<'a>(
    compiled: &'a CompileResult,
    object: &Object<'_>,
    bindings: &[&crate::schemas::SchemaBinding],
    schema: Option<&SchemaDecl>,
) -> Vec<&'a Diagnostic> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| {
            SchemaIncompleteReason::from_diagnostic(d).is_some()
                || (d.file == object.file
                    && (d.span.line == object.loc.line
                        || object.properties.iter().any(|p| p.loc.line == d.span.line)))
                || bindings
                    .iter()
                    .any(|b| b.file == d.file && b.loc.line == d.span.line)
                || schema.is_some_and(|s| {
                    s.file == d.file
                        && (s.loc.line == d.span.line
                            || s.fields.iter().any(|f| f.loc.line == d.span.line))
                })
        })
        .collect()
}

pub(super) fn relative_path(project: &Project, file: &str) -> Result<std::path::PathBuf, String> {
    let path = Path::new(file);
    crate::authoring_intents::require_active_source(project, path)?;
    path.strip_prefix(&project.root)
        .map(Path::to_path_buf)
        .map_err(|_| "schema 来源不在当前工作区".into())
}

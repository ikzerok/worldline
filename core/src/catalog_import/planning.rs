use super::*;
use crate::project::Project;
use crate::refactor::preview::Edit;
use crate::source_lifecycle::safety;
use std::collections::{BTreeMap, BTreeSet};

impl Project {
    pub fn preview_catalog_import(&self, request: &CatalogImportRequest) -> Result<CatalogImportPlan, String> {
        prepare(self, request).map(|(_, plan)| plan)
    }
    /// 整批重新证明后一次替换内存；不保存，调用者用一个Project快照撤销。
    pub fn apply_catalog_import(&mut self, request: &CatalogImportRequest, plan_digest: &str) -> Result<CatalogImportResult, String> {
        let (candidate, plan) = prepare(self, request)?;
        if plan.plan_digest != plan_digest {
            return Err("资料导入预览已过期或摘要遭篡改，请重新预览；整批未提交".into());
        }
        if !plan.can_apply { return Err("资料导入存在阻断诊断，整批未提交".into()); }
        self.source_lifecycle_disk_baselines_match()?;
        safety::inventory(self).map_err(|failure| failure.message)?;
        for path in &plan.changed_files { safety::writable_path(&self.root.join(path))?; }
        let changed_files = plan.changed_files.clone();
        let new_baseline = candidate.content_baseline();
        if !changed_files.is_empty() { *self = candidate; }
        Ok(CatalogImportResult { plan, changed_files, new_baseline })
    }
}
fn prepare(project: &Project, request: &CatalogImportRequest) -> Result<(Project, CatalogImportPlan), String> {
    if request.schema_version != 1 { return Err("不支持的资料导入DTO版本".into()); }
    project.ensure_workspace_writable()?;
    if request.expected_baseline != project.content_baseline() { return Err("资料导入基线已过期，请重新预览".into()); }
    safety::relative(&request.destination)?;
    safety::inventory(project).map_err(|failure| failure.message)?;
    project.source_lifecycle_disk_baselines_match()?;
    let destination = crate::file_access::within(&project.root, &project.root.join(&request.destination))?;
    if !project.sources().contains_key(&destination) { return Err("导入目标必须是已载入的活动.wl源码".into()); }
    let content = project.compile_current();
    let mut plan = CatalogImportPlan {
        schema_version: 1, baseline: project.content_baseline(),
        input_digest: digest(request.csv.as_bytes()), plan_digest: String::new(),
        destination: request.destination.clone(), ignored_columns: Vec::new(), normalization_count: 0,
        rows: Vec::new(), diagnostics: Vec::new(), error_count: 0, can_apply: false,
        changed_files: Vec::new(), runtime_fingerprint_before: content.analysis.fingerprint,
        runtime_fingerprint_after: None,
    };
    let table = match parse_catalog_csv(&request.csv) {
        Ok(table) => table,
        Err(error) => { diagnostic(&mut plan, error); return finish(project, project.clone(), request, plan); }
    };
    plan.normalization_count = table.normalization_count;
    if let Err(message) = mapping::validate(&request.columns, table.headers.len()) {
        diagnostic(&mut plan, CatalogImportDiagnostic::new("IMPORT_MAPPING", message));
        return finish(project, project.clone(), request, plan);
    }
    plan.ignored_columns = request.columns.iter().filter(|map| map.field == CatalogImportField::Ignore).map(|map| table.headers[map.column].clone()).collect();
    for error in content.diagnostics.iter().filter(|error| error.severity == crate::Severity::Error) {
        diagnostic(&mut plan, CatalogImportDiagnostic::new("IMPORT_BASELINE", format!("无效基线工程：{} {}:{} {}", error.code, error.file, error.span.line, error.message)));
    }
    let baseline_invalid = plan.error_count > 0;
    let mut patches: BTreeMap<PathBuf, Vec<Edit>> = BTreeMap::new();
    let mut additions: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut identities = BTreeSet::new();
    let mut syntax_cache = BTreeMap::new();
    let mapping_context = mapping::Context::new(project);
    let mut row_preview_bytes = 0usize;
    for (index, record) in table.rows.iter().enumerate() {
        let row_number = index+2;
        let mut row = CatalogImportRow { row: row_number, line: record.line, target: None, operation: "blocked".into(), source: None, fields: Vec::new() };
        let mut duplicate = false;
        if let Some(target) = mapping::identity(record, &request.columns) {
            row.source = content.analysis.catalog.object(&target).map(|object| relative(project, std::path::Path::new(&object.file))).transpose()?;
            duplicate = !identities.insert((target.kind.clone(),target.id.clone()));
            row.target = Some(target);
            if duplicate { diagnostic(&mut plan, CatalogImportDiagnostic::new("IMPORT_DUPLICATE", "CSV包含重复kind+id；整批拒绝").at(row_number, 1, record.line)); }
        }
        let mapped = mapping::row(record, row_number, &request.columns, &mapping_context);
        match mapped {
            Err(errors) => { for error in errors { diagnostic(&mut plan, error); } },
            Ok(mapped) => {
                row.target = Some(mapped.target.clone());
                if !duplicate && !baseline_invalid {
                    match patch::prepare(project, &content, &destination, &mapped, &mut syntax_cache) {
                        Err(message) => diagnostic(&mut plan, CatalogImportDiagnostic::new("IMPORT_PATCH", message).at(row_number, 1, record.line)),
                        Ok(prepared) => {
                            row.source = Some(relative(project, &prepared.path)?);
                            row.operation = prepared.operation;
                            row.fields = prepared.fields;
                            patches.entry(prepared.path.clone()).or_default().extend(prepared.edits);
                            additions.entry(prepared.path).or_default().push_str(&prepared.append);
                        }
                    }
                }
            }
        }
        row_preview_bytes = row_preview_bytes.saturating_add(serde_json::to_vec(&row).map_err(|error| error.to_string())?.len());
        if row_preview_bytes > MAX_PREVIEW_BYTES { return Err("资料导入逐行预览超过4 MiB预算，整批未提交；请缩小批次".into()); }
        plan.rows.push(row);
    }
    let mut candidate = project.clone();
    if plan.error_count == 0 {
        for (path, edits) in patches {
            let source = project.document(&path)?;
            let mut after = patch::apply_fields(source, edits)?;
            let append = additions.get(&path).map(String::as_str).unwrap_or_default();
            if !append.is_empty() {
                patch::safe_insertion(&after, after.len())?;
                let eol = patch::newline(source);
                if !after.is_empty() && !after.ends_with('\n') { after.push_str(eol); }
                after.push_str(eol);
                after.push_str(append);
            }
            if after != source {
                safety::writable_path(&path)?;
                candidate.set_text(&path, after)?;
                plan.changed_files.push(relative(project, &path)?);
            }
        }
        safety::buffer_budget(&candidate)?;
        let compiled = candidate.compile_current();
        for error in compiled.diagnostics.iter().filter(|error| error.severity == crate::Severity::Error) {
            diagnostic(&mut plan, CatalogImportDiagnostic::new(error.code, format!("候选{}:{} {}", error.file, error.span.line, error.message)));
        }
        if plan.error_count == 0 { plan.runtime_fingerprint_after = Some(compiled.analysis.fingerprint); }
    }
    plan.can_apply = plan.error_count == 0;
    finish(project, candidate, request, plan)
}
fn diagnostic(plan: &mut CatalogImportPlan, error: CatalogImportDiagnostic) {
    plan.error_count += 1;
    if plan.diagnostics.len() < MAX_DIAGNOSTICS { plan.diagnostics.push(error); }
}
fn relative(project: &Project, path: &std::path::Path) -> Result<PathBuf, String> {
    path.strip_prefix(&project.root).map(std::path::Path::to_path_buf).map_err(|_| "导入文件不在工程内".into())
}
fn finish(project: &Project, candidate: Project, request: &CatalogImportRequest, mut plan: CatalogImportPlan) -> Result<(Project, CatalogImportPlan), String> {
    let bytes = serde_json::to_vec(&("catalog-import-v1", &project.root, request, &plan, candidate.content_baseline())).map_err(|error| error.to_string())?;
    plan.plan_digest = digest(&bytes);
    if serde_json::to_vec(&plan).map_err(|error| error.to_string())?.len() > MAX_PREVIEW_BYTES {
        return Err("资料导入完整预览超过4 MiB预算，整批未提交；请缩小批次".into());
    }
    Ok((candidate, plan))
}

//! 跨源码与展示文档的稳定 TargetRef 重命名计划。
use crate::catalog::TargetRef;
use crate::project::Project;
use crate::{CompileResult, Severity};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct RefactorChange {
    pub path: PathBuf,
    pub kind: String,
    pub reference_count: usize,
    #[serde(skip)]
    before: Vec<u8>,
    #[serde(skip)]
    after: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenamePlan {
    pub target: TargetRef,
    pub new_id: String,
    pub content_baseline: String,
    pub changes: Vec<RefactorChange>,
    pub explicit_references: usize,
}
impl Project {
    pub fn plan_rename_target(
        &self,
        target: &TargetRef,
        new_id: &str,
    ) -> Result<RenamePlan, String> {
        if !matches!(target.kind.as_str(), "entity" | "relation") {
            return Err("首版跨视图 ID 重命名只支持 entity / relation".into());
        }
        crate::authoring::identifier(new_id)?;
        if target.id == new_id {
            return Err("新 ID 与当前 ID 相同".into());
        }
        let content = self.compile_current();
        let object = content
            .analysis
            .catalog
            .object(target)
            .cloned()
            .ok_or("待重命名对象不存在")?;
        if content
            .analysis
            .catalog
            .object(&TargetRef::new(&target.kind, new_id))
            .is_some()
        {
            return Err("新 ID 已被同类型对象使用".into());
        }
        let impact = self.deletion_impact(target);
        if !impact.complete {
            return Err("引用检查不完整，请先修复内容或展示文档诊断".into());
        }
        let mut lines = BTreeMap::<PathBuf, BTreeSet<u32>>::new();
        lines
            .entry(PathBuf::from(&object.file))
            .or_default()
            .insert(object.line);
        for reference in &impact.content_references {
            lines
                .entry(PathBuf::from(&reference.file))
                .or_default()
                .insert(reference.line);
        }

        let mut changes = Vec::new();
        for (path, line_numbers) in lines {
            let before = self
                .documents
                .get(&path)
                .filter(|document| !document.is_deleted())
                .map(|document| document.text.as_bytes().to_vec())
                .ok_or_else(|| format!("源码未载入：{}", path.display()))?;
            let text = String::from_utf8(before.clone()).map_err(|_| "源码不是 UTF-8")?;
            let (after, count) = rewrite_source(&text, &line_numbers, target, new_id);
            if count == 0 {
                return Err(format!("无法安全定位重命名位置：{}", path.display()));
            }
            changes.push(RefactorChange {
                path,
                kind: "source".into(),
                reference_count: count,
                before,
                after: after.into_bytes(),
            });
        }
        let manifest_path = crate::workspace_documents::manifest_path(&self.root);
        let manuscript_paths: BTreeSet<PathBuf> = self
            .authoring_document(&manifest_path)
            .ok()
            .map(|document| {
                let registry =
                    crate::workspace_documents::parse_registry(&self.root, document.bytes());
                if registry.diagnostics.is_empty() {
                    registry.manuscripts.into_values().collect()
                } else {
                    BTreeSet::new()
                }
            })
            .unwrap_or_default();
        for (path, document) in &self.authoring_documents {
            if document.is_deleted() {
                continue;
            }
            if document.is_read_only() {
                return Err(format!(
                    "展示文档为只读，无法证明重命名完整：{}",
                    path.display()
                ));
            }
            let before = document.bytes().to_vec();
            let mut value =
                crate::workspace_documents::parse_unique_json(&before).map_err(|error| {
                    format!("展示文档 JSON 无法安全读取：{}：{error}", path.display())
                })?;
            let count = if manuscript_paths.contains(path) {
                rewrite_manuscript_json(&mut value, target, new_id)
            } else {
                rewrite_json(&mut value, target, new_id)
            };
            if count == 0 {
                continue;
            }
            let after = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
            changes.push(RefactorChange {
                path: path.clone(),
                kind: "authoring".into(),
                reference_count: count,
                before,
                after,
            });
        }

        let explicit_references = changes.iter().map(|change| change.reference_count).sum();
        let plan = RenamePlan {
            target: target.clone(),
            new_id: new_id.into(),
            content_baseline: self.content_baseline(),
            changes,
            explicit_references,
        };
        let mut candidate = self.clone();
        apply_plan_bytes(&mut candidate, &plan)?;
        validate_candidate(&candidate, &content, target)?;
        Ok(plan)
    }

    pub fn apply_rename_plan(&mut self, plan: &RenamePlan) -> Result<(), String> {
        if self.content_baseline() != plan.content_baseline {
            return Err("重命名预览已过期，请重新生成影响计划".into());
        }
        let current = self.compile_current();
        if current.analysis.catalog.object(&plan.target).is_none() {
            return Err("待重命名对象已不存在".into());
        }
        for change in &plan.changes {
            let actual = current_bytes(self, change)?;
            if actual != change.before {
                return Err(format!(
                    "重命名目标文件已变化，整批未提交：{}",
                    change.path.display()
                ));
            }
        }
        let mut candidate = self.clone();
        apply_plan_bytes(&mut candidate, plan)?;
        validate_candidate(&candidate, &current, &plan.target)?;
        *self = candidate;
        Ok(())
    }
}
fn current_bytes(project: &Project, change: &RefactorChange) -> Result<Vec<u8>, String> {
    match change.kind.as_str() {
        "source" => project
            .documents
            .get(&change.path)
            .filter(|document| !document.is_deleted())
            .map(|document| document.text.as_bytes().to_vec())
            .ok_or_else(|| format!("源码已不存在：{}", change.path.display())),
        "authoring" => project
            .authoring_document(&change.path)
            .map(|document| document.bytes().to_vec()),
        _ => Err("未知重构文件类型".into()),
    }
}

fn apply_plan_bytes(project: &mut Project, plan: &RenamePlan) -> Result<(), String> {
    for change in &plan.changes {
        match change.kind.as_str() {
            "source" => {
                let text = String::from_utf8(change.after.clone())
                    .map_err(|_| "重构后的源码不是 UTF-8")?;
                project.set_text(&change.path, text)?;
            }
            "authoring" => {
                project.set_authoring_document(&change.path, change.after.clone())?;
            }
            _ => return Err("未知重构文件类型".into()),
        }
    }
    Ok(())
}
fn validate_candidate(
    project: &Project,
    before: &CompileResult,
    old_target: &TargetRef,
) -> Result<(), String> {
    let compiled = project.compile_current();
    if let Some(error) = compiled
        .diagnostics
        .iter()
        .find(|item| item.severity == Severity::Error)
    {
        return Err(format!("{} {}", error.code, error.message));
    }
    if compiled.analysis.catalog.object(old_target).is_some() {
        return Err("旧 ID 在提交候选中仍然存在".into());
    }
    let maps = crate::presentation::build_map_index(project, &compiled);
    if let Some(error) = maps
        .diagnostics
        .iter()
        .find(|item| item.severity == Severity::Error)
    {
        return Err(format!("{} {}", error.code, error.message));
    }
    let views = crate::graph_views::build_graph_view_index(project, &compiled);
    if let Some(error) = views
        .diagnostics
        .iter()
        .find(|item| item.severity == Severity::Error)
    {
        return Err(format!("{} {}", error.code, error.message));
    }
    let presets =
        crate::presentation_presets::build_preset_index(project, &compiled, &maps, &views);
    if let Some(error) = presets
        .diagnostics
        .iter()
        .find(|item| item.severity == Severity::Error)
    {
        return Err(format!("{} {}", error.code, error.message));
    }
    let comments = crate::collaboration::build_comment_index(project, &compiled, &maps);
    if let Some(error) = comments
        .diagnostics
        .iter()
        .find(|item| item.severity == Severity::Error)
    {
        return Err(format!("{} {}", error.code, error.message));
    }
    let proposals = crate::collaboration::build_proposal_index(project);
    if let Some(error) = proposals
        .diagnostics
        .iter()
        .find(|item| item.severity == Severity::Error)
    {
        return Err(format!("{} {}", error.code, error.message));
    }
    for manuscript in project.manuscript_indices().values() {
        if let Some(error) = manuscript
            .diagnostics
            .iter()
            .find(|item| item.severity == Severity::Error)
        {
            return Err(format!("{} {}", error.code, error.message));
        }
        if !manuscript.references_to(old_target).is_empty() {
            return Err("重命名候选仍含有旧 ID 的书稿引用".into());
        }
    }
    if before.analysis.fingerprint != compiled.analysis.fingerprint {
        return Err("entity / relation ID 重命名不应改变运行指纹".into());
    }
    Ok(())
}
fn rewrite_source(
    source: &str,
    lines: &BTreeSet<u32>,
    target: &TargetRef,
    new_id: &str,
) -> (String, usize) {
    let mut out = String::with_capacity(source.len());
    let mut count = 0usize;
    for (index, part) in source.split_inclusive('\n').enumerate() {
        let line = index as u32 + 1;
        if lines.contains(&line) {
            let (rewritten, hits) = rewrite_source_line(part, target, new_id);
            out.push_str(&rewritten);
            count += hits;
        } else {
            out.push_str(part);
        }
    }
    if source.is_empty() {
        return (out, count);
    }
    if !source.ends_with('\n')
        && source.lines().count() as u32 > lines.iter().copied().max().unwrap_or(0)
    {
        return (out, count);
    }
    (out, count)
}

fn rewrite_source_line(line: &str, target: &TargetRef, new_id: &str) -> (String, usize) {
    let mut text = line.to_string();
    let mut count = 0usize;
    let link_old = format!("[[{}:{}|", target.kind, target.id);
    let link_new = format!("[[{}:{}|", target.kind, new_id);
    let hits = text.matches(&link_old).count();
    if hits > 0 {
        text = text.replace(&link_old, &link_new);
        count += hits;
    }

    let pair_old = format!("{} {}", target.kind, target.id);
    let pair_new = format!("{} {}", target.kind, new_id);
    let (rewritten, hits) = replace_structural_pair(&text, &pair_old, &pair_new);
    text = rewritten;
    count += hits;

    if target.kind == "relation" {
        let decl_old = format!("relation_def {}", target.id);
        let decl_new = format!("relation_def {new_id}");
        let (rewritten, hits) = replace_structural_pair(&text, &decl_old, &decl_new);
        text = rewritten;
        count += hits;
    }
    (text, count)
}

fn replace_structural_pair(text: &str, old: &str, new: &str) -> (String, usize) {
    let mut result = String::new();
    let mut rest = text;
    let mut count = 0;
    while let Some(index) = rest.find(old) {
        let before_ok = index == 0 || !rest[..index].chars().next_back().is_some_and(is_ident_char);
        let end = index + old.len();
        let after_ok = end == rest.len() || !rest[end..].chars().next().is_some_and(is_ident_char);
        if before_ok && after_ok {
            result.push_str(&rest[..index]);
            result.push_str(new);
            rest = &rest[end..];
            count += 1;
        } else {
            let split = index + old.chars().next().map(char::len_utf8).unwrap_or(1);
            result.push_str(&rest[..split]);
            rest = &rest[split..];
        }
    }
    result.push_str(rest);
    (result, count)
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

fn rewrite_json(value: &mut Value, target: &TargetRef, new_id: &str) -> usize {
    match value {
        Value::Array(items) => items
            .iter_mut()
            .map(|item| rewrite_json(item, target, new_id))
            .sum(),
        Value::Object(object) => {
            let mut count = 0usize;
            if object.get("kind").and_then(Value::as_str) == Some(target.kind.as_str())
                && object.get("id").and_then(Value::as_str) == Some(target.id.as_str())
            {
                object.insert("id".into(), Value::String(new_id.into()));
                count += 1;
            }
            let old_key = format!("{}:{}", target.kind, target.id);
            if let Some(position) = object.remove(&old_key) {
                object.insert(format!("{}:{new_id}", target.kind), position);
                count += 1;
            }
            for (key, child) in object.iter_mut() {
                if target.kind == "relation" && key == "hidden_relation_ids" {
                    if let Some(items) = child.as_array_mut() {
                        for item in items {
                            if item.as_str() == Some(target.id.as_str()) {
                                *item = Value::String(new_id.into());
                                count += 1;
                            }
                        }
                    }
                } else {
                    count += rewrite_json(child, target, new_id);
                }
            }
            count
        }
        _ => 0,
    }
}

fn rewrite_manuscript_json(value: &mut Value, target: &TargetRef, new_id: &str) -> usize {
    let Some(entries) = value
        .as_object_mut()
        .and_then(|object| object.get_mut("entries"))
        .and_then(Value::as_array_mut)
    else {
        return 0;
    };
    let mut count = 0;
    for entry in entries.iter_mut().filter_map(Value::as_object_mut) {
        let Some(reference) = entry.get_mut("target_ref").and_then(Value::as_object_mut) else {
            continue;
        };
        if reference.get("kind").and_then(Value::as_str) == Some(target.kind.as_str())
            && reference.get("id").and_then(Value::as_str) == Some(target.id.as_str())
        {
            reference.insert("id".into(), Value::String(new_id.into()));
            count += 1;
        }
    }
    count
}

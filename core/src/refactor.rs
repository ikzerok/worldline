//! 跨源码与展示文档的稳定 TargetRef 重命名计划。
mod documents;
mod json_spans;
mod language;
mod preview;
pub use preview::{RefactorByteRange, RefactorOccurrence};
mod property;
mod text;
use crate::catalog::TargetRef;
use crate::project::Project;
use crate::{CompileResult, Severity};
use documents::rewrite_registered;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefactorChange {
    pub path: PathBuf,
    pub kind: String,
    pub reference_count: usize,
    pub occurrences: Vec<RefactorOccurrence>,
    #[serde(skip)]
    before: Vec<u8>,
    #[serde(skip)]
    after: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenamePlan {
    pub target: TargetRef,
    pub new_id: String,
    pub content_baseline: String,
    pub changes: Vec<RefactorChange>,
    pub explicit_references: usize,
    pub runtime_fingerprint_before: u64,
    pub runtime_fingerprint_after: u64,
}
impl Project {
    pub fn plan_rename_target(
        &self,
        target: &TargetRef,
        new_id: &str,
    ) -> Result<RenamePlan, String> {
        self.ensure_workspace_writable()?;
        ensure_source_inventory(self)?;
        if !matches!(
            target.kind.as_str(),
            "entity" | "relation" | "rule" | "fragment" | "character" | "tag" | "state"
        ) {
            return Err("此对象类型暂不支持跨视图 ID 重命名".into());
        }
        if !matches!(target.kind.as_str(), "entity" | "relation")
            && !self.language_version_kind().supports_language_111()
        {
            return Err("此类身份的统一重命名需要显式语言 1.11；旧作品不自动升级".into());
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
        for reference in content
            .analysis
            .catalog
            .references
            .iter()
            .filter(|reference| {
                reference.kind == "对象属性引用"
                    && reference.source == *target
                    && reference.target == *target
            })
        {
            lines
                .entry(PathBuf::from(&reference.file))
                .or_default()
                .insert(reference.line);
        }
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
            let (after, occurrences, count) =
                rewrite_source(&text, &line_numbers, target, new_id, self.compile_options())?;
            if count == 0 {
                return Err(format!("无法安全定位重命名位置：{}", path.display()));
            }
            changes.push(RefactorChange {
                path,
                kind: "source".into(),
                reference_count: count,
                occurrences,
                before,
                after: after.into_bytes(),
            });
        }
        let manifest_path = crate::workspace_documents::manifest_path(&self.root);
        let registry = self
            .authoring_document(&manifest_path)
            .ok()
            .map(|document| {
                crate::workspace_documents::parse_registry(&self.root, document.bytes())
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
            let original = value.clone();
            let count = rewrite_registered(&mut value, &registry, path, target, new_id);
            if count == 0 {
                continue;
            }
            let source = std::str::from_utf8(&before).map_err(|_| "展示文档不是 UTF-8")?;
            let edits = json_spans::edits(source, &original, &value, target, new_id)?;
            let (after, occurrences) = preview::apply(source, edits)?;
            if occurrences.len() != count
                || serde_json::from_str::<serde_json::Value>(&after)
                    .map_err(|error| error.to_string())?
                    != value
            {
                return Err("展示文档逐处预览与候选不匹配".into());
            }
            changes.push(RefactorChange {
                path: path.clone(),
                kind: "authoring".into(),
                reference_count: count,
                occurrences,
                before,
                after: after.into_bytes(),
            });
        }

        let explicit_references = changes.iter().map(|change| change.reference_count).sum();
        let mut plan = RenamePlan {
            target: target.clone(),
            new_id: new_id.into(),
            content_baseline: self.content_baseline(),
            changes,
            explicit_references,
            runtime_fingerprint_before: content.analysis.fingerprint,
            runtime_fingerprint_after: content.analysis.fingerprint,
        };
        let mut candidate = self.clone();
        apply_plan_bytes(&mut candidate, &plan)?;
        plan.runtime_fingerprint_after = validate_candidate(&candidate, &content, target)?;
        Ok(plan)
    }

    pub fn apply_rename_plan(&mut self, plan: &RenamePlan) -> Result<(), String> {
        self.ensure_workspace_writable()?;
        self.checkpoint_disk_baselines_match()?;
        if self.content_baseline() != plan.content_baseline {
            return Err("重命名预览已过期，请重新生成影响计划".into());
        }
        let expected = self.plan_rename_target(&plan.target, &plan.new_id)?;
        if &expected != plan {
            return Err("重命名计划或逐处预览已变化，整批未提交，请重新生成计划".into());
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
fn ensure_source_inventory(project: &Project) -> Result<(), String> {
    let paths = match crate::file_access::workspace_files(&project.root) {
        Ok(paths) => paths,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(format!("无法检查重命名源码集合：{error}")),
    };
    if let Some(path) = paths.iter().find(|path| {
        path.extension().is_some_and(|extension| extension == "wl")
            && !project.documents.contains_key(*path)
    }) {
        return Err(format!(
            "磁盘新增源码尚未载入，整批未提交，请刷新后重新预览：{}",
            path.display()
        ));
    }
    Ok(())
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
) -> Result<u64, String> {
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
    if let Some(error) = project
        .template_index()
        .diagnostics
        .iter()
        .find(|item| item.severity == Severity::Error)
    {
        return Err(format!("{} {}", error.code, error.message));
    }
    if matches!(old_target.kind.as_str(), "entity" | "relation")
        && before.analysis.fingerprint != compiled.analysis.fingerprint
    {
        let affected = before
            .analysis
            .catalog
            .states
            .values()
            .filter(|state| state.target == *old_target)
            .map(|state| format!("state {}（{}:{}）", state.id, state.file, state.line))
            .collect::<Vec<_>>();
        let reason = if affected.is_empty() {
            "此候选改变了运行身份或可见正文".to_string()
        } else {
            format!(
                "受影响状态：{}。状态所属对象的稳定 ID 参与运行指纹",
                affected.join("、")
            )
        };
        let alternative = if old_target.kind == "entity" {
            "请保留实体稳定 ID，仅修改实体声明的 as \"显示名\"。"
        } else {
            "请保留关系稳定 ID，使用 alias relation ID as \"别名\" 修改对象别名；若修改关系类型显示文字，会影响同类关系的显示，但不改变关系 ID。"
        };
        return Err(format!(
            "已安全拒绝稳定 ID 重命名，工程未修改。{reason}。旧 fingerprint={}；候选 fingerprint={}。旧 Story 存档（save）与检查点的 fingerprint 不匹配，不能直接载入此候选；入口轨迹（replay）须按当前稿重新受控验证，不能保证沿用。不会修改或自动迁移旧文件。{alternative}",
            before.analysis.fingerprint, compiled.analysis.fingerprint,
        ));
    }
    Ok(compiled.analysis.fingerprint)
}
fn rewrite_source(
    source: &str,
    lines: &BTreeSet<u32>,
    target: &TargetRef,
    new_id: &str,
    options: crate::CompileOptions,
) -> Result<(String, Vec<RefactorOccurrence>, usize), String> {
    if matches!(target.kind.as_str(), "entity" | "relation") {
        let edits = crate::lexer::identity_source_spans("rename.wl", source, options)
            .into_iter()
            // Catalog 的部分引用位置指向所属声明（例如 scope_ref），不等于 token 所在行。
            // 文件由语义引用闭包选定；文件内仅正式身份 span 决定实际修改范围。
            .filter(|span| span.target == *target)
            .map(|span| preview::Edit {
                range: span.range,
                replacement: new_id.into(),
                field: span.field,
            })
            .collect();
        let (after, occurrences) = preview::apply(source, edits)?;
        let count = occurrences.len();
        return Ok((after, occurrences, count));
    }
    let mut after = String::new();
    let mut edits = Vec::new();
    let mut offset = 0;
    let mut count = 0;
    let property_ranges = property::reference_ranges(source, target);
    for (index, part) in source.split_inclusive('\n').enumerate() {
        let line = index as u32 + 1;
        let (rewritten, hits) = if lines.contains(&line) {
            match property_ranges.get(&line) {
                Some(Some(range)) => property::rewrite(part, range.clone(), new_id),
                Some(None) => (part.to_owned(), 0),
                None => language::rewrite(part, target, new_id),
            }
        } else {
            (part.to_owned(), 0)
        };
        count += hits;
        for mut edit in preview::legacy_edits(part, &rewritten, &target.id, new_id) {
            edit.range = offset + edit.range.start..offset + edit.range.end;
            edits.push(edit);
        }
        offset += part.len();
        after.push_str(&rewritten);
    }
    let (actual, occurrences) = preview::apply(source, edits)?;
    if actual != after {
        return Err("逐处预览与语言重构候选不匹配".into());
    }
    Ok((actual, occurrences, count))
}

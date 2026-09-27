use super::source::{
    append_relation_source, line_end, line_start, relation_source, relation_type_source,
    replace_relation_block,
};
use super::{
    LegacyRelationHandle, LegacyRelationPromotionDraft, RelationDraft, RelationPromotionPreview,
    RelationTypeDraft,
};
use crate::catalog::TargetRef;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Project 结构编辑与旧关系提升
// ---------------------------------------------------------------------------

impl crate::project::Project {
    fn ensure_relation_capability(&self) -> Result<(), String> {
        if !self.language_version_kind().supports_relations() {
            return Err("关系编辑需要显式语言版本 1.10".into());
        }
        let path = crate::workspace_documents::manifest_path(&self.root);
        let document = self.authoring_document(&path)?;
        let manifest: serde_json::Value = serde_json::from_slice(document.bytes())
            .map_err(|error| format!("无法读取关系能力清单：{error}"))?;
        if document.is_deleted()
            || !manifest
                .get("required_features")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|features| {
                    features
                        .iter()
                        .any(|feature| feature.as_str() == Some("content.relations.v1"))
                })
        {
            return Err("关系编辑需要清单声明 content.relations.v1".into());
        }
        Ok(())
    }

    /// 创建或修改关系类型。调用方应在 `Project::edit` 外直接使用本方法；
    /// 方法自身沿用 clone→compile→提交的整批事务边界。
    pub fn write_relation_type(
        &mut self,
        original: Option<&str>,
        draft: &RelationTypeDraft,
    ) -> Result<(), String> {
        let draft = draft.clone();
        let original = original.map(str::to_string);
        self.edit(move |candidate| candidate.write_relation_type_raw(original.as_deref(), &draft))
    }

    fn write_relation_type_raw(
        &mut self,
        original: Option<&str>,
        draft: &RelationTypeDraft,
    ) -> Result<(), String> {
        self.ensure_relation_capability()?;
        crate::authoring::identifier(&draft.id)?;
        if draft.display.trim().is_empty() {
            return Err("关系类型显示名不能为空".into());
        }
        if let Some(kind) = &draft.from_kind {
            validate_relation_kind(kind)?;
        }
        if let Some(kind) = &draft.to_kind {
            validate_relation_kind(kind)?;
        }
        if original.is_some_and(|id| id != draft.id) {
            return Err("关系类型 ID 是引用身份,修改资料时请保留 ID".into());
        }
        let result = self.compile_current();
        let existing = result.analysis.catalog.relation_types.get(&draft.id);
        if original.is_some() && existing.is_none() {
            return Err("待修改的关系类型不存在".into());
        }
        if original.is_none() && existing.is_some() {
            return Err("关系类型 ID 已存在".into());
        }
        let path = existing
            .map(|info| PathBuf::from(&info.file))
            .unwrap_or_else(|| self.entry.clone());
        let source = relation_type_source(draft)?;
        if let Some(info) = existing {
            replace_relation_block(self, &path, info.line, true, &source)
        } else {
            append_relation_source(self, &path, &source)
        }
    }

    /// 创建或修改一个独立关系实例。
    pub fn write_relation(
        &mut self,
        original: Option<&str>,
        draft: &RelationDraft,
    ) -> Result<(), String> {
        let draft = draft.clone();
        let original = original.map(str::to_string);
        self.edit(move |candidate| candidate.write_relation_raw(original.as_deref(), &draft))
    }

    fn write_relation_raw(
        &mut self,
        original: Option<&str>,
        draft: &RelationDraft,
    ) -> Result<(), String> {
        self.ensure_relation_capability()?;
        crate::authoring::identifier(&draft.id)?;
        crate::authoring::identifier(&draft.relation_type)?;
        validate_relation_target(&draft.from, self.compile_options())?;
        validate_relation_target(&draft.to, self.compile_options())?;
        for reference in &draft.scope_refs {
            validate_relation_target(reference, self.compile_options())?;
        }
        if original.is_some_and(|id| id != draft.id) {
            return Err("关系 ID 是引用身份,修改资料时请保留 ID".into());
        }
        let result = self.compile_current();
        let existing = result.analysis.catalog.relations.get(&draft.id);
        if original.is_some() && existing.is_none() {
            return Err("待修改的关系不存在".into());
        }
        if original.is_none() && existing.is_some() {
            return Err("关系 ID 已存在".into());
        }
        if !result
            .analysis
            .catalog
            .relation_types
            .contains_key(&draft.relation_type)
        {
            return Err(format!("关系类型 `{}` 不存在", draft.relation_type));
        }
        let path = existing
            .map(|info| PathBuf::from(&info.file))
            .unwrap_or_else(|| self.entry.clone());
        for target in std::iter::once(&draft.from)
            .chain(std::iter::once(&draft.to))
            .chain(draft.scope_refs.iter())
        {
            validate_relation_file_target(&self.root, target)?;
        }
        let source = relation_source(draft, &path)?;
        if let Some(info) = existing {
            replace_relation_block(self, &path, info.line, false, &source)
        } else {
            append_relation_source(self, &path, &source)
        }
    }

    /// 删除关系；地图/视图等显式关系引用会沿既有删除影响边界阻止提交。
    pub fn remove_relation(&mut self, id: &str) -> Result<(), String> {
        let id = id.to_string();
        self.edit(move |candidate| candidate.remove_relation_raw(&id))
    }

    fn remove_relation_raw(&mut self, id: &str) -> Result<(), String> {
        self.ensure_relation_capability()?;
        let result = self.compile_current();
        let relation = result
            .analysis
            .catalog
            .relations
            .get(id)
            .cloned()
            .ok_or("关系不存在")?;
        let impact = self.deletion_impact(&TargetRef::new("relation", id));
        if !impact.complete {
            return Err("引用检查不完整，请先修复内容或地图诊断，再删除关系".into());
        }
        if !impact.content_references.is_empty() {
            return Err(format!("关系 `{id}` 仍被正文或目录引用，请先解除这些引用"));
        }
        if !impact.map_placements.is_empty()
            || !impact.map_scopes.is_empty()
            || !impact.graph_views.is_empty()
            || !impact.comments.is_empty()
            || !impact.manuscripts.is_empty()
        {
            return Err(format!(
                "关系 `{id}` 仍被展示或批注文档引用，请先解除这些引用"
            ));
        }
        replace_relation_block(
            self,
            &PathBuf::from(&relation.file),
            relation.line,
            false,
            "",
        )
    }

    pub fn remove_relation_type(&mut self, id: &str) -> Result<(), String> {
        let id = id.to_string();
        self.edit(move |candidate| candidate.remove_relation_type_raw(&id))
    }

    fn remove_relation_type_raw(&mut self, id: &str) -> Result<(), String> {
        self.ensure_relation_capability()?;
        let result = self.compile_current();
        let relation_type = result
            .analysis
            .catalog
            .relation_types
            .get(id)
            .cloned()
            .ok_or("关系类型不存在")?;
        if result
            .analysis
            .catalog
            .relations
            .values()
            .any(|relation| relation.relation_type == id)
        {
            return Err(format!(
                "关系类型 `{id}` 仍被关系实例使用，请先删除或修改这些关系"
            ));
        }
        let views = crate::graph_views::build_graph_view_index(self, &result);
        if views
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::Severity::Error)
        {
            return Err("网络视图引用检查不完整，不能删除关系类型".into());
        }
        let referencing_views = views
            .views
            .values()
            .filter(|view| {
                view.draft
                    .filters
                    .relation_types
                    .iter()
                    .any(|reference| reference == id)
            })
            .map(|view| view.draft.id.clone())
            .collect::<Vec<_>>();
        if !referencing_views.is_empty() {
            return Err(format!(
                "关系类型 `{id}` 仍被共享视图筛选引用，请先修改筛选：{}",
                referencing_views.join("、")
            ));
        }
        replace_relation_block(
            self,
            &PathBuf::from(&relation_type.file),
            relation_type.line,
            true,
            "",
        )
    }

    /// 预览旧人物关系提升；此调用不修改 Project。
    pub fn preview_promote_legacy_relation(
        &self,
        handle: &LegacyRelationHandle,
        draft: &RelationDraft,
    ) -> Result<RelationPromotionPreview, String> {
        if draft.from != handle.source || draft.to != handle.target {
            return Err("提升关系的 from/to 必须与旧人物关系一致".into());
        }
        let current = self.compile_current();
        if !current
            .analysis
            .catalog
            .legacy_relation_handles()
            .contains(handle)
        {
            return Err("旧人物关系句柄已失效，请重新读取工程".into());
        }
        let before = current.analysis.fingerprint;
        let mut candidate = self.clone();
        candidate.apply_legacy_promotion_raw(handle, draft)?;
        let compiled = candidate.compile();
        if let Some(error) = compiled
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity == crate::Severity::Error)
        {
            return Err(format!("{} {}", error.code, error.message));
        }
        let after = compiled.analysis.fingerprint;
        Ok(RelationPromotionPreview {
            handle: handle.clone(),
            content_baseline: self.content_baseline(),
            draft: draft.clone(),
            relation_id: draft.id.clone(),
            relation_type: draft.relation_type.clone(),
            description: draft.description.clone(),
            source_note: draft.source_note.clone(),
            before_fingerprint: before,
            after_fingerprint: after,
            fingerprint_changed: before != after,
        })
    }

    pub fn preview_legacy_relation_promotion(
        &self,
        handle: &LegacyRelationHandle,
        draft: &LegacyRelationPromotionDraft,
    ) -> Result<RelationPromotionPreview, String> {
        let relation = RelationDraft {
            id: draft.relation_id.clone(),
            relation_type: draft.relation_type.clone(),
            from: handle.source.clone(),
            to: handle.target.clone(),
            description: if draft.description.is_empty() {
                handle.label.clone()
            } else {
                draft.description.clone()
            },
            source_note: draft.source_note.clone(),
            ..Default::default()
        };
        self.preview_promote_legacy_relation(handle, &relation)
    }

    /// 按预览提交提升；源文件已变更或句柄已失效时零写入失败。
    pub fn apply_legacy_relation_promotion(
        &mut self,
        preview: &RelationPromotionPreview,
    ) -> Result<(), String> {
        let current = self.compile_current();
        if self.content_baseline() != preview.content_baseline {
            return Err("关系提升的内容基线已过期，请重新预览".into());
        }
        if !current
            .analysis
            .catalog
            .legacy_relation_handles()
            .contains(&preview.handle)
        {
            return Err("旧人物关系句柄已失效，请重新读取工程".into());
        }
        let preview = preview.clone();
        if preview.draft.from != preview.handle.source || preview.draft.to != preview.handle.target
        {
            return Err("提升关系的 from/to 必须与旧人物关系一致".into());
        }
        let verified = self.preview_promote_legacy_relation(&preview.handle, &preview.draft)?;
        if verified != preview {
            return Err("关系提升预览与当前草稿或指纹差异不一致，请重新预览".into());
        }
        self.edit(move |candidate| {
            candidate.apply_legacy_promotion_raw(&preview.handle, &preview.draft())
        })
    }

    pub fn promote_legacy_relation(
        &mut self,
        handle: &LegacyRelationHandle,
        draft: &RelationDraft,
    ) -> Result<RelationPromotionPreview, String> {
        let preview = self.preview_promote_legacy_relation(handle, draft)?;
        self.apply_legacy_relation_promotion(&preview)?;
        Ok(preview)
    }

    fn apply_legacy_promotion_raw(
        &mut self,
        handle: &LegacyRelationHandle,
        draft: &RelationDraft,
    ) -> Result<(), String> {
        self.ensure_relation_capability()?;
        let path = PathBuf::from(&handle.file);
        let text = self.document(&path)?.to_string();
        let parsed = crate::lexer::lex_source_with_options(
            &path.to_string_lossy(),
            &text,
            &mut Vec::new(),
            self.compile_options(),
        );
        let _line = parsed
            .iter()
            .find(|line| {
                line.no == handle.line
                    && matches!(
                        &line.kind,
                        crate::lexer::LineKind::Relation { target, label, .. }
                            if target == &handle.target.id && label == &handle.label
                    )
            })
            .ok_or("旧人物关系源行不存在")?;
        let mut text = text;
        let start = line_start(&text, handle.line);
        let end = line_end(&text, handle.line);
        let raw = text[start..end].to_string();
        let retained = crate::authoring::comments(&raw);
        text.replace_range(start..end, &retained);
        self.set_text(&path, text)?;
        append_relation_source(self, &path, &relation_source(draft, &path)?)?;
        Ok(())
    }
}

fn validate_relation_kind(kind: &str) -> Result<(), String> {
    if crate::catalog::TARGET_KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(format!("关系端点类型 `{kind}` 无效"))
    }
}

fn validate_relation_target(
    target: &TargetRef,
    options: crate::CompileOptions,
) -> Result<(), String> {
    if !crate::catalog::is_target_kind(&target.kind, options) {
        return Err(format!("关系端点类型 `{}` 无效", target.kind));
    }
    if target.kind == "file" {
        if target.id.is_empty() {
            return Err("关系 file TargetRef 路径不能为空".into());
        }
        return Ok(());
    }
    for part in target.id.split('.') {
        crate::authoring::identifier(part)?;
    }
    if target.id.is_empty() {
        return Err("关系端点 ID 不能为空".into());
    }
    Ok(())
}

fn validate_relation_file_target(root: &Path, target: &TargetRef) -> Result<(), String> {
    if target.kind != "file" {
        return Ok(());
    }
    let path = Path::new(&target.id);
    let canonical = crate::compiler::source_path(path);
    if !path.is_absolute() || canonical != path {
        return Err("关系 file TargetRef 必须使用工作区内源码的 canonical 绝对路径".into());
    }
    if !canonical.starts_with(crate::compiler::source_path(root)) {
        return Err("关系 file TargetRef 必须指向当前工作区内的源码".into());
    }
    Ok(())
}

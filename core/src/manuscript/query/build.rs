use super::*;
use crate::manuscript::{build_manuscript_index, ManuscriptReferenceStatus, WritingBuffer};
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry};
use serde_json::{json, Value};

impl Project {
    /// 建立不可变查询快照。草稿只覆盖候选缓冲，不应用、不保存、不预检磁盘基线。
    pub fn manuscript_query_snapshot(
        &self,
        buffers: &[WritingBuffer],
        drafts: &[ManuscriptQueryDraft],
    ) -> Result<ManuscriptQuerySnapshot, ManuscriptQueryError> {
        let baseline = self.content_baseline();
        let key = self.manuscript_query_key(buffers, drafts);
        let mut candidate = self.clone();
        let mut writing_paths = BTreeMap::new();
        for buffer in buffers.iter().filter(|buffer| buffer.is_changed()) {
            if buffer.baseline() != baseline {
                return Err(ManuscriptQueryError::new(
                    "STALE_DRAFT",
                    "正文草稿基线已过期，请保留输入并核对工程变化",
                ));
            }
            if let Some(previous) = writing_paths.insert(buffer.path(), buffer.source()) {
                if previous != buffer.source() {
                    return Err(ManuscriptQueryError::new(
                        "CONFLICTING_DRAFT",
                        "同一正文文件存在不同草稿，不能确定查询来源",
                    ));
                }
            }
            let document = candidate
                .documents
                .get_mut(buffer.path())
                .filter(|document| !document.is_deleted())
                .ok_or_else(|| {
                    ManuscriptQueryError::new("STALE_DRAFT", "正文草稿文件已不在当前已载入工程中")
                })?;
            let original = self
                .documents
                .get(buffer.path())
                .expect("候选与原稿共享文件身份");
            if original.text != buffer.original() {
                return Err(ManuscriptQueryError::new(
                    "STALE_DRAFT",
                    "正文草稿原文与工程不一致，请保留输入并重新核对",
                ));
            }
            document.text = buffer.source().to_owned();
        }
        // 缺失 include 不从磁盘重新载入；有编译诊断仍保留可识别编排。
        let content = candidate.compile_problems_snapshot();
        let manifest = manifest_path(&self.root);
        let registry = self
            .authoring_documents
            .get(&manifest)
            .filter(|doc| !doc.is_deleted())
            .map(|document| parse_registry(&self.root, document.bytes()))
            .unwrap_or_default();
        let features: Vec<_> = registry.required_features.iter().cloned().collect();
        let mut indices: BTreeMap<_, _> = registry
            .manuscripts
            .iter()
            .map(|(id, path)| {
                let document = self
                    .authoring_documents
                    .get(path)
                    .filter(|document| !document.is_deleted());
                let index = build_manuscript_index(
                    document.map(|doc| doc.bytes()).unwrap_or_default(),
                    &path.to_string_lossy(),
                    id,
                    &features,
                    registry.read_only(path) || document.is_some_and(|doc| doc.is_read_only()),
                    &content,
                );
                (id.clone(), index)
            })
            .collect();
        // 无效当前源码不能借用部分旧/恢复AST统计冒充已解析的当前正文。
        if content.has_errors() {
            for index in indices.values_mut() {
                mark_unresolved(index);
            }
        }
        let applied_indices = indices.clone();
        let mut draft_ids = BTreeSet::new();
        for input in drafts {
            if input.expected_baseline != baseline {
                return Err(ManuscriptQueryError::new(
                    "STALE_DRAFT",
                    "书稿编排草稿基线已过期，请保留输入并重新核对",
                ));
            }
            if !draft_ids.insert(input.draft.id.clone()) {
                return Err(ManuscriptQueryError::new(
                    "DUPLICATE_DRAFT",
                    "同一书稿收到多个编排草稿",
                ));
            }
            let current = indices.get(&input.draft.id).ok_or_else(|| {
                ManuscriptQueryError::new(
                    "MANUSCRIPT_NOT_FOUND",
                    "编排草稿对应书稿未在当前清单中注册",
                )
            })?;
            if current.read_only {
                return Err(ManuscriptQueryError::new(
                    "READ_ONLY_MANUSCRIPT",
                    "只读或未知版本书稿不能安全投影编排草稿",
                ));
            }
            if current.diagnostics.iter().any(|diagnostic| {
                matches!(
                    diagnostic.code,
                    "MAN001" | "MAN005" | "MAN006" | "MAN007" | "MAN008"
                )
            }) {
                return Err(ManuscriptQueryError::new(
                    "INVALID_DRAFT",
                    "书稿原稿仍有结构诊断，不能用已识别项草稿掩盖或丢弃原始坏项",
                ));
            }
            let bytes = merge_draft(current, &input.draft)?;
            let mut index = build_manuscript_index(
                &bytes,
                &current.file,
                &input.draft.id,
                &features,
                false,
                &content,
            );
            if content.has_errors() {
                mark_unresolved(&mut index);
            }
            indices.insert(input.draft.id.clone(), index);
        }
        let displays = content
            .analysis
            .catalog
            .objects
            .iter()
            .map(|object| (object.target.clone(), object.display.clone()))
            .collect();
        let books = indices
            .iter()
            .map(|(id, index)| (id.clone(), hierarchy::build_book(index, &displays)))
            .collect();
        let mut diagnostics = self.authoring_diagnostics().to_vec();
        for diagnostic in registry.diagnostics.into_iter().chain(content.diagnostics) {
            if !diagnostics.iter().any(|old| {
                old.code == diagnostic.code
                    && old.file == diagnostic.file
                    && old.message == diagnostic.message
            }) {
                diagnostics.push(diagnostic);
            }
        }
        if let Some(error) = self.manuscript_observation_error() {
            diagnostics.push(Diagnostic::error(
                "MAN004",
                &self.entry.to_string_lossy(),
                crate::Span::new(1, 1, 1),
                format!("外部依赖观测未完成，当前快照不能确认完整：{error}"),
            ));
        }
        if !self.recovery_conflicts().is_empty() {
            diagnostics.push(Diagnostic::error(
                "MAN004",
                &self.entry.to_string_lossy(),
                crate::Span::new(1, 1, 1),
                "工程仍有保存恢复冲突，查询只表示当前已载入缓冲",
            ));
        }
        let complete = diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != crate::Severity::Error);
        // 缓存失效key只读Project观测；游标还绑定本次真正生成的诊断、来源和附件状态。
        let mut snapshot_key = super::key::Key::new("worldline-manuscript-snapshot-v2");
        snapshot_key.bytes(key.as_bytes());
        snapshot_key.bytes(&serde_json::to_vec(&diagnostics).expect("诊断可序列化"));
        snapshot_key
            .bytes(&serde_json::to_vec(&content.analysis.catalog.assets).expect("附件可序列化"));
        for (id, index) in &indices {
            snapshot_key.bytes(id.as_bytes());
            snapshot_key.bytes(
                &serde_json::to_vec(&(&index.entries, &index.diagnostics, index.read_only))
                    .expect("章节投影可序列化"),
            );
        }
        Ok(ManuscriptQuerySnapshot {
            key: snapshot_key.finish(),
            indices,
            applied_indices,
            books,
            draft_ids,
            writing_draft: !writing_paths.is_empty(),
            diagnostics,
            complete,
        })
    }
}

fn merge_draft(
    index: &ManuscriptIndex,
    draft: &ManuscriptDraft,
) -> Result<Vec<u8>, ManuscriptQueryError> {
    let error = |message: String| ManuscriptQueryError::new("INVALID_DRAFT", message);
    let mut value = index
        .source_document()
        .cloned()
        .ok_or_else(|| error("书稿原稿无法安全读取".into()))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| error("书稿原稿必须是JSON对象".into()))?;
    let previous = object
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| error("书稿原稿entries无法安全读取".into()))?;
    let entries = crate::manuscript::commands::merge_draft_entries(previous, &draft.entries)
        .map_err(error)?;
    object.insert("id".into(), json!(draft.id));
    object.insert("title".into(), json!(draft.title));
    object.insert("entries".into(), Value::Array(entries));
    serde_json::to_vec(&value).map_err(|failure| error(format!("书稿草稿不能序列化：{failure}")))
}
fn mark_unresolved(index: &mut ManuscriptIndex) {
    let mut changed = false;
    for entry in &mut index.entries {
        if let Some(source) = entry
            .source
            .as_mut()
            .filter(|source| source.status != ManuscriptReferenceStatus::Invalid)
        {
            source.status = ManuscriptReferenceStatus::Unresolved;
            source.location = None;
            source.stats = None;
            changed = true;
        }
        if entry
            .perspective_status
            .is_some_and(|status| status != ManuscriptReferenceStatus::Invalid)
        {
            entry.perspective_status = Some(ManuscriptReferenceStatus::Unresolved);
        }
    }
    if changed {
        index.error("MAN004", "当前正文含编译错误，来源定位与统计暂不可确认");
    }
}

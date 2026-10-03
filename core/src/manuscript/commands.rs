use super::index::valid_id;
use super::*;
use crate::project::{AuthoringDocument, Project};
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json};
use crate::CompileResult;
use serde_json::json;
use std::collections::{BTreeMap, HashSet};

impl Project {
    /// 返回当前缓冲中所有清单注册书稿的只读索引；不会刷新工程或编译写入缓冲。
    pub fn manuscript_indices(&self) -> BTreeMap<String, ManuscriptIndex> {
        self.manuscript_indices_with_content(&self.compile_current())
    }

    /// 复用同一活动内容快照，不重新编译。
    pub fn manuscript_indices_with_content(
        &self,
        content: &CompileResult,
    ) -> BTreeMap<String, ManuscriptIndex> {
        let manifest = manifest_path(&self.root);
        let Some(manifest_document) = self
            .authoring_documents
            .get(&manifest)
            .filter(|document| !document.is_deleted())
        else {
            return BTreeMap::new();
        };
        let registry = parse_registry(&self.root, manifest_document.bytes());
        let required_features: Vec<_> = registry.required_features.iter().cloned().collect();

        registry
            .manuscripts
            .iter()
            .map(|(id, path)| {
                let document = self.authoring_documents.get(path);
                let bytes = document
                    .filter(|document| !document.is_deleted())
                    .map(AuthoringDocument::bytes)
                    .unwrap_or_default();
                let read_only = registry.read_only(path)
                    || document.is_some_and(AuthoringDocument::is_read_only);
                (
                    id.clone(),
                    build_manuscript_index(
                        bytes,
                        &path.to_string_lossy(),
                        id,
                        &required_features,
                        read_only,
                        content,
                    ),
                )
            })
            .collect()
    }

    pub fn manuscript_index(&self, id: &str) -> Result<ManuscriptIndex, String> {
        self.manuscript_indices()
            .remove(id)
            .ok_or_else(|| format!("书稿 `{id}` 未在工作区清单中注册"))
    }

    /// 预览章节编辑。索引构建仅读取当前缓冲，Project 与磁盘均不改变。
    pub fn preview_manuscript(
        &self,
        revision: Revision,
        command: &ManuscriptCommand,
    ) -> Result<ManuscriptIndex, String> {
        self.prepare_manuscript(revision, command)
            .map(|(_, index, _)| index)
    }

    /// 在完整内容基线、修订与磁盘保存基线匹配后一次提交书稿和清单。
    pub fn apply_manuscript(
        &mut self,
        revision: &mut Revision,
        command: ManuscriptCommand,
    ) -> Result<ManuscriptResult, String> {
        let (candidate, _index, changed_files) = self.prepare_manuscript(*revision, &command)?;
        let new_revision = revision.next_presentation();
        *self = candidate;
        *revision = new_revision;
        Ok(ManuscriptResult {
            changed_files,
            new_revision,
        })
    }

    fn prepare_manuscript(
        &self,
        revision: Revision,
        command: &ManuscriptCommand,
    ) -> Result<(Project, ManuscriptIndex, Vec<PathBuf>), String> {
        if command.expected_revision != revision {
            return Err("StaleRevision：书稿编辑修订已过期，请保留输入并重新预览".into());
        }
        if command.expected_baseline != self.content_baseline() {
            return Err("StaleRevision：书稿编辑基线已过期，请保留输入并重新预览".into());
        }
        self.ensure_workspace_writable()?;
        if !self.recovery_conflicts().is_empty() {
            return Err("工程有未解决的保存事务冲突".into());
        }
        ensure_disk_matches_saved_baselines(self)?;
        validate_draft_shape(&command.draft)?;

        let manifest = manifest_path(&self.root);
        let existing_manifest = self
            .authoring_documents
            .get(&manifest)
            .filter(|document| !document.is_deleted());
        if existing_manifest.is_some_and(AuthoringDocument::is_read_only) {
            return Err("工作区清单为只读，不能编辑书稿注册".into());
        }
        let (mut manifest_value, registry) = if let Some(document) = existing_manifest {
            let value = parse_unique_json(document.bytes())
                .map_err(|error| format!("工作区清单 JSON 无法安全读取：{error}"))?;
            let registry = parse_registry(&self.root, document.bytes());
            if !registry.diagnostics.is_empty() {
                return Err("工作区清单诊断未修复，不能编辑书稿".into());
            }
            (value, registry)
        } else {
            if command.original.is_some() {
                return Err("待编辑书稿未注册".into());
            }
            let entry = self
                .entry
                .strip_prefix(&self.root)
                .map_err(|_| "工程入口不在工作区内")?
                .to_string_lossy()
                .replace('\\', "/");
            (
                json!({
                    "schema_version": 1,
                    "language_version": self.language_version(),
                    "entry": entry,
                    "required_features": [],
                    "manuscripts": {}
                }),
                crate::workspace_documents::Registry::default(),
            )
        };
        let original_index = match command.original.as_deref() {
            Some(original) => {
                if original != command.draft.id {
                    return Err("书稿 ID 是稳定身份，不能在编辑中改名".into());
                }
                let path = registry
                    .manuscripts
                    .get(original)
                    .ok_or("待编辑书稿未注册")?;
                let index = self.manuscript_index(original)?;
                if index.read_only {
                    return Err("书稿为只读，不能写入".into());
                }
                if index.diagnostics.iter().any(|diagnostic| {
                    matches!(
                        diagnostic.code,
                        "MAN001" | "MAN005" | "MAN006" | "MAN007" | "MAN008"
                    )
                }) {
                    return Err("书稿结构诊断未修复，不能安全更新文档".into());
                }
                Some((path.clone(), index))
            }
            None => {
                if registry.manuscripts.contains_key(&command.draft.id) {
                    return Err("书稿 ID 已注册，不能覆盖".into());
                }
                None
            }
        };

        let path = if let Some((path, _)) = &original_index {
            path.clone()
        } else {
            self.root
                .join(".world")
                .join("manuscripts")
                .join(format!("{}.json", command.draft.id))
        };
        crate::file_access::within(&self.root, &path)?;
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| "书稿路径不在工作区内")?
            .to_string_lossy()
            .replace('\\', "/");
        if original_index.is_none() {
            if registry.documents.contains_key(&path) {
                return Err("书稿目标路径已被其他展示文档注册".into());
            }
            if let Some(manuscripts) = manifest_value.get("manuscripts").and_then(Value::as_object)
            {
                if manuscripts
                    .values()
                    .any(|value| value.as_str() == Some(&relative))
                {
                    return Err("书稿目标路径已被其他书稿注册".into());
                }
            }
        }

        let mut source = match &original_index {
            Some((_, index)) => index
                .source_document()
                .cloned()
                .ok_or("原書稿 JSON 无法读取")?,
            None => json!({"schema_version": MANUSCRIPT_SCHEMA_VERSION}),
        };
        let object = source.as_object_mut().ok_or("书稿 JSON 顶层必须是对象")?;
        object.insert("schema_version".into(), json!(MANUSCRIPT_SCHEMA_VERSION));
        object.insert("id".into(), json!(command.draft.id));
        object.insert("title".into(), json!(command.draft.title));
        let previous_entries = object
            .get("entries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let entries = merge_draft_entries(previous_entries, &command.draft.entries)?;
        object.insert("entries".into(), Value::Array(entries));
        let bytes = serde_json::to_vec_pretty(&source).map_err(|error| error.to_string())?;

        let mut required_features = match manifest_value.get("required_features") {
            None => Vec::new(),
            Some(Value::Array(features)) => features.clone(),
            Some(_) => return Err("工作区清单 required_features 必须是数组".into()),
        };
        let feature_added = !required_features
            .iter()
            .any(|feature| feature.as_str() == Some(MANUSCRIPT_REQUIRED_FEATURE));
        if feature_added {
            required_features.push(json!(MANUSCRIPT_REQUIRED_FEATURE));
        }
        manifest_value["required_features"] = Value::Array(required_features);
        if manifest_value.get("manuscripts").is_none() {
            manifest_value["manuscripts"] = json!({});
        }
        let manuscripts = manifest_value
            .get_mut("manuscripts")
            .and_then(Value::as_object_mut)
            .ok_or("工作区清单 manuscripts 必须是对象")?;
        if original_index.is_none() {
            manuscripts.insert(command.draft.id.clone(), json!(relative));
        }

        let content = self.compile_current();
        let mut candidate = self.clone();
        let mut changed_files = Vec::new();
        let manifest_changed =
            existing_manifest.is_none() || original_index.is_none() || feature_added;
        if manifest_changed {
            let manifest_bytes =
                serde_json::to_vec_pretty(&manifest_value).map_err(|error| error.to_string())?;
            if existing_manifest.is_some() {
                candidate.set_authoring_document(&manifest, manifest_bytes)?;
            } else {
                candidate.create_authoring_document(&manifest, manifest_bytes)?;
            }
            changed_files.push(manifest.clone());
        }
        if original_index.is_some() {
            candidate.set_authoring_document(&path, bytes)?;
        } else {
            candidate.create_authoring_document(&path, bytes)?;
        }
        changed_files.push(path);
        changed_files.sort();
        changed_files.dedup();

        let index = candidate.manuscript_index(&command.draft.id)?;
        validate_manuscript_candidate(&index, original_index.as_ref().map(|(_, index)| index))?;
        // 使用同一内容快照验证引用；书稿编辑本身不重新解释或改写事件语义。
        if content.sources != candidate.sources()
            || content.options.language_version != candidate.language_version_kind()
        {
            return Err("StaleContent：书稿编辑期间内容快照已变化".into());
        }
        Ok((candidate, index, changed_files))
    }
}

fn validate_draft_shape(draft: &ManuscriptDraft) -> Result<(), String> {
    if !valid_id(&draft.id) || draft.title.trim().is_empty() {
        return Err("书稿 ID 或标题无效".into());
    }
    let mut ids = HashSet::new();
    for entry in &draft.entries {
        if !valid_id(&entry.id) || !ids.insert(entry.id.as_str()) || entry.title.trim().is_empty() {
            return Err("书稿节点 ID 重复、格式无效或标题为空".into());
        }
        match entry.kind {
            ManuscriptEntryKind::Section if entry.target_ref.is_some() || entry.pov.is_some() => {
                return Err(format!("section `{}` 不能引用正文或声明 POV", entry.id));
            }
            ManuscriptEntryKind::Chapter if entry.target_ref.is_none() => {
                return Err(format!("chapter `{}` 必须引用正文目标", entry.id));
            }
            _ => {}
        }
    }
    Ok(())
}

fn merge_draft_entries(
    previous: Vec<Value>,
    drafts: &[ManuscriptEntryDraft],
) -> Result<Vec<Value>, String> {
    let mut previous_by_id = BTreeMap::new();
    for entry in previous {
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .ok_or("原书稿含有无法识别的节点 ID")?
            .to_string();
        if previous_by_id.insert(id.clone(), entry).is_some() {
            return Err(format!("原书稿节点 ID `{id}` 重复，不能安全更新"));
        }
    }
    let mut entries = Vec::with_capacity(drafts.len());
    for draft in drafts {
        let mut value = previous_by_id
            .remove(&draft.id)
            .unwrap_or_else(|| json!({}));
        let object = value.as_object_mut().ok_or("原书稿节点必须是对象")?;
        object.insert("id".into(), json!(draft.id));
        object.insert(
            "kind".into(),
            json!(match draft.kind {
                ManuscriptEntryKind::Section => "section",
                ManuscriptEntryKind::Chapter => "chapter",
            }),
        );
        object.insert("title".into(), json!(draft.title));
        set_optional_field(object, "parent_id", draft.parent_id.as_deref());
        set_optional_field(object, "summary", draft.summary.as_deref());
        set_optional_field(object, "status", draft.status.as_deref());
        set_optional_field(object, "goal", draft.goal.as_deref());
        set_optional_target(object, "pov", draft.pov.as_ref());
        set_optional_target(object, "target_ref", draft.target_ref.as_ref());
        entries.push(value);
    }
    Ok(entries)
}

fn set_optional_field(object: &mut serde_json::Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        object.insert(key.into(), json!(value));
    } else {
        object.remove(key);
    }
}

fn set_optional_target(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<&TargetRef>,
) {
    if let Some(value) = value {
        if let Some(Value::Object(existing)) = object.get_mut(key) {
            existing.insert("kind".into(), json!(value.kind));
            existing.insert("id".into(), json!(value.id));
        } else {
            object.insert(key.into(), json!(value));
        }
    } else {
        object.remove(key);
    }
}

fn validate_manuscript_candidate(
    candidate: &ManuscriptIndex,
    previous: Option<&ManuscriptIndex>,
) -> Result<(), String> {
    if candidate.read_only {
        return Err("书稿能力或格式不受支持，只能只读查看".into());
    }
    if let Some(diagnostic) = candidate.diagnostics.iter().find(|diagnostic| {
        matches!(
            diagnostic.code,
            "MAN001" | "MAN002" | "MAN005" | "MAN006" | "MAN007"
        )
    }) {
        return Err(format!("{}：{}", diagnostic.code, diagnostic.message));
    }
    let old_references = previous
        .map(|index| index.references_to_targets_with_status())
        .unwrap_or_default();
    for (reference, status) in candidate.references_to_targets_with_status() {
        if status != ManuscriptReferenceStatus::Resolved
            && !old_references
                .iter()
                .any(|(old, old_status)| old == &reference && old_status == &status)
        {
            return Err(format!(
                "UnresolvedReference：不能新增未解析的书稿引用 {}:{}",
                reference.target.kind, reference.target.id
            ));
        }
    }
    Ok(())
}

pub(super) fn ensure_disk_matches_saved_baselines(project: &Project) -> Result<(), String> {
    for path in project
        .documents
        .keys()
        .chain(project.authoring_documents.keys())
    {
        crate::file_access::within(&project.root, path)?;
        let state = project.tracked_file_state(path).ok_or("文档基线不存在")?;
        let disk = match crate::file_access::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("无法检查文档基线：{error}")),
        };
        if disk != state.baseline {
            return Err(format!(
                "文档已被外部修改，请保留草稿并刷新：{}",
                path.display()
            ));
        }
    }
    Ok(())
}

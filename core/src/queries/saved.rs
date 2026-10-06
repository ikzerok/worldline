use super::{
    CatalogQuery, CATALOG_QUERY_REFERENCE_REQUIRED_FEATURE, CATALOG_QUERY_SORT_REQUIRED_FEATURE,
    SAVED_QUERY_REQUIRED_FEATURE, SAVED_QUERY_SCHEMA_VERSION,
};
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json};
use crate::Diagnostic;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedQueryDraft {
    pub id: String,
    pub name: String,
    pub query: CatalogQuery,
}

#[derive(Debug, Clone)]
pub struct SavedQueryDocument {
    pub draft: SavedQueryDraft,
    pub path: PathBuf,
    /// 原始对象用于更新已知字段时保留扩展字段。
    pub source: Value,
    pub read_only: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SavedQueryIndex {
    pub queries: BTreeMap<String, SavedQueryDocument>,
    pub diagnostics: Vec<Diagnostic>,
}
impl Project {
    /// 读取清单明确注册的共享查询；无效文档仍保留在 Project 的原始字节缓冲中。
    pub fn saved_query_index(&self) -> SavedQueryIndex {
        let mut index = SavedQueryIndex::default();
        let manifest = manifest_path(&self.root);
        let Ok(document) = self.authoring_document(&manifest) else {
            return index;
        };
        if document.is_deleted() {
            return index;
        }
        let registry = parse_registry(&self.root, document.bytes());
        index.diagnostics.extend(registry.diagnostics);
        for (registered_id, path) in registry.saved_queries {
            let parsed = (|| -> Result<(SavedQueryDraft, Value, bool), String> {
                let document = self.authoring_document(&path)?;
                if document.is_deleted() {
                    return Err("注册的已保存查询文档已删除".into());
                }
                let source = parse_unique_json(document.bytes())
                    .map_err(|error| format!("查询 JSON 无法解析:{error}"))?;
                if source.get("schema_version").and_then(Value::as_u64)
                    != Some(SAVED_QUERY_SCHEMA_VERSION)
                {
                    return Err("已保存查询 schema_version 不受支持，只读保留原文".into());
                }
                let draft: SavedQueryDraft = serde_json::from_value(source.clone())
                    .map_err(|error| format!("已保存查询结构无效:{error}"))?;
                validate_saved_query(&self.root, &draft)?;
                validate_query_capabilities(&draft, &source)?;
                if draft.id != registered_id {
                    return Err("已保存查询 ID 与清单注册 ID 不一致".into());
                }
                Ok((draft, source, document.is_read_only()))
            })();
            match parsed {
                Ok((draft, source, read_only)) => {
                    index.queries.insert(
                        registered_id,
                        SavedQueryDocument {
                            draft,
                            path,
                            source,
                            read_only,
                        },
                    );
                }
                Err(error) => index.diagnostics.push(Diagnostic::error(
                    "QRY001",
                    &path.to_string_lossy(),
                    crate::Span::new(1, 1, 1),
                    error,
                )),
            }
        }
        crate::sort_diagnostics(&mut index.diagnostics);
        index
    }

    /// 创建或更新共享查询定义；个人收藏与待办状态不写入查询文档。
    pub fn save_saved_query(
        &mut self,
        draft: SavedQueryDraft,
        expected_baseline: &str,
    ) -> Result<Vec<PathBuf>, String> {
        if expected_baseline != self.content_baseline() {
            return Err("StaleBaseline：已保存查询写入基线已过期".into());
        }
        self.ensure_workspace_writable()?;
        self.checkpoint_disk_baselines_match()?;
        validate_saved_query(&self.root, &draft)?;

        let mut candidate = self.clone();
        let manifest = manifest_path(&self.root);
        if !candidate.authoring_documents.contains_key(&manifest) {
            candidate.create_authoring_document(&manifest, minimal_manifest(&candidate)?)?;
        }
        let manifest_document = candidate.authoring_document(&manifest)?;
        if manifest_document.is_deleted() || manifest_document.is_read_only() {
            return Err("已保存查询需要可写的工作区清单".into());
        }
        let mut manifest_value = parse_unique_json(manifest_document.bytes())
            .map_err(|error| format!("工作区清单无法解析：{error}"))?;
        let registry = parse_registry(&self.root, manifest_document.bytes());
        let path = registry
            .saved_queries
            .get(&draft.id)
            .cloned()
            .unwrap_or_else(|| self.root.join(format!(".world/queries/{}.json", draft.id)));
        if registry
            .saved_queries
            .iter()
            .any(|(registered_id, registered_path)| {
                registered_id != &draft.id && registered_path == &path
            })
        {
            return Err("已保存查询路径已由其他 ID 注册，保留原文档".into());
        }
        let existing = candidate
            .authoring_documents
            .get(&path)
            .filter(|document| !document.deleted)
            .cloned();
        if let Some(document) = &existing {
            if document.read_only {
                return Err("已保存查询版本或必需能力未知，只能只读查看".into());
            }
            let source = parse_unique_json(&document.bytes)
                .map_err(|error| format!("已保存查询 JSON 无法解析：{error}"))?;
            if source.get("schema_version").and_then(Value::as_u64)
                != Some(SAVED_QUERY_SCHEMA_VERSION)
            {
                return Err("已保存查询 schema_version 未知，只读保留原文".into());
            }
            let existing_draft: SavedQueryDraft = serde_json::from_value(source.clone())
                .map_err(|error| format!("现有查询文档结构无效，只读保留原文：{error}"))?;
            validate_saved_query(&self.root, &existing_draft)
                .map_err(|error| format!("现有查询无效，只读保留原文：{error}"))?;
            validate_query_capabilities(&existing_draft, &source)?;
            if existing_draft.id != draft.id {
                return Err("已保存查询 ID 与请求或清单注册 ID 不一致，保留原文档".into());
            }
        }

        let mut changed = Vec::new();
        if existing.is_none() {
            let manifest_object = manifest_value
                .as_object_mut()
                .ok_or("工作区清单顶层必须是对象")?;
            let features = manifest_object
                .entry("required_features")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or("清单 required_features 必须是数组")?;
            if features.iter().any(|feature| !feature.is_string()) {
                return Err("清单 required_features 只能包含字符串".into());
            }
            if !features
                .iter()
                .any(|feature| feature.as_str() == Some(SAVED_QUERY_REQUIRED_FEATURE))
            {
                features.push(json!(SAVED_QUERY_REQUIRED_FEATURE));
            }
            let registrations = manifest_object
                .entry("saved_queries")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or("清单 saved_queries 必须是对象")?;
            if registrations.contains_key(&draft.id) {
                return Err("已保存查询 ID 已注册但路径无效，保留原注册".into());
            }
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| "已保存查询路径越出工作区")?
                .to_string_lossy()
                .replace('\\', "/");
            registrations.insert(draft.id.clone(), json!(relative));
            candidate.set_authoring_document(
                &manifest,
                serde_json::to_vec_pretty(&manifest_value).map_err(|error| error.to_string())?,
            )?;
            changed.push(manifest.clone());
            if !candidate.authoring_diagnostics().is_empty() {
                return Err("注册已保存查询后清单诊断无效，未应用更改".into());
            }
        }

        let fresh = serde_json::to_value(&draft).map_err(|error| error.to_string())?;
        let mut saved = existing
            .as_ref()
            .map(|document| parse_unique_json(&document.bytes))
            .transpose()
            .map_err(|error| format!("已保存查询 JSON 无法解析：{error}"))?
            .unwrap_or_else(|| json!({}));
        merge_json_preserving_unknown(&mut saved, &fresh);
        update_query_metadata(&mut saved, &draft)?;
        saved["schema_version"] = json!(SAVED_QUERY_SCHEMA_VERSION);
        let bytes = serde_json::to_vec_pretty(&saved).map_err(|error| error.to_string())?;
        if existing.is_some() {
            candidate.set_authoring_document(&path, bytes)?;
        } else {
            candidate.create_authoring_document(&path, bytes)?;
        }
        changed.push(path);
        self.checkpoint_disk_baselines_match()?;
        *self = candidate;
        Ok(changed)
    }
}

fn validate_query_capabilities(draft: &SavedQueryDraft, source: &Value) -> Result<(), String> {
    for (required, feature) in [
        (
            draft.query.sort.is_some(),
            CATALOG_QUERY_SORT_REQUIRED_FEATURE,
        ),
        (
            draft.query.has_reference_values(),
            CATALOG_QUERY_REFERENCE_REQUIRED_FEATURE,
        ),
    ] {
        if required
            && !source
                .get("required_features")
                .and_then(Value::as_array)
                .is_some_and(|features| {
                    features.iter().any(|value| value.as_str() == Some(feature))
                })
        {
            return Err(format!(
                "查询缺少文档级 required_features {feature}，只读保留原文"
            ));
        }
    }
    Ok(())
}

fn update_query_metadata(saved: &mut Value, draft: &SavedQueryDraft) -> Result<(), String> {
    if draft.query.sort.is_none() {
        saved
            .get_mut("query")
            .and_then(Value::as_object_mut)
            .ok_or("查询必须是对象")?
            .remove("sort");
    }
    let object = saved.as_object_mut().ok_or("查询展示文档必须是对象")?;
    if draft.query.sort.is_some()
        || draft.query.has_reference_values()
        || object.contains_key("required_features")
    {
        let features = object
            .entry("required_features")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("查询 required_features 必须是数组")?;
        features.retain(|feature| {
            !matches!(
                feature.as_str(),
                Some(
                    CATALOG_QUERY_SORT_REQUIRED_FEATURE | CATALOG_QUERY_REFERENCE_REQUIRED_FEATURE
                )
            )
        });
        if draft.query.sort.is_some() {
            features.push(json!(CATALOG_QUERY_SORT_REQUIRED_FEATURE));
        }
        if draft.query.has_reference_values() {
            features.push(json!(CATALOG_QUERY_REFERENCE_REQUIRED_FEATURE));
        }
        if features.is_empty() {
            object.remove("required_features");
        }
    }
    Ok(())
}

fn validate_saved_query(root: &Path, draft: &SavedQueryDraft) -> Result<(), String> {
    if !crate::workspace_documents::valid_id(&draft.id) {
        return Err("已保存查询 ID 无效".into());
    }
    if draft.name.trim().is_empty() || draft.name.contains(['\n', '\r']) {
        return Err("已保存查询名称不能为空或跨行".into());
    }
    draft
        .query
        .validate(root)
        .map_err(|error| error.to_string())
}

fn minimal_manifest(project: &Project) -> Result<Vec<u8>, String> {
    let entry = project
        .entry
        .strip_prefix(&project.root)
        .map_err(|_| "工程入口越出工作区")?
        .to_string_lossy()
        .replace('\\', "/");
    let raw_id = project
        .root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    let mut id = raw_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if !id
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
    {
        id.insert(0, '_');
    }
    let value = json!({
        "schema_version": 1,
        "project_id": id,
        "language_version": project.language_version(),
        "entry": entry,
        "required_features": [SAVED_QUERY_REQUIRED_FEATURE],
        "maps": {},
        "graph_views": {},
        "saved_queries": {}
    });
    serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())
}

fn merge_json_preserving_unknown(original: &mut Value, fresh: &Value) {
    match (original, fresh) {
        (Value::Object(original), Value::Object(fresh)) => {
            for (key, value) in fresh {
                match original.get_mut(key) {
                    Some(existing) => merge_json_preserving_unknown(existing, value),
                    None => {
                        original.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (Value::Array(original), Value::Array(fresh))
            if fresh
                .iter()
                .all(|value| value.get("dimension").and_then(Value::as_str).is_some()) =>
        {
            let mut merged = Vec::with_capacity(fresh.len());
            for filter in fresh {
                let prior = original
                    .iter()
                    .find(|candidate| candidate.get("dimension") == filter.get("dimension"));
                if let Some(prior) = prior {
                    let mut retained = prior.clone();
                    merge_json_preserving_unknown(&mut retained, filter);
                    merged.push(retained);
                } else {
                    merged.push(filter.clone());
                }
            }
            *original = merged;
        }
        (Value::Array(original), Value::Array(fresh)) => {
            for (index, value) in fresh.iter().enumerate() {
                if let Some(existing) = original.get_mut(index) {
                    merge_json_preserving_unknown(existing, value);
                } else {
                    original.push(value.clone());
                }
            }
            original.truncate(fresh.len());
        }
        (original, fresh) => *original = fresh.clone(),
    }
}

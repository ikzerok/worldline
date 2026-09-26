//! 工程级作者模板索引、影响预览与基线保护写入。
//!
//! 模板描述只用于编辑器表单；本模块从不把字段默认值或类型迁移写入源码实例。

use crate::ast::PropertyValue;
use crate::catalog::TargetRef;
use crate::presentation_commands::Revision;
use crate::project::{AuthoringDocument, Project};
use crate::workspace_documents::{
    manifest_path, parse_registry, parse_unique_json, registered_path,
};
use crate::{CompileResult, Diagnostic, Span};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

type PreparedTemplateMutation = (
    Project,
    Option<ProjectTemplate>,
    Option<ProjectTemplate>,
    Vec<PathBuf>,
    Vec<Diagnostic>,
);

pub const PROJECT_TEMPLATE_REQUIRED_FEATURE: &str = "content.templates.v1";
pub const OBJECT_REFS_REQUIRED_FEATURE: &str = "content.object_refs.v1";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectTemplateField {
    pub id: String,
    pub key: Option<String>,
    pub label: String,
    pub field_type: String,
    pub required: bool,
    pub choices: Vec<String>,
    pub target: Option<TargetRef>,
    pub target_entity_type: Option<String>,
    pub default: Option<Value>,
    pub fields: Vec<ProjectTemplateField>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectTemplate {
    pub id: String,
    pub title: String,
    pub applies_to: TargetRef,
    pub applies_to_entity_type: Option<String>,
    pub fields: Vec<ProjectTemplateField>,
}

#[derive(Debug, Clone)]
pub struct ProjectTemplateDocument {
    pub template: Option<ProjectTemplate>,
    pub source_document: Option<Value>,
    pub source_bytes: Vec<u8>,
    pub diagnostics: Vec<Diagnostic>,
    pub read_only: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectTemplateIndex {
    pub builtins: Vec<crate::content_templates::ContentTemplate>,
    pub projects: BTreeMap<String, ProjectTemplateDocument>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectTemplateMutation {
    Import { id: String, document: Vec<u8> },
    Replace { id: String, document: Vec<u8> },
    Delete { id: String },
}

#[derive(Debug, Clone)]
pub struct TemplateCommand {
    pub expected_revision: Revision,
    pub expected_baseline: String,
    pub mutation: ProjectTemplateMutation,
    pub check_integrity: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTemplateValueState {
    Missing,
    Empty,
    Default,
    Set,
    TypeMismatch,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectTemplateFieldImpact {
    pub field_id: String,
    pub template_state: String,
    pub key: String,
    pub state: ProjectTemplateValueState,
    pub value: Option<PropertyValue>,
    pub type_matches: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectTemplateInstanceImpact {
    pub target: TargetRef,
    pub fields: Vec<ProjectTemplateFieldImpact>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectTemplateFieldChange {
    pub field_id: String,
    pub change: String,
    pub old_key: Option<String>,
    pub new_key: Option<String>,
    pub old_type: Option<String>,
    pub new_type: Option<String>,
}

#[derive(Clone)]
pub struct ProjectTemplatePreview {
    pub mutation: ProjectTemplateMutation,
    pub expected_revision: Revision,
    pub expected_baseline: String,
    pub field_changes: Vec<ProjectTemplateFieldChange>,
    pub instances: Vec<ProjectTemplateInstanceImpact>,
    pub diagnostics: Vec<Diagnostic>,
    pub changed_files: Vec<PathBuf>,
    candidate: Project,
}

#[derive(Debug, Clone)]
pub struct ProjectTemplateResult {
    pub changed_files: Vec<PathBuf>,
    pub new_revision: Revision,
}

impl std::fmt::Debug for ProjectTemplatePreview {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProjectTemplatePreview")
            .field("mutation", &self.mutation)
            .field("expected_revision", &self.expected_revision)
            .field("expected_baseline", &self.expected_baseline)
            .field("field_changes", &self.field_changes)
            .field("instances", &self.instances)
            .field("diagnostics", &self.diagnostics)
            .field("changed_files", &self.changed_files)
            .finish_non_exhaustive()
    }
}

impl Project {
    /// 返回 builtin 与清单注册的工程模板；坏文档只产生局部诊断，不中断其他项。
    pub fn template_index(&self) -> ProjectTemplateIndex {
        let builtins = crate::content_templates::builtin_templates()
            .templates
            .clone();
        let manifest = manifest_path(&self.root);
        let Some(manifest_document) = self
            .authoring_documents
            .get(&manifest)
            .filter(|document| !document.is_deleted())
        else {
            return ProjectTemplateIndex {
                builtins,
                projects: BTreeMap::new(),
                diagnostics: Vec::new(),
            };
        };
        let registry = parse_registry(&self.root, manifest_document.bytes());
        let root_has_feature = registry
            .required_features
            .contains(PROJECT_TEMPLATE_REQUIRED_FEATURE);
        let content = self.compile_current();
        let mut projects = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for (id, path) in &registry.templates {
            let document = self.authoring_documents.get(path);
            let bytes = document
                .filter(|document| !document.is_deleted())
                .map(AuthoringDocument::bytes)
                .unwrap_or_default();
            let registered_read_only = registry.read_only(path)
                || document.is_some_and(AuthoringDocument::is_read_only)
                || !root_has_feature;
            let mut entry = parse_template_document(
                bytes,
                &path.to_string_lossy(),
                id,
                &registry.required_features,
                registered_read_only,
                &content,
            );
            if !root_has_feature {
                entry.error(
                    "TPL005",
                    &path.to_string_lossy(),
                    1,
                    format!("清单注册模板时必须声明 `{PROJECT_TEMPLATE_REQUIRED_FEATURE}`"),
                );
                entry.read_only = true;
            }
            diagnostics.extend(entry.diagnostics.iter().cloned());
            projects.insert(id.clone(), entry);
        }
        ProjectTemplateIndex {
            builtins,
            projects,
            diagnostics,
        }
    }

    /// 只生成影响预览；此方法不改变 Project、源码、文档或保存基线。
    pub fn preview_template_mutation(
        &self,
        revision: Revision,
        command: &TemplateCommand,
    ) -> Result<ProjectTemplatePreview, String> {
        if command.expected_revision != revision {
            return Err("StaleRevision：模板编辑修订已过期，请重新预览".into());
        }
        if command.expected_baseline != self.content_baseline() {
            return Err("StaleRevision：模板编辑基线已过期，请重新预览".into());
        }
        self.ensure_workspace_writable()?;
        if !self.recovery_conflicts().is_empty() {
            return Err("工程有未解决的保存事务冲突".into());
        }
        ensure_disk_matches_saved_baselines(self)?;
        let (candidate, old, new, changed_files, diagnostics) =
            prepare_template_mutation(self, &command.mutation, command.check_integrity)?;
        let mut preview = ProjectTemplatePreview {
            mutation: command.mutation.clone(),
            expected_revision: revision,
            expected_baseline: command.expected_baseline.clone(),
            field_changes: Vec::new(),
            instances: Vec::new(),
            diagnostics,
            changed_files,
            candidate,
        };
        preview.field_changes = field_changes(old.as_ref(), new.as_ref());
        let content = self.compile_current();
        preview.instances = instance_impacts(
            old.as_ref(),
            new.as_ref(),
            &content,
            command.check_integrity,
            &mut preview.diagnostics,
        );
        Ok(preview)
    }

    /// 仅提交显式预览；基线、修订或任一磁盘保存基线过期时整批拒绝。
    pub fn apply_template_mutation(
        &mut self,
        revision: &mut Revision,
        preview: ProjectTemplatePreview,
    ) -> Result<ProjectTemplateResult, String> {
        if *revision != preview.expected_revision {
            return Err("StaleRevision：模板编辑修订已过期，请重新预览".into());
        }
        if self.content_baseline() != preview.expected_baseline {
            return Err("StaleRevision：模板编辑基线已过期，请重新预览".into());
        }
        self.ensure_workspace_writable()?;
        if !self.recovery_conflicts().is_empty() {
            return Err("工程有未解决的保存事务冲突".into());
        }
        ensure_disk_matches_saved_baselines(self)?;
        let next_revision = revision.next_presentation();
        *self = preview.candidate;
        *revision = next_revision;
        Ok(ProjectTemplateResult {
            changed_files: preview.changed_files,
            new_revision: next_revision,
        })
    }
}

fn prepare_template_mutation(
    project: &Project,
    mutation: &ProjectTemplateMutation,
    check_integrity: bool,
) -> Result<PreparedTemplateMutation, String> {
    let manifest = manifest_path(&project.root);
    let existing_manifest = project
        .authoring_documents
        .get(&manifest)
        .filter(|document| !document.is_deleted());
    if existing_manifest.is_some_and(AuthoringDocument::is_read_only) {
        return Err("工作区清单为只读，不能编辑模板注册".into());
    }
    let (mut manifest_value, registry) = if let Some(document) = existing_manifest {
        let value = parse_unique_json(document.bytes())
            .map_err(|error| format!("工作区清单 JSON 无法安全读取：{error}"))?;
        let registry = parse_registry(&project.root, document.bytes());
        if !registry.diagnostics.is_empty() {
            return Err("工作区清单诊断未修复，不能编辑模板注册".into());
        }
        (value, registry)
    } else {
        let entry = project
            .entry
            .strip_prefix(&project.root)
            .map_err(|_| "工程入口不在工作区内")?
            .to_string_lossy()
            .replace('\\', "/");
        (
            json!({
                "schema_version": 1,
                "project_id": project_id(&project.root),
                "language_version": project.language_version(),
                "entry": entry,
                "required_features": [],
                "templates": {}
            }),
            crate::workspace_documents::Registry::default(),
        )
    };
    let manifest_object = manifest_value
        .as_object_mut()
        .ok_or("工作区清单顶层必须为对象")?;

    let (id, document, operation) = match mutation {
        ProjectTemplateMutation::Import { id, document } => {
            if registry.templates.contains_key(id) {
                return Err("工程模板已注册，必须使用替换操作".into());
            }
            (id.clone(), Some(document.clone()), "import")
        }
        ProjectTemplateMutation::Replace { id, document } => {
            if !crate::workspace_documents::valid_template_id(id) {
                return Err("TPL003：工程模板 ID 必须使用 project: 命名空间".into());
            }
            if !registry.templates.contains_key(id) {
                return Err("待替换工程模板未注册".into());
            }
            (id.clone(), Some(document.clone()), "replace")
        }
        ProjectTemplateMutation::Delete { id } => {
            if !crate::workspace_documents::valid_template_id(id) {
                return Err("TPL003：工程模板 ID 必须使用 project: 命名空间".into());
            }
            if !registry.templates.contains_key(id) {
                return Err("待删除工程模板未注册".into());
            }
            (id.clone(), None, "delete")
        }
    };
    if !crate::workspace_documents::valid_template_id(&id) {
        return Err("TPL003：工程模板 ID 必须使用 project: 命名空间".into());
    }
    if id.starts_with("template_")
        || crate::content_templates::builtin_templates()
            .templates
            .iter()
            .any(|template| template.id == id)
    {
        return Err("内置模板只读，不能覆盖或删除".into());
    }

    let old_path = registry.templates.get(&id).cloned();
    let old_entry = old_path.as_ref().map(|path| {
        let document = project.authoring_documents.get(path);
        parse_template_document(
            document
                .filter(|document| !document.is_deleted())
                .map(AuthoringDocument::bytes)
                .unwrap_or_default(),
            &path.to_string_lossy(),
            &id,
            &registry.required_features,
            registry.read_only(path) || document.is_some_and(AuthoringDocument::is_read_only),
            &project.compile_current(),
        )
    });
    if old_entry.as_ref().is_some_and(|entry| entry.read_only) {
        return Err("模板格式或必需能力未知，只能只读查看".into());
    }
    let old_template = old_entry.as_ref().and_then(|entry| entry.template.clone());

    let mut bytes = document;
    if let (Some(old_path), Some(new_bytes)) = (old_path.as_ref(), bytes.as_mut()) {
        let old = project
            .authoring_documents
            .get(old_path)
            .ok_or("模板文档未载入")?;
        if old.is_read_only() {
            return Err("模板格式或必需能力未知，只能只读查看".into());
        }
        let old_value = parse_unique_json(old.bytes())
            .map_err(|error| format!("TPL001：模板 JSON 无法安全读取：{error}"))?;
        let mut new_value = parse_unique_json(new_bytes)
            .map_err(|error| format!("TPL001：模板 JSON 无法安全读取：{error}"))?;
        preserve_unknown_fields(&old_value, &mut new_value, "root");
        *new_bytes = serde_json::to_vec_pretty(&new_value).map_err(|e| e.to_string())?;
    }

    let mut new_template = None;
    let mut diagnostics = Vec::new();
    let path = if let Some(bytes) = bytes.as_ref() {
        let path = match old_path.as_ref() {
            Some(path) => path.clone(),
            None => {
                let slug = id.strip_prefix("project:").ok_or("模板 ID 无效")?;
                registered_path(&project.root, &format!(".world/templates/{slug}.json"))?
            }
        };
        if old_path.is_none() && registry.documents.contains_key(&path) {
            return Err("模板注册路径与其他展示文档冲突".into());
        }
        let mut parse_features = registry.required_features.clone();
        if operation == "import" {
            parse_features.insert(PROJECT_TEMPLATE_REQUIRED_FEATURE.to_owned());
        }
        let registered_read_only = old_path.is_some()
            && !registry
                .required_features
                .contains(PROJECT_TEMPLATE_REQUIRED_FEATURE);
        let parsed = parse_template_document(
            bytes,
            &path.to_string_lossy(),
            &id,
            &parse_features,
            registered_read_only,
            &project.compile_current(),
        );
        diagnostics.extend(parsed.diagnostics.iter().cloned());
        if parsed.read_only {
            return Err(parsed
                .diagnostics
                .first()
                .map(|diagnostic| format!("{}：{}", diagnostic.code, diagnostic.message))
                .unwrap_or_else(|| "模板文档只读".into()));
        }
        if let Some(diagnostic) = parsed
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity == crate::Severity::Error)
        {
            return Err(format!("{}：{}", diagnostic.code, diagnostic.message));
        }
        if parsed.template.is_none() {
            let diagnostic = parsed.diagnostics.first();
            return Err(diagnostic
                .map(|d| format!("{}：{}", d.code, d.message))
                .unwrap_or_else(|| "TPL004：模板字段无效".into()));
        }
        new_template = parsed.template;
        (path, Some(bytes.clone()))
    } else {
        let path = old_path.clone().ok_or("待删除工程模板未注册")?;
        (path, None)
    };
    if let Some(template) = &new_template {
        if template.id != id {
            return Err("TPL003：模板文档 id 与注册 ID 不一致".into());
        }
    }

    {
        let required_features = manifest_object
            .entry("required_features")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("工作区清单 required_features 必须为数组")?;
        if !required_features
            .iter()
            .any(|feature| feature.as_str() == Some(PROJECT_TEMPLATE_REQUIRED_FEATURE))
        {
            required_features.push(json!(PROJECT_TEMPLATE_REQUIRED_FEATURE));
        }
    }
    let templates = manifest_object
        .entry("templates")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("工作区清单 templates 必须为对象")?;
    match operation {
        "import" => {
            let relative = path
                .0
                .strip_prefix(&project.root)
                .map_err(|_| "模板路径越界")?;
            templates.insert(
                id.clone(),
                Value::String(relative.to_string_lossy().replace('\\', "/")),
            );
        }
        "replace" => {}
        "delete" => {
            templates.remove(&id);
        }
        _ => unreachable!(),
    }

    let manifest_bytes = serde_json::to_vec_pretty(&manifest_value).map_err(|e| e.to_string())?;
    let mut candidate = project.clone();
    if candidate.authoring_document(&manifest).is_ok() {
        candidate.set_authoring_document(&manifest, manifest_bytes)?;
    } else {
        candidate.create_authoring_document(&manifest, manifest_bytes)?;
    }
    match path.1 {
        Some(bytes) => {
            if candidate.authoring_document(&path.0).is_ok() {
                candidate.set_authoring_document(&path.0, bytes)?;
            } else {
                candidate.create_authoring_document(&path.0, bytes)?;
            }
        }
        None => candidate.delete_authoring_document(&path.0)?,
    }
    let mut changed_files = vec![manifest, path.0];
    changed_files.sort();
    changed_files.dedup();
    let _ = check_integrity;
    Ok((
        candidate,
        old_template,
        new_template,
        changed_files,
        diagnostics,
    ))
}

fn parse_template_document(
    bytes: &[u8],
    file: &str,
    registered_id: &str,
    manifest_features: &BTreeSet<String>,
    registered_read_only: bool,
    content: &CompileResult,
) -> ProjectTemplateDocument {
    let mut entry = ProjectTemplateDocument {
        template: None,
        source_document: None,
        source_bytes: bytes.to_vec(),
        diagnostics: Vec::new(),
        read_only: registered_read_only,
    };
    let value = match parse_unique_json(bytes) {
        Ok(value) => value,
        Err(error) => {
            entry.error("TPL001", file, 1, format!("模板 JSON 无法解析：{error}"));
            entry.read_only = true;
            return entry;
        }
    };
    entry.source_document = Some(value.clone());
    let Some(object) = value.as_object() else {
        entry.error("TPL001", file, 1, "模板文档顶层必须是对象");
        entry.read_only = true;
        return entry;
    };
    if object.get("schema_version").and_then(Value::as_u64) != Some(1) {
        entry.error(
            "TPL002",
            file,
            line_for(bytes, "schema_version"),
            "模板格式版本不受支持，按只读处理",
        );
        entry.read_only = true;
    }
    if let Some(features) = object.get("required_features") {
        match features.as_array() {
            Some(features) => {
                for feature in features {
                    let Some(feature) = feature.as_str() else {
                        entry.error(
                            "TPL002",
                            file,
                            line_for(bytes, "required_features"),
                            "模板必需能力必须是字符串，按只读处理",
                        );
                        entry.read_only = true;
                        continue;
                    };
                    if !supported_template_feature(feature) {
                        entry.error(
                            "TPL002",
                            file,
                            line_for(bytes, feature),
                            format!("模板包含未知必需能力 `{feature}`，按只读处理"),
                        );
                        entry.read_only = true;
                    }
                }
            }
            None => {
                entry.error(
                    "TPL002",
                    file,
                    line_for(bytes, "required_features"),
                    "required_features 必须是数组，按只读处理",
                );
                entry.read_only = true;
            }
        }
    }
    let Some(id) = object.get("id").and_then(Value::as_str) else {
        entry.error(
            "TPL003",
            file,
            line_for(bytes, "id"),
            "模板 id 必须是字符串",
        );
        return entry;
    };
    if !crate::workspace_documents::valid_template_id(id) {
        entry.error(
            "TPL003",
            file,
            line_for(bytes, id),
            "工程模板 ID 必须使用 project: 命名空间",
        );
        return entry;
    }
    if id != registered_id {
        entry.error(
            "TPL003",
            file,
            line_for(bytes, id),
            format!("模板 id `{id}` 与清单注册 ID `{registered_id}` 不一致"),
        );
        return entry;
    }
    let title = match object.get("title").and_then(Value::as_str) {
        Some(title) if !title.trim().is_empty() => title.to_owned(),
        _ => {
            entry.error(
                "TPL004",
                file,
                line_for(bytes, "title"),
                "模板 title 必须是非空字符串",
            );
            return entry;
        }
    };
    let Some(applies_to) = object.get("applies_to").and_then(Value::as_object) else {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, "applies_to"),
            "模板 applies_to 必须是对象",
        );
        return entry;
    };
    let Some(kind) = applies_to.get("kind").and_then(Value::as_str) else {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, "kind"),
            "模板适用类型必须是字符串",
        );
        return entry;
    };
    if !matches!(kind, "world" | "character" | "entity") {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, kind),
            format!("模板适用目标 `{kind}` 无效"),
        );
        return entry;
    }
    let entity_type = match applies_to.get("entity_type") {
        None => None,
        Some(Value::String(value)) if !value.trim().is_empty() => Some(value.clone()),
        Some(_) => {
            entry.error(
                "TPL004",
                file,
                line_for(bytes, "entity_type"),
                "applies_to.entity_type 必须是非空字符串",
            );
            return entry;
        }
    };
    if kind != "entity" && entity_type.is_some() {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, "entity_type"),
            "只有 entity 模板可以声明 entity_type",
        );
        return entry;
    }
    let Some(fields) = object.get("fields").and_then(Value::as_array) else {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, "fields"),
            "模板 fields 必须是数组",
        );
        return entry;
    };
    let mut ids = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut parsed_fields = Vec::new();
    for field in fields {
        match parse_field(field, bytes, file, &mut ids, &mut keys, &mut entry) {
            Some(field) => parsed_fields.push(field),
            None => continue,
        }
    }
    if parsed_fields.len() != fields.len() {
        return entry;
    }
    let has_object_ref = parsed_fields.iter().any(field_contains_object_ref);
    if has_object_ref && !manifest_features.contains(OBJECT_REFS_REQUIRED_FEATURE) {
        entry.error(
            "TPL005",
            file,
            line_for(bytes, "object_ref"),
            format!("对象引用字段要求清单声明 `{OBJECT_REFS_REQUIRED_FEATURE}`，模板按只读处理"),
        );
        entry.read_only = true;
    }
    for field in &parsed_fields {
        validate_field_targets(field, &content.analysis.catalog, bytes, file, &mut entry);
    }
    entry.template = Some(ProjectTemplate {
        id: id.to_owned(),
        title,
        applies_to: TargetRef::new(kind, ""),
        applies_to_entity_type: entity_type,
        fields: parsed_fields,
    });
    entry
}

fn parse_field(
    value: &Value,
    bytes: &[u8],
    file: &str,
    ids: &mut BTreeSet<String>,
    keys: &mut BTreeSet<String>,
    entry: &mut ProjectTemplateDocument,
) -> Option<ProjectTemplateField> {
    let Some(object) = value.as_object() else {
        entry.error("TPL004", file, 1, "模板字段必须是对象");
        return None;
    };
    let get_string = |name: &str| object.get(name).and_then(Value::as_str);
    let Some(id) = get_string("id") else {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, "id"),
            "模板字段 id 必须是字符串",
        );
        return None;
    };
    let line = line_for(bytes, id);
    if !crate::workspace_documents::valid_id(id) {
        entry.error("TPL004", file, line, format!("字段 ID `{id}` 无效"));
        return None;
    }
    if !ids.insert(id.to_owned()) {
        entry.error(
            "TPL004",
            file,
            line_for_last(bytes, id),
            format!("字段 ID `{id}` 重复"),
        );
        return None;
    }
    let label = match get_string("label") {
        Some(label) if !label.trim().is_empty() => label.to_owned(),
        _ => {
            entry.error(
                "TPL004",
                file,
                line_for(bytes, "label"),
                format!("字段 `{id}` 的 label 必须非空"),
            );
            return None;
        }
    };
    let field_type = match get_string("type") {
        Some(kind)
            if matches!(
                kind,
                "text" | "number" | "boolean" | "enum" | "object_ref" | "group"
            ) =>
        {
            kind.to_owned()
        }
        _ => {
            entry.error("TPL004", file, line, format!("字段 `{id}` 的 type 无效"));
            return None;
        }
    };
    let key = get_string("key").map(str::to_owned);
    let required = object
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let choices = match object.get("choices") {
        Some(Value::Array(values)) if values.iter().all(Value::is_string) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        Some(_) => {
            entry.error(
                "TPL004",
                file,
                line_for(bytes, "choices"),
                format!("字段 `{id}` 的 choices 必须是字符串数组"),
            );
            return None;
        }
        None => Vec::new(),
    };
    let target = object
        .get("target")
        .and_then(Value::as_object)
        .and_then(|target| Some(TargetRef::new(target.get("kind")?.as_str()?, "")));
    let target_entity_type = object
        .get("target")
        .and_then(Value::as_object)
        .and_then(|target| target.get("entity_type"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    if object
        .get("target")
        .and_then(Value::as_object)
        .is_some_and(|target| target.contains_key("entity_type"))
        && target_entity_type
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
    {
        entry.error(
            "TPL004",
            file,
            line_for(bytes, "entity_type"),
            format!("字段 `{id}` target.entity_type 必须是非空字符串"),
        );
        return None;
    }
    let default = object.get("default").cloned();
    let children = object.get("fields").and_then(Value::as_array);
    let mut parsed_children = Vec::new();
    if let Some(children) = children {
        for child in children {
            if let Some(field) = parse_field(child, bytes, file, ids, keys, entry) {
                parsed_children.push(field);
            }
        }
    }

    if field_type == "group" {
        if key.is_some()
            || object.contains_key("required")
            || object.contains_key("choices")
            || object.contains_key("target")
            || default.is_some()
        {
            entry.error(
                "TPL004",
                file,
                line,
                format!("分组 `{id}` 不能声明 key、required、choices、target 或 default"),
            );
            return None;
        }
        if parsed_children.is_empty() {
            entry.error("TPL004", file, line, format!("分组 `{id}` 必须包含字段"));
            return None;
        }
    } else {
        if children.is_some() {
            entry.error(
                "TPL004",
                file,
                line,
                format!("非分组字段 `{id}` 不能包含子字段"),
            );
            return None;
        }
        if !key
            .as_deref()
            .is_some_and(crate::workspace_documents::valid_id)
        {
            entry.error(
                "TPL004",
                file,
                line,
                format!("字段 `{id}` 必须提供有效 key"),
            );
            return None;
        }
        if !keys.insert(key.clone().unwrap()) {
            entry.error(
                "TPL004",
                file,
                line_for_last(bytes, key.as_deref().unwrap()),
                format!("实例字段 key `{}` 重复", key.as_deref().unwrap()),
            );
            return None;
        }
        if !object.get("required").is_some_and(Value::is_boolean) {
            entry.error(
                "TPL004",
                file,
                line,
                format!("字段 `{id}` 的 required 必须是布尔值"),
            );
            return None;
        }
        if field_type == "enum" {
            let mut seen = BTreeSet::new();
            if choices.is_empty()
                || choices
                    .iter()
                    .any(|item| item.trim().is_empty() || !seen.insert(item))
            {
                entry.error(
                    "TPL004",
                    file,
                    line,
                    format!("枚举字段 `{id}` 必须有非空且不重复的 choices"),
                );
                return None;
            }
        } else if object.contains_key("choices") {
            entry.error(
                "TPL004",
                file,
                line,
                format!("字段 `{id}` 只有 enum 可以声明 choices"),
            );
            return None;
        }
        if field_type == "object_ref" {
            let valid_target = target.as_ref().is_some_and(|target| {
                crate::catalog::TARGET_KINDS.contains(&target.kind.as_str())
                    && (target.kind == "entity" || target_entity_type.is_none())
            });
            if !valid_target {
                entry.error(
                    "TPL004",
                    file,
                    line,
                    format!("对象引用字段 `{id}` 必须指定有效 target"),
                );
                return None;
            }
        } else if object.contains_key("target") {
            entry.error(
                "TPL004",
                file,
                line,
                format!("字段 `{id}` 只有 object_ref 可以声明 target"),
            );
            return None;
        }
        if !valid_default(&field_type, default.as_ref(), &choices, target.as_ref()) {
            entry.error(
                "TPL004",
                file,
                line_for(bytes, "default"),
                format!("字段 `{id}` 的 default 与类型不匹配"),
            );
            return None;
        }
    }
    Some(ProjectTemplateField {
        id: id.to_owned(),
        key,
        label,
        field_type,
        required,
        choices,
        target,
        target_entity_type,
        default,
        fields: parsed_children,
    })
}

fn valid_default(
    field_type: &str,
    default: Option<&Value>,
    choices: &[String],
    target: Option<&TargetRef>,
) -> bool {
    let Some(default) = default else { return true };
    match field_type {
        "text" => default.is_string(),
        "number" => default.as_f64().is_some_and(f64::is_finite),
        "boolean" => default.is_boolean(),
        "enum" => default
            .as_str()
            .is_some_and(|value| choices.iter().any(|choice| choice == value)),
        "object_ref" => default.as_object().is_some_and(|value| {
            value.len() == 2
                && value
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| target.is_some_and(|target| kind == target.kind))
                && value
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !id.trim().is_empty())
        }),
        _ => false,
    }
}

fn validate_field_targets(
    field: &ProjectTemplateField,
    catalog: &crate::catalog::Catalog,
    bytes: &[u8],
    file: &str,
    entry: &mut ProjectTemplateDocument,
) {
    if field.field_type == "object_ref" {
        if let Some(target) = field.target.as_ref() {
            let id = field
                .default
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|object| object.get("id"))
                .and_then(Value::as_str);
            if let Some(id) = id {
                let target_ref = TargetRef::new(&target.kind, id);
                let object_exists = catalog.object(&target_ref).is_some();
                let entity_type_matches =
                    field.target_entity_type.as_ref().is_none_or(|expected| {
                        catalog
                            .entities
                            .get(id)
                            .is_some_and(|entity| &entity.entity_type == expected)
                    });
                if !object_exists || !entity_type_matches {
                    entry.error(
                        "TPL006",
                        file,
                        line_for(bytes, id),
                        format!("默认对象引用 {} `{id}` 不存在", target.kind),
                    );
                }
            }
        }
    }
    for child in &field.fields {
        validate_field_targets(child, catalog, bytes, file, entry);
    }
}

fn field_contains_object_ref(field: &ProjectTemplateField) -> bool {
    field.field_type == "object_ref" || field.fields.iter().any(field_contains_object_ref)
}

fn supported_template_feature(feature: &str) -> bool {
    matches!(
        feature,
        PROJECT_TEMPLATE_REQUIRED_FEATURE | OBJECT_REFS_REQUIRED_FEATURE
    )
}

fn field_changes(
    old: Option<&ProjectTemplate>,
    new: Option<&ProjectTemplate>,
) -> Vec<ProjectTemplateFieldChange> {
    let old_fields = old.map(flatten_fields).unwrap_or_default();
    let new_fields = new.map(flatten_fields).unwrap_or_default();
    let ids: BTreeSet<_> = old_fields
        .keys()
        .chain(new_fields.keys())
        .cloned()
        .collect();
    ids.into_iter()
        .filter_map(|id| {
            let old = old_fields.get(&id);
            let new = new_fields.get(&id);
            let change = match (old, new) {
                (None, Some(_)) => "added",
                (Some(_), None) => "removed",
                (Some(old), Some(new)) if old.key != new.key => "renamed",
                (Some(old), Some(new)) if old.field_type != new.field_type => "type_changed",
                (Some(old), Some(new)) if old.label != new.label => "label_changed",
                (Some(old), Some(new))
                    if old.required != new.required
                        || old.default != new.default
                        || old.choices != new.choices =>
                {
                    "constraints_changed"
                }
                _ => return None,
            };
            Some(ProjectTemplateFieldChange {
                field_id: id,
                change: change.into(),
                old_key: old.and_then(|field| field.key.clone()),
                new_key: new.and_then(|field| field.key.clone()),
                old_type: old.map(|field| field.field_type.clone()),
                new_type: new.map(|field| field.field_type.clone()),
            })
        })
        .collect()
}

fn flatten_fields(template: &ProjectTemplate) -> BTreeMap<String, ProjectTemplateField> {
    fn add(field: &ProjectTemplateField, fields: &mut BTreeMap<String, ProjectTemplateField>) {
        fields.insert(field.id.clone(), field.clone());
        for child in &field.fields {
            add(child, fields);
        }
    }
    let mut fields = BTreeMap::new();
    for field in &template.fields {
        add(field, &mut fields);
    }
    fields
}

fn instance_impacts(
    old: Option<&ProjectTemplate>,
    new: Option<&ProjectTemplate>,
    content: &CompileResult,
    check_integrity: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<ProjectTemplateInstanceImpact> {
    let mut templates = Vec::new();
    if let Some(old) = old {
        templates.push((old, "current"));
    }
    if let Some(new) = new {
        if old != Some(new) {
            templates.push((new, "proposed"));
        }
    }
    let mut objects = BTreeMap::<TargetRef, BTreeMap<String, PropertyValue>>::new();
    for (template, _) in &templates {
        for object in content
            .analysis
            .catalog
            .objects
            .iter()
            .filter(|object| object.target.kind == template.applies_to.kind)
        {
            let properties = match object.target.kind.as_str() {
                "entity" => content
                    .analysis
                    .catalog
                    .entities
                    .get(&object.target.id)
                    .filter(|entity| {
                        template
                            .applies_to_entity_type
                            .as_ref()
                            .is_none_or(|kind| kind == &entity.entity_type)
                    })
                    .map(|entity| &entity.properties),
                "character" => content
                    .analysis
                    .symbols
                    .characters
                    .get(&object.target.id)
                    .map(|character| &character.properties),
                "world" => content
                    .analysis
                    .world
                    .as_ref()
                    .map(|world| &world.properties),
                _ => None,
            };
            if let Some(properties) = properties {
                objects.insert(object.target.clone(), properties.clone());
            }
        }
    }
    objects
        .into_iter()
        .map(|(target, properties)| {
            let mut fields = Vec::new();
            for (template, template_state) in &templates {
                for field in flatten_fields(template).into_values() {
                    let Some(key) = field.key.as_ref() else {
                        continue;
                    };
                    let value = properties.get(key).cloned();
                    let type_matches = value.as_ref().map(|value| {
                        property_matches_type(value, &field, &content.analysis.catalog)
                    });
                    let state = match value.as_ref() {
                        None => ProjectTemplateValueState::Missing,
                        Some(PropertyValue::Str(text)) if text.trim().is_empty() => {
                            ProjectTemplateValueState::Empty
                        }
                        Some(value)
                            if field
                                .default
                                .as_ref()
                                .is_some_and(|default| property_matches_json(value, default)) =>
                        {
                            ProjectTemplateValueState::Default
                        }
                        Some(_) if type_matches == Some(true) => ProjectTemplateValueState::Set,
                        Some(_) => ProjectTemplateValueState::TypeMismatch,
                    };
                    if check_integrity && state == ProjectTemplateValueState::TypeMismatch {
                        let object = content.analysis.catalog.object(&target);
                        diagnostics.push(Diagnostic::warning(
                            "TPL006",
                            object.map_or("", |object| object.file.as_str()),
                            Span::new(object.map_or(1, |object| object.line), 1, 1),
                            format!(
                                "{} `{}` 的字段 `{key}` 与{}模板类型不匹配；预览不会转换实例值",
                                target.kind,
                                target.id,
                                if *template_state == "current" {
                                    "当前"
                                } else {
                                    "新"
                                }
                            ),
                        ));
                    }
                    fields.push(ProjectTemplateFieldImpact {
                        field_id: field.id.clone(),
                        template_state: (*template_state).into(),
                        key: key.clone(),
                        state,
                        type_matches,
                        value,
                    });
                }
            }
            ProjectTemplateInstanceImpact { target, fields }
        })
        .collect()
}

fn property_matches_type(
    value: &PropertyValue,
    field: &ProjectTemplateField,
    catalog: &crate::catalog::Catalog,
) -> bool {
    match field.field_type.as_str() {
        "text" => matches!(value, PropertyValue::Str(_)),
        "number" => matches!(value, PropertyValue::Num(number) if number.is_finite()),
        "boolean" => matches!(value, PropertyValue::Bool(_)),
        "enum" => matches!(value, PropertyValue::Str(text) if field.choices.contains(text)),
        "object_ref" => {
            matches!(value, PropertyValue::Ref(reference) if field.target.as_ref().is_some_and(|target| target.kind == reference.kind)
            && field.target_entity_type.as_ref().is_none_or(|expected| catalog.entities.get(&reference.id).is_some_and(|entity| &entity.entity_type == expected)))
        }
        _ => true,
    }
}

fn property_matches_json(value: &PropertyValue, expected: &Value) -> bool {
    match (value, expected) {
        (PropertyValue::Str(actual), Value::String(expected)) => actual == expected,
        (PropertyValue::Num(actual), Value::Number(expected)) => expected.as_f64() == Some(*actual),
        (PropertyValue::Bool(actual), Value::Bool(expected)) => actual == expected,
        (PropertyValue::Ref(actual), Value::Object(expected)) => {
            expected.get("kind").and_then(Value::as_str) == Some(actual.kind.as_str())
                && expected.get("id").and_then(Value::as_str) == Some(actual.id.as_str())
        }
        _ => false,
    }
}

fn preserve_unknown_fields(old: &Value, new: &mut Value, context: &str) {
    let (Value::Object(old), Value::Object(new)) = (old, new) else {
        return;
    };
    let known: &[&str] = match context {
        "root" => &[
            "schema_version",
            "id",
            "title",
            "applies_to",
            "fields",
            "required_features",
        ],
        "applies_to" | "target" => &["kind", "entity_type"],
        "field" => &[
            "id", "key", "label", "type", "required", "choices", "target", "fields", "default",
        ],
        _ => &[],
    };
    for (key, value) in old {
        if !known.contains(&key.as_str()) && !new.contains_key(key) {
            new.insert(key.clone(), value.clone());
        }
    }
    for key in ["applies_to", "target"] {
        if let (Some(old), Some(new)) = (old.get(key), new.get_mut(key)) {
            preserve_unknown_fields(old, new, key);
        }
    }
    if let (Some(Value::Array(old_fields)), Some(Value::Array(new_fields))) =
        (old.get("fields"), new.get_mut("fields"))
    {
        let old_by_id = old_fields
            .iter()
            .filter_map(|field| Some((field.get("id")?.as_str()?, field)))
            .collect::<HashMap<_, _>>();
        for field in new_fields {
            if let Some(id) = field.get("id").and_then(Value::as_str) {
                if let Some(old_field) = old_by_id.get(id) {
                    preserve_unknown_fields(old_field, field, "field");
                }
            }
        }
    }
}

fn ensure_disk_matches_saved_baselines(project: &Project) -> Result<(), String> {
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
                "文档已被外部修改，请刷新后重新预览：{}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn project_id(root: &Path) -> String {
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    let mut id = String::from("project_");
    id.extend(name.chars().map(|character| {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
            character
        } else {
            '_'
        }
    }));
    id
}

fn line_for(bytes: &[u8], needle: &str) -> u32 {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return 1;
    };
    text.find(needle)
        .map(|position| {
            text[..position]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count() as u32
                + 1
        })
        .unwrap_or(1)
}

fn line_for_last(bytes: &[u8], needle: &str) -> u32 {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return 1;
    };
    text.rfind(needle)
        .map(|position| {
            text[..position]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count() as u32
                + 1
        })
        .unwrap_or(1)
}

impl ProjectTemplateDocument {
    fn error(&mut self, code: &'static str, file: &str, line: u32, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(
            code,
            file,
            Span::new(line, 1, 1),
            message,
        ));
    }
}

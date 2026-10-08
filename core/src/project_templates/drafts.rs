//! 未应用的原始字节草稿；所有投影和结构编辑仅消费现有内容快照。
use super::*;
mod context;
mod create;
mod edits;
mod reservations;
use context::DraftContext;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectTemplateDraftSource {
    New,
    Copy { id: String },
    Existing { id: String },
    Json { bytes: Vec<u8> },
}

// 内部标记的 unit variant 会忽略未知成员；空 struct wire variant 才严格拒绝。
impl<'de> Deserialize<'de> for ProjectTemplateDraftSource {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum WireSource {
            New {},
            Copy { id: String },
            Existing { id: String },
            Json { bytes: Vec<u8> },
        }
        Ok(match WireSource::deserialize(deserializer)? {
            WireSource::New {} => Self::New,
            WireSource::Copy { id } => Self::Copy { id },
            WireSource::Existing { id } => Self::Existing { id },
            WireSource::Json { bytes } => Self::Json { bytes },
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTemplateDraft {
    pub source_bytes: Vec<u8>,
    /// 既有文档的替换身份；手工修改 JSON id 不会改变此绑定。
    pub existing_id: Option<String>,
    /// 草稿会话中已使用的稳定身份；不写入模板 JSON。
    #[serde(default)]
    pub reserved_field_ids: Vec<String>,
    /// 自动新增不得重新绑定会话中已移除的实例 key。
    #[serde(default)]
    pub reserved_keys: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTemplateFieldType {
    Text,
    Number,
    Boolean,
    Enum,
    ObjectRef,
    Group,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTemplateDraftTarget {
    pub kind: String,
    pub entity_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTemplateFieldProperties {
    pub label: String,
    pub key: Option<String>,
    pub field_type: ProjectTemplateFieldType,
    pub required: bool,
    pub choices: Vec<String>,
    pub target: Option<ProjectTemplateDraftTarget>,
    pub default: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectTemplateDraftEdit {
    SetMetadata {
        title: String,
        applies_to: ProjectTemplateDraftTarget,
    },
    AddField {
        parent_id: Option<String>,
        index: usize,
        field_type: ProjectTemplateFieldType,
    },
    UpdateField {
        field_id: String,
        properties: ProjectTemplateFieldProperties,
    },
    DeleteField {
        field_id: String,
    },
    /// index 是字段移除后，目标父级 fields 数组中的插入位置。
    MoveField {
        field_id: String,
        parent_id: Option<String>,
        index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTemplateDraftDiagnostic {
    pub severity: crate::Severity,
    pub code: String,
    pub message: String,
    pub file: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTemplateDraftProjection {
    pub draft: ProjectTemplateDraft,
    pub template: Option<ProjectTemplate>,
    pub diagnostics: Vec<ProjectTemplateDraftDiagnostic>,
    /// 未知格式/能力或继承保护；坏 JSON 可在高级视图修复，不因此设为只读。
    pub read_only: bool,
    /// 仅有效且受支持的文档允许结构编辑；与 JSON 原文是否可修复分开。
    pub editable: bool,
}

impl ProjectTemplateFieldType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Enum => "enum",
            Self::ObjectRef => "object_ref",
            Self::Group => "group",
        }
    }
}

impl Project {
    /// 新建/复制/载入草稿，不编译、不读磁盘、不改变工程。
    pub fn template_draft(
        &self,
        source: ProjectTemplateDraftSource,
        content: &CompileResult,
    ) -> Result<ProjectTemplateDraftProjection, String> {
        let draft = create::create(self, source, content)?;
        Ok(self.project_template_draft_projection(&draft, content))
    }

    /// 保留所有原始字节，复用正式模板解析器和调用方的活动内容快照。
    pub fn project_template_draft_projection(
        &self,
        draft: &ProjectTemplateDraft,
        content: &CompileResult,
    ) -> ProjectTemplateDraftProjection {
        let context = DraftContext::new(self);
        let value = parse_unique_json(&draft.source_bytes).ok();
        let document_id = value
            .as_ref()
            .and_then(|v| v.get("id"))
            .and_then(Value::as_str);
        let id = draft.existing_id.as_deref().or(document_id).unwrap_or("");
        let path = context.template_path(self, id);
        let file = path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<template-draft>".into());
        let inherited = context.read_only
            || path
                .as_ref()
                .and_then(|p| self.authoring_documents.get(p))
                .is_some_and(AuthoringDocument::is_read_only);
        let mut features = context.features.clone();
        // 与正式 Import 一致：1.10+ 的普通引用导入可显式启用 object_refs。
        if !context.registered(id) {
            features.insert(PROJECT_TEMPLATE_REQUIRED_FEATURE.into());
            if self.language_version_kind().supports_entities() {
                features.insert(OBJECT_REFS_REQUIRED_FEATURE.into());
            }
        }
        let mut parsed = super::document::fields::parse_template_document(
            &draft.source_bytes,
            &file,
            id,
            &features,
            inherited,
            self.language_version_kind(),
            content,
        );
        if parsed
            .template
            .as_ref()
            .is_some_and(|template| contains_character(&template.fields))
            && !context.features.contains(OBJECT_REFS_REQUIRED_FEATURE)
        {
            parsed.diagnostics.push(Diagnostic::error(
                "TPL005",
                &file,
                Span::new(1, 1, 1),
                "人物引用模板必须预先声明 content.object_refs.v1；不会自动开启缺失能力",
            ));
            parsed.read_only = true;
        }
        if draft.existing_id.is_some() && !context.registered(id) {
            parsed.diagnostics.push(Diagnostic::error(
                "TPL003",
                &file,
                Span::new(1, 1, 1),
                "草稿绑定的工程模板已不再注册，请重新载入",
            ));
            parsed.read_only = true;
        }
        let only_bad_json =
            !parsed.diagnostics.is_empty() && parsed.diagnostics.iter().all(|d| d.code == "TPL001");
        let read_only = inherited || (parsed.read_only && !only_bad_json);
        let editable = !read_only
            && parsed.template.is_some()
            && !parsed
                .diagnostics
                .iter()
                .any(|d| d.severity == crate::Severity::Error);
        ProjectTemplateDraftProjection {
            draft: draft.clone(),
            template: parsed.template,
            read_only,
            editable,
            diagnostics: parsed
                .diagnostics
                .into_iter()
                .map(|d| ProjectTemplateDraftDiagnostic {
                    severity: d.severity,
                    code: d.code.into(),
                    message: d.message,
                    file: d.file,
                    span: d.span,
                })
                .collect(),
        }
    }

    /// 候选全部验证成功才返回新草稿；失败时调用方输入与 Project 均保持不变。
    pub fn edit_template_draft(
        &self,
        draft: &ProjectTemplateDraft,
        edit: &ProjectTemplateDraftEdit,
        content: &CompileResult,
    ) -> Result<ProjectTemplateDraftProjection, String> {
        let projection = self.project_template_draft_projection(draft, content);
        require_editable(&projection)?;
        let mut value = parse_unique_json(&draft.source_bytes)?;
        let (mut ids, mut keys) = reservations::for_edit(self, draft, &value);
        edits::apply(&mut value, edit, &mut ids, &mut keys)?;
        edits::collect_names(&value, &mut ids, &mut keys);
        let candidate = ProjectTemplateDraft {
            source_bytes: serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
            existing_id: draft.existing_id.clone(),
            reserved_field_ids: ids.into_iter().collect(),
            reserved_keys: keys.into_iter().collect(),
        };
        let result = self.project_template_draft_projection(&candidate, content);
        require_editable(&result)?;
        Ok(result)
    }

    /// 保留 Existing 草稿的原注册身份；JSON 改 ID 必须显式选择新的导入入口。
    pub fn template_mutation_from_draft(
        &self,
        draft: &ProjectTemplateDraft,
    ) -> Result<ProjectTemplateMutation, String> {
        let id = document_id(&draft.source_bytes)?;
        if let Some(existing_id) = &draft.existing_id {
            if &id != existing_id {
                return Err(
                    "TPL003：JSON 模板 ID 已改变；请显式按新身份导入，不可替换原模板".into(),
                );
            }
            if !DraftContext::new(self).registered(existing_id) {
                return Err("待替换工程模板已不再注册".into());
            }
            return Ok(ProjectTemplateMutation::Replace {
                id,
                document: draft.source_bytes.clone(),
            });
        }
        self.template_mutation_from_bytes(&draft.source_bytes)
    }

    /// 仅选择明确的坏原文完整替换模式；完整原字节基线仍由 TemplateCommand 绑定。
    pub fn template_repair_mutation_from_draft(
        &self,
        draft: &ProjectTemplateDraft,
    ) -> Result<ProjectTemplateMutation, String> {
        if draft.existing_id.is_none() {
            return Err("修复坏原文必须绑定已注册模板".into());
        }
        match self.template_mutation_from_draft(draft)? {
            ProjectTemplateMutation::Replace { id, document } => {
                Ok(ProjectTemplateMutation::RepairInvalid { id, document })
            }
            _ => Err("修复坏原文必须绑定已注册模板".into()),
        }
    }

    /// 显式按 JSON 身份选择导入或替换；仍须经过 TemplateCommand 预览/应用。
    pub fn template_mutation_from_bytes(
        &self,
        bytes: &[u8],
    ) -> Result<ProjectTemplateMutation, String> {
        let id = document_id(bytes)?;
        if DraftContext::new(self).registered(&id) {
            Ok(ProjectTemplateMutation::Replace {
                id,
                document: bytes.to_vec(),
            })
        } else {
            Ok(ProjectTemplateMutation::Import {
                id,
                document: bytes.to_vec(),
            })
        }
    }
}

fn document_id(bytes: &[u8]) -> Result<String, String> {
    let value =
        parse_unique_json(bytes).map_err(|e| format!("TPL001：模板 JSON 无法安全读取：{e}"))?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or("TPL003：模板 id 必须是字符串")?;
    if !crate::workspace_documents::valid_template_id(id) {
        return Err("TPL003：工程模板 ID 必须使用 project: 命名空间".into());
    }
    Ok(id.into())
}

fn require_editable(projection: &ProjectTemplateDraftProjection) -> Result<(), String> {
    if projection.editable {
        return Ok(());
    }
    Err(projection
        .diagnostics
        .first()
        .map(|d| format!("{}：{}", d.code, d.message))
        .unwrap_or_else(|| "模板格式或必需能力未知，只能只读查看".into()))
}

fn contains_character(fields: &[ProjectTemplateField]) -> bool {
    fields.iter().any(|field| {
        (field.field_type == "object_ref"
            && field
                .target
                .as_ref()
                .is_some_and(|target| target.kind == "character"))
            || contains_character(&field.fields)
    })
}

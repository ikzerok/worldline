//! Explicitly selected localization exchange; see `spec/localization.md`.

use crate::ast::{Expr, Stmt, TextPart, UnOp};
use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const LOCALIZATION_REQUIRED_FEATURE: &str = "content.localization.v1";
pub const LOCALIZATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationSelection {
    pub schema_version: u32,
    pub source_locale: String,
    pub target_locale: String,
    pub string_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalizationPart {
    Text { text: String },
    Placeholder { token: String },
    Link { token: String, label: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationSource {
    pub file: String,
    pub line: u32,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationExchangeEntry {
    pub id: String,
    pub source_revision: String,
    pub source: LocalizationSource,
    pub source_parts: Vec<LocalizationPart>,
    pub translation_parts: Option<Vec<LocalizationPart>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationExchange {
    pub schema_version: u32,
    pub source_locale: String,
    pub target_locale: String,
    pub source_baseline: String,
    pub string_ids: Vec<String>,
    pub entries: Vec<LocalizationExchangeEntry>,
}

impl LocalizationExchange {
    /// Parse an exchange file while rejecting duplicate JSON keys.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, String> {
        let value = crate::workspace_documents::parse_unique_json(bytes)
            .map_err(|error| format!("本地化交换包 JSON 无效：{error}"))?;
        let exchange: Self = serde_json::from_value(value)
            .map_err(|error| format!("本地化交换包字段无效：{error}"))?;
        Ok(exchange)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationDiagnostic {
    pub code: String,
    pub id: Option<String>,
    pub source: Option<LocalizationSource>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationExportPlan {
    pub schema_version: u32,
    pub plan_digest: String,
    pub content_baseline: String,
    pub exchange: LocalizationExchange,
    pub diagnostics: Vec<LocalizationDiagnostic>,
    pub can_export: bool,
}

#[derive(Clone)]
struct SourceUnit {
    source: LocalizationSource,
    parts: Vec<LocalizationPart>,
    source_revision: String,
}

impl Project {
    /// Build a read-only export plan from only the caller's explicit ID whitelist.
    pub fn preview_localization_export(
        &self,
        selection: &LocalizationSelection,
    ) -> Result<LocalizationExportPlan, String> {
        let selection = normalize_selection(selection)?;
        if !self.compile_options().localization_ids {
            return Err(format!(
                "工程清单必须声明 required_features {LOCALIZATION_REQUIRED_FEATURE}"
            ));
        }

        let mut compiled_project = self.clone();
        let compiled = compiled_project.compile();
        if compiled.has_errors() {
            return Err("工程有编译错误，无法安全提取本地化字符串".into());
        }

        let mut all = BTreeMap::<String, Vec<SourceUnit>>::new();
        for (event_index, event) in compiled.program.events.iter().enumerate() {
            let file = compiled
                .program
                .event_files
                .get(event_index)
                .ok_or("事件缺少源码文件映射")?;
            collect_units(
                &event.body,
                Path::new(file),
                &self.root,
                &mut all,
            )?;
        }

        let source_baseline = source_baseline(self)?;
        let mut diagnostics = Vec::new();
        let mut entries = Vec::with_capacity(selection.string_ids.len());
        for id in &selection.string_ids {
            match all.get(id).map(Vec::as_slice) {
                None | Some([]) => diagnostics.push(LocalizationDiagnostic {
                    code: "UNKNOWN_ID".into(),
                    id: Some(id.clone()),
                    source: None,
                    message: format!("工程没有本地化字符串 ID `{id}`"),
                }),
                Some(units) if units.len() != 1 => diagnostics.push(LocalizationDiagnostic {
                    code: "DUPLICATE_ID".into(),
                    id: Some(id.clone()),
                    source: units.first().map(|unit| unit.source.clone()),
                    message: format!("本地化字符串 ID `{id}` 在活动源码中重复"),
                }),
                Some(units) => {
                    let unit = &units[0];
                    entries.push(LocalizationExchangeEntry {
                        id: id.clone(),
                        source_revision: unit.source_revision.clone(),
                        source: unit.source.clone(),
                        source_parts: unit.parts.clone(),
                        translation_parts: None,
                    });
                }
            }
        }
        let exchange = LocalizationExchange {
            schema_version: LOCALIZATION_SCHEMA_VERSION,
            source_locale: selection.source_locale.clone(),
            target_locale: selection.target_locale.clone(),
            source_baseline,
            string_ids: selection.string_ids.clone(),
            entries,
        };
        let content_baseline = self.content_baseline();
        let can_export = diagnostics.is_empty();
        let plan_digest = export_plan_digest(&selection, &exchange, &content_baseline, &diagnostics)?;
        Ok(LocalizationExportPlan {
            schema_version: LOCALIZATION_SCHEMA_VERSION,
            plan_digest,
            content_baseline,
            exchange,
            diagnostics,
            can_export,
        })
    }
}

impl Project {
    /// Rebuild the export plan and publish a new UTF-8 exchange file outside the workspace.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn export_localization(
        &self,
        selection: &LocalizationSelection,
        expected_plan_digest: &str,
        destination: &Path,
    ) -> Result<LocalizationExportPlan, String> {
        let plan = self.preview_localization_export(selection)?;
        if plan.plan_digest != expected_plan_digest {
            return Err("本地化导出预览已过期，请重新预览".into());
        }
        if !plan.can_export {
            return Err(plan
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.clone())
                .unwrap_or_else(|| "本地化导出未通过校验".into()));
        }
        let bytes = serde_json::to_vec(&plan.exchange)
            .map_err(|error| format!("无法序列化本地化交换包：{error}"))?;

        let destination = crate::compiler::source_path(destination);
        if destination.starts_with(crate::compiler::source_path(&self.root)) {
            return Err("本地化交换包必须写到当前工作区之外".into());
        }
        if destination.extension().and_then(|extension| extension.to_str()) != Some("json") {
            return Err("本地化交换包目标必须使用 .json 扩展名".into());
        }
        let parent = destination.parent().ok_or("本地化交换包缺少父目录")?;
        if !parent.is_dir() {
            return Err("本地化交换包目标的父目录必须已存在".into());
        }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| format!("无法创建本地化交换包：{error}"))?;
        if let Err(error) = file.write_all(&bytes) {
            drop(file);
            let _ = std::fs::remove_file(&destination);
            return Err(format!("无法写入本地化交换包：{error}"));
        }
        Ok(plan)
    }
}

fn normalize_selection(selection: &LocalizationSelection) -> Result<LocalizationSelection, String> {
    if selection.schema_version != LOCALIZATION_SCHEMA_VERSION {
        return Err("不支持的本地化选择 schema_version".into());
    }
    if !crate::workspace_documents::valid_id(&selection.source_locale)
        || !crate::workspace_documents::valid_id(&selection.target_locale)
        || selection.source_locale == selection.target_locale
    {
        return Err("source_locale 与 target_locale 必须是不同的有效 locale ID".into());
    }
    if selection.string_ids.is_empty() {
        return Err("本地化 selection 至少要包含一个 ID".into());
    }
    let mut string_ids = selection.string_ids.clone();
    if string_ids.iter().any(|id| !crate::workspace_documents::valid_id(id)) {
        return Err("selection 含无效的本地化字符串 ID".into());
    }
    string_ids.sort();
    if string_ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("selection 不得重复本地化字符串 ID".into());
    }
    Ok(LocalizationSelection {
        schema_version: selection.schema_version,
        source_locale: selection.source_locale.clone(),
        target_locale: selection.target_locale.clone(),
        string_ids,
    })
}

fn collect_units(
    statements: &[Stmt],
    file: &Path,
    root: &Path,
    out: &mut BTreeMap<String, Vec<SourceUnit>>,
) -> Result<(), String> {
    for statement in statements {
        match statement {
            Stmt::Text(text) => {
                if let Some(id) = &text.localization_id {
                    let source = source_reference(root, file, text.loc.line, "text")?;
                    out.entry(id.clone()).or_default().push(SourceUnit {
                        source,
                        parts: source_parts(&text.parts),
                        source_revision: source_revision("text", &text.parts, text.glue),
                    });
                }
            }
            Stmt::Choice(choice) => {
                if let Some(id) = &choice.localization_id {
                    let source = source_reference(root, file, choice.loc.line, "choice")?;
                    out.entry(id.clone()).or_default().push(SourceUnit {
                        source,
                        parts: source_parts(&choice.label),
                        source_revision: source_revision("choice", &choice.label, false),
                    });
                }
                collect_units(&choice.body, file, root, out)?;
            }
            Stmt::If(statement) => {
                for (_, branch) in &statement.branches {
                    collect_units(branch, file, root, out)?;
                }
            }
            Stmt::Scene(scene) => collect_units(&scene.body, file, root, out)?,
            _ => {}
        }
    }
    Ok(())
}

fn source_reference(
    root: &Path,
    file: &Path,
    line: u32,
    kind: &str,
) -> Result<LocalizationSource, String> {
    let relative = file
        .strip_prefix(root)
        .map_err(|_| format!("本地化源码不在工程内:{}", file.display()))?;
    let relative = relative
        .to_str()
        .ok_or("本地化源码路径不是 UTF-8")?
        .replace('\\', "/");
    Ok(LocalizationSource {
        file: relative,
        line,
        kind: kind.into(),
    })
}

fn source_parts(parts: &[TextPart]) -> Vec<LocalizationPart> {
    let mut placeholders = 0usize;
    let mut links = 0usize;
    parts
        .iter()
        .map(|part| match part {
            TextPart::Str(text) => LocalizationPart::Text { text: text.clone() },
            TextPart::Expr(_) => {
                let token = format!("p{placeholders}");
                placeholders += 1;
                LocalizationPart::Placeholder { token }
            }
            TextPart::Link(link) => {
                let token = format!("l{links}");
                links += 1;
                LocalizationPart::Link {
                    token,
                    label: link.label.clone(),
                }
            }
        })
        .collect()
}

fn source_revision(kind: &str, parts: &[TextPart], glue: bool) -> String {
    let mut bytes = Vec::new();
    append_field(&mut bytes, b"kind", kind.as_bytes());
    for part in parts {
        match part {
            TextPart::Str(text) => append_field(&mut bytes, b"text", text.as_bytes()),
            TextPart::Expr(expression) => {
                let mut encoded = Vec::new();
                encode_expression(expression, &mut encoded);
                append_field(&mut bytes, b"expression", &encoded);
            }
            TextPart::Link(link) => {
                append_field(&mut bytes, b"link-kind", link.target.kind.as_bytes());
                append_field(&mut bytes, b"link-id", link.target.id.as_bytes());
                append_field(&mut bytes, b"link-label", link.label.as_bytes());
            }
        }
    }
    append_field(&mut bytes, b"glue", &[u8::from(glue)]);
    digest("worldline-localization-unit-v1", &bytes)
}

fn encode_expression(expression: &Expr, bytes: &mut Vec<u8>) {
    match expression {
        Expr::Num(value) => append_field(bytes, b"number", &value.to_bits().to_le_bytes()),
        Expr::Str(value) => append_field(bytes, b"string", value.as_bytes()),
        Expr::Bool(value) => append_field(bytes, b"boolean", &[u8::from(*value)]),
        Expr::Var { name, .. } => append_field(bytes, b"variable", name.as_bytes()),
        Expr::Unary { op, expr } => {
            append_field(
                bytes,
                b"unary",
                match op {
                    UnOp::Neg => b"neg",
                    UnOp::Not => b"not",
                },
            );
            encode_expression(expr, bytes);
        }
        Expr::Binary { op, lhs, rhs } => {
            append_field(bytes, b"binary", op.symbol().as_bytes());
            encode_expression(lhs, bytes);
            encode_expression(rhs, bytes);
        }
        Expr::Call { name, args, .. } => {
            append_field(bytes, b"call", name.as_bytes());
            for arg in args {
                encode_expression(arg, bytes);
            }
        }
    }
}

fn source_baseline(project: &Project) -> Result<String, String> {
    let options = project.compile_options();
    let mut bytes = Vec::new();
    let entry = project
        .entry
        .strip_prefix(&project.root)
        .map_err(|_| "工程入口不在工作区内")?
        .to_str()
        .ok_or("工程入口路径不是 UTF-8")?
        .replace('\\', "/");
    append_field(&mut bytes, b"entry", entry.as_bytes());
    append_field(
        &mut bytes,
        b"language-version",
        options.language_version.as_str().as_bytes(),
    );
    append_field(&mut bytes, b"object-refs", &[u8::from(options.object_refs)]);
    append_field(
        &mut bytes,
        b"localization-ids",
        &[u8::from(options.localization_ids)],
    );
    for (path, source) in project.sources() {
        let relative = path
            .strip_prefix(&project.root)
            .map_err(|_| format!("源码不在工作区内:{}", path.display()))?
            .to_str()
            .ok_or("源码路径不是 UTF-8")?
            .replace('\\', "/");
        append_field(&mut bytes, b"source-path", relative.as_bytes());
        append_field(&mut bytes, b"source-bytes", source.as_bytes());
    }
    Ok(digest("worldline-localization-source-v1", &bytes))
}

fn export_plan_digest(
    selection: &LocalizationSelection,
    exchange: &LocalizationExchange,
    content_baseline: &str,
    diagnostics: &[LocalizationDiagnostic],
) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(selection, exchange, content_baseline, diagnostics))
        .map_err(|error| format!("无法序列化本地化导出计划：{error}"))?;
    Ok(digest("worldline-localization-export-plan-v1", &bytes))
}

fn append_field(bytes: &mut Vec<u8>, kind: &[u8], value: &[u8]) {
    bytes.extend_from_slice(&(kind.len() as u64).to_le_bytes());
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value);
}

fn digest(domain: &str, bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in (domain.len() as u64)
        .to_le_bytes()
        .iter()
        .chain(domain.as_bytes())
        .chain((bytes.len() as u64).to_le_bytes().iter())
        .chain(bytes)
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationImportPlan {
    pub schema_version: u32,
    pub plan_digest: String,
    pub content_baseline: String,
    pub source_baseline: String,
    pub target_locale: String,
    pub sidecar_path: String,
    pub affected_ids: Vec<String>,
    pub diagnostics: Vec<LocalizationDiagnostic>,
    pub can_apply: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationImportResult {
    pub plan: LocalizationImportPlan,
    pub changed_files: Vec<PathBuf>,
    pub baseline: String,
    pub new_baseline: String,
}

struct PreparedImport {
    plan: LocalizationImportPlan,
    sidecar_path: PathBuf,
    manifest_bytes: Option<Vec<u8>>,
    sidecar_bytes: Option<Vec<u8>>,
    create_sidecar: bool,
}

impl Project {
    /// Validate a translator-edited exchange package without changing Project buffers.
    pub fn preview_localization_import(
        &self,
        selection: &LocalizationSelection,
        exchange: &LocalizationExchange,
    ) -> Result<LocalizationImportPlan, String> {
        Ok(prepare_import(self, selection, exchange)?.plan)
    }

    /// Revalidate and persist the complete locale update before replacing this Project buffer.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn apply_localization_import(
        &mut self,
        selection: &LocalizationSelection,
        exchange: &LocalizationExchange,
        expected_plan_digest: &str,
    ) -> Result<LocalizationImportResult, String> {
        if self.is_dirty() {
            return Err("请先保存或撤销工程修改，再应用本地化译文".into());
        }
        let mut refreshed = self.clone();
        let conflicts = refreshed.refresh()?;
        if !conflicts.is_empty() {
            return Err("工程存在外部刷新冲突，不能应用本地化译文".into());
        }
        let prepared = prepare_import(&refreshed, selection, exchange)?;
        if prepared.plan.plan_digest != expected_plan_digest {
            return Err("本地化导入预览已过期，请重新预览".into());
        }
        if !prepared.plan.can_apply {
            return Err(prepared
                .plan
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.clone())
                .unwrap_or_else(|| "本地化导入未通过校验".into()));
        }

        let baseline = refreshed.content_baseline();
        let manifest_path = crate::workspace_documents::manifest_path(&refreshed.root);
        let mut candidate = refreshed;
        let manifest_changed = prepared.manifest_bytes.is_some();
        if let Some(bytes) = prepared.manifest_bytes {
            candidate.set_authoring_document(&manifest_path, bytes)?;
        }
        let sidecar_bytes = prepared
            .sidecar_bytes
            .ok_or("本地化导入没有生成 sidecar 内容")?;
        if prepared.create_sidecar {
            candidate.create_authoring_document(&prepared.sidecar_path, sidecar_bytes)?;
        } else {
            candidate.set_authoring_document(&prepared.sidecar_path, sidecar_bytes)?;
        }
        candidate.save()?;
        let new_baseline = candidate.content_baseline();
        let mut changed_files = Vec::with_capacity(2);
        if manifest_changed {
            changed_files.push(manifest_path);
        }
        changed_files.push(prepared.sidecar_path);
        changed_files.sort();
        let plan = prepared.plan;
        *self = candidate;
        Ok(LocalizationImportResult {
            plan,
            changed_files,
            baseline,
            new_baseline,
        })
    }
}

fn prepare_import(
    project: &Project,
    requested_selection: &LocalizationSelection,
    exchange: &LocalizationExchange,
) -> Result<PreparedImport, String> {
    let selection = normalize_selection(requested_selection)?;
    let content_baseline = project.content_baseline();
    let source_baseline = source_baseline(project)?;
    let (sidecar_path, sidecar_registered) =
        localization_sidecar_path(project, &selection.target_locale)?;
    let sidecar_relative = sidecar_path
        .strip_prefix(&project.root)
        .map_err(|_| "本地化 sidecar 路径越出工作区")?
        .to_str()
        .ok_or("本地化 sidecar 路径不是 UTF-8")?
        .replace('\\', "/");
    let mut diagnostics = Vec::new();
    let mut current_units = BTreeMap::<String, Vec<SourceUnit>>::new();

    if exchange.schema_version != LOCALIZATION_SCHEMA_VERSION {
        diagnostic(
            &mut diagnostics,
            "UNSUPPORTED_VERSION",
            None,
            None,
            "不支持的本地化交换格式版本",
        );
    }
    if exchange.source_locale != selection.source_locale
        || exchange.target_locale != selection.target_locale
        || exchange.string_ids != selection.string_ids
    {
        diagnostic(
            &mut diagnostics,
            "SELECTION_MISMATCH",
            None,
            None,
            "导入 selection 必须与交换包的 locale 和 ID 白名单完全一致",
        );
    }
    if !project.compile_options().localization_ids {
        diagnostic(
            &mut diagnostics,
            "FEATURE_REQUIRED",
            None,
            None,
            format!("工程清单必须声明 required_features {LOCALIZATION_REQUIRED_FEATURE}"),
        );
    }
    if !project.authoring_diagnostics().is_empty() {
        diagnostic(
            &mut diagnostics,
            "READ_ONLY",
            None,
            None,
            "工程存在工作区诊断，只能只读查看",
        );
    }
    if project.is_dirty() {
        diagnostic(
            &mut diagnostics,
            "DIRTY_PROJECT",
            None,
            None,
            "请先保存或撤销工程修改，再预览本地化导入",
        );
    }

    let mut compile_project = project.clone();
    let compiled = compile_project.compile();
    if compiled.has_errors() {
        diagnostic(
            &mut diagnostics,
            "COMPILE_ERROR",
            None,
            None,
            "工程存在编译错误，不能验证本地化导入",
        );
    } else {
        for (event_index, event) in compiled.program.events.iter().enumerate() {
            let file = compiled
                .program
                .event_files
                .get(event_index)
                .ok_or("事件缺少源码文件映射")?;
            collect_units(
                &event.body,
                Path::new(file),
                &project.root,
                &mut current_units,
            )?;
        }
    }

    if exchange.source_baseline != source_baseline {
        diagnostic(
            &mut diagnostics,
            "BASELINE_MISMATCH",
            None,
            None,
            "交换包的源码基线已过期，请重新导出",
        );
    }

    let selected: BTreeSet<_> = selection.string_ids.iter().map(String::as_str).collect();
    let mut package_entries = BTreeMap::<&str, Vec<&LocalizationExchangeEntry>>::new();
    for entry in &exchange.entries {
        package_entries.entry(&entry.id).or_default().push(entry);
        if !selected.contains(entry.id.as_str()) {
            diagnostic(
                &mut diagnostics,
                "UNKNOWN_ID",
                Some(&entry.id),
                None,
                "交换包包含未授权的本地化字符串 ID",
            );
        }
    }
    for id in &selection.string_ids {
        let source_units = current_units.get(id).map(Vec::as_slice).unwrap_or_default();
        let Some(package_matches) = package_entries.get(id.as_str()).map(Vec::as_slice) else {
            diagnostic(
                &mut diagnostics,
                "MISSING_ENTRY",
                Some(id),
                source_units.first().map(|unit| &unit.source),
                "交换包缺少 selection 中的字符串",
            );
            continue;
        };
        if package_matches.len() != 1 {
            diagnostic(
                &mut diagnostics,
                "DUPLICATE_ID",
                Some(id),
                source_units.first().map(|unit| &unit.source),
                "交换包中的本地化字符串 ID 重复",
            );
            continue;
        }
        if source_units.len() != 1 {
            diagnostic(
                &mut diagnostics,
                if source_units.is_empty() {
                    "UNKNOWN_ID"
                } else {
                    "DUPLICATE_ID"
                },
                Some(id),
                source_units.first().map(|unit| &unit.source),
                "当前活动源码中的本地化字符串 ID 缺失或重复",
            );
            continue;
        }
        let unit = &source_units[0];
        let entry = package_matches[0];
        if entry.source_revision != unit.source_revision {
            diagnostic(
                &mut diagnostics,
                "STALE_SOURCE",
                Some(id),
                Some(&unit.source),
                "源文已改变，此译文需要复核并重新导出",
            );
        }
        if entry.source != unit.source || entry.source_parts != unit.parts {
            diagnostic(
                &mut diagnostics,
                "SOURCE_MISMATCH",
                Some(id),
                Some(&unit.source),
                "交换包的源引用或受保护源片段与当前工程不匹配",
            );
        }
        match &entry.translation_parts {
            None => diagnostic(
                &mut diagnostics,
                "MISSING_TRANSLATION",
                Some(id),
                Some(&unit.source),
                "选中的字符串缺少译文",
            ),
            Some(parts) if !protected_tokens_match(&unit.parts, parts) => diagnostic(
                &mut diagnostics,
                "INVALID_TOKEN",
                Some(id),
                Some(&unit.source),
                "译文添加、丢失、重复或改写了受保护占位符/链接 token",
            ),
            Some(_) => {}
        }
    }

    let mut manifest_bytes = None;
    let mut sidecar_bytes = None;
    let mut create_sidecar = false;
    if diagnostics.is_empty() {
        match prepare_sidecar(project, &sidecar_path, sidecar_registered, exchange) {
            Ok((manifest, bytes, create)) => {
                manifest_bytes = manifest;
                sidecar_bytes = Some(bytes);
                create_sidecar = create;
            }
            Err(message) => diagnostic(
                &mut diagnostics,
                "SIDECAR_INVALID",
                None,
                None,
                message,
            ),
        }
    }
    let can_apply = diagnostics.is_empty();
    let plan_digest = import_plan_digest(
        &selection,
        exchange,
        &content_baseline,
        &source_baseline,
        &sidecar_relative,
        &diagnostics,
    )?;
    Ok(PreparedImport {
        plan: LocalizationImportPlan {
            schema_version: LOCALIZATION_SCHEMA_VERSION,
            plan_digest,
            content_baseline,
            source_baseline,
            target_locale: selection.target_locale,
            affected_ids: selection.string_ids.clone(),
            sidecar_path: sidecar_relative,
            diagnostics,
            can_apply,
        },
        sidecar_path,
        manifest_bytes,
        sidecar_bytes,
        create_sidecar,
    })
}

fn diagnostic(
    diagnostics: &mut Vec<LocalizationDiagnostic>,
    code: &str,
    id: Option<&str>,
    source: Option<&LocalizationSource>,
    message: impl Into<String>,
) {
    diagnostics.push(LocalizationDiagnostic {
        code: code.into(),
        id: id.map(str::to_string),
        source: source.cloned(),
        message: message.into(),
    });
}

fn protected_tokens_match(source: &[LocalizationPart], translation: &[LocalizationPart]) -> bool {
    fn tokens(parts: &[LocalizationPart]) -> (BTreeMap<String, usize>, BTreeMap<String, usize>) {
        let mut placeholders = BTreeMap::new();
        let mut links = BTreeMap::new();
        for part in parts {
            match part {
                LocalizationPart::Placeholder { token } => {
                    *placeholders.entry(token.clone()).or_insert(0) += 1;
                }
                LocalizationPart::Link { token, .. } => {
                    *links.entry(token.clone()).or_insert(0) += 1;
                }
                LocalizationPart::Text { .. } => {}
            }
        }
        (placeholders, links)
    }
    tokens(source) == tokens(translation)
}

fn localization_sidecar_path(project: &Project, locale: &str) -> Result<(PathBuf, bool), String> {
    let manifest_path = crate::workspace_documents::manifest_path(&project.root);
    let manifest = project
        .authoring_documents
        .get(&manifest_path)
        .filter(|document| !document.is_deleted())
        .ok_or("本地化需要已载入的工程清单")?;
    let registry = crate::workspace_documents::parse_registry(&project.root, manifest.bytes());
    if let Some(path) = registry.localizations.get(locale) {
        return Ok((path.clone(), true));
    }
    let relative = format!(".world/localization/{locale}.json");
    let path = crate::workspace_documents::registered_path(&project.root, &relative)?;
    Ok((path, false))
}

fn prepare_sidecar(
    project: &Project,
    path: &Path,
    registered: bool,
    exchange: &LocalizationExchange,
) -> Result<(Option<Vec<u8>>, Vec<u8>, bool), String> {
    let manifest_path = crate::workspace_documents::manifest_path(&project.root);
    let manifest_bytes = if registered {
        None
    } else {
        Some(register_localization_path(
            project,
            &manifest_path,
            path,
            &exchange.target_locale,
        )?)
    };

    let current = project
        .authoring_documents
        .get(path)
        .filter(|document| !document.is_deleted());
    if current.is_some_and(|document| document.is_read_only()) {
        return Err("目标 locale sidecar 是只读文档".into());
    }
    let create_sidecar = current.is_none();
    let mut sidecar = if let Some(document) = current {
        crate::workspace_documents::parse_unique_json(document.bytes())
            .map_err(|error| format!("locale sidecar JSON 无法解析：{error}"))?
    } else {
        let files = crate::file_access::workspace_files(&project.root)
            .map_err(|error| format!("无法读取工作区文件清单：{error}"))?;
        if files.iter().any(|file| file.as_path() == path) {
            return Err("目标 locale sidecar 已存在但未注册，拒绝覆盖".into());
        }
        serde_json::json!({
            "schema_version": 1,
            "required_features": [LOCALIZATION_REQUIRED_FEATURE],
            "source_locale": exchange.source_locale,
            "target_locale": exchange.target_locale,
            "entries": {}
        })
    };
    let object = sidecar
        .as_object_mut()
        .ok_or("locale sidecar 顶层必须是 JSON 对象")?;
    if object.get("schema_version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err("locale sidecar schema_version 不受支持".into());
    }
    let features = object
        .get("required_features")
        .and_then(serde_json::Value::as_array)
        .ok_or("locale sidecar 缺少 required_features 数组")?;
    if !features
        .iter()
        .any(|feature| feature.as_str() == Some(LOCALIZATION_REQUIRED_FEATURE))
    {
        return Err("locale sidecar 缺少 content.localization.v1".into());
    }
    if object.get("source_locale").and_then(serde_json::Value::as_str)
        != Some(exchange.source_locale.as_str())
        || object.get("target_locale").and_then(serde_json::Value::as_str)
            != Some(exchange.target_locale.as_str())
    {
        return Err("locale sidecar 的 source/target locale 不匹配".into());
    }
    let entries = object
        .get_mut("entries")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("locale sidecar entries 必须是对象")?;
    for entry in &exchange.entries {
        let value = entries
            .entry(entry.id.clone())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let fields = value
            .as_object_mut()
            .ok_or_else(|| format!("sidecar 条目 `{}` 不是对象", entry.id))?;
        fields.insert(
            "source_revision".into(),
            serde_json::Value::String(entry.source_revision.clone()),
        );
        fields.insert(
            "translation_parts".into(),
            serde_json::to_value(&entry.translation_parts)
                .map_err(|error| format!("无法序列化译文：{error}"))?,
        );
    }
    let bytes = serde_json::to_vec(&sidecar)
        .map_err(|error| format!("无法序列化 locale sidecar：{error}"))?;
    Ok((manifest_bytes, bytes, create_sidecar))
}

fn register_localization_path(
    project: &Project,
    manifest_path: &Path,
    sidecar_path: &Path,
    locale: &str,
) -> Result<Vec<u8>, String> {
    let document = project
        .authoring_documents
        .get(manifest_path)
        .filter(|document| !document.is_deleted())
        .ok_or("本地化需要已载入的工程清单")?;
    let mut manifest = crate::workspace_documents::parse_unique_json(document.bytes())
        .map_err(|error| format!("工程清单 JSON 无法解析：{error}"))?;
    let object = manifest
        .as_object_mut()
        .ok_or("工程清单顶层必须是 JSON 对象")?;
    let relative = sidecar_path
        .strip_prefix(&project.root)
        .map_err(|_| "locale sidecar 路径越出工作区")?
        .to_str()
        .ok_or("locale sidecar 路径不是 UTF-8")?
        .replace('\\', "/");
    let localizations = object
        .entry("localizations")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or("工程清单 localizations 必须是对象")?;
    if let Some(existing) = localizations.get(locale).and_then(serde_json::Value::as_str) {
        if existing.replace('\\', "/") != relative {
            return Err("目标 locale 已登记到其他 sidecar 路径".into());
        }
    } else {
        localizations.insert(locale.into(), serde_json::Value::String(relative));
    }
    serde_json::to_vec(&manifest).map_err(|error| format!("无法序列化工程清单：{error}"))
}

fn import_plan_digest(
    selection: &LocalizationSelection,
    exchange: &LocalizationExchange,
    content_baseline: &str,
    source_baseline: &str,
    sidecar_path: &str,
    diagnostics: &[LocalizationDiagnostic],
) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(
        selection,
        exchange,
        content_baseline,
        source_baseline,
        sidecar_path,
        diagnostics,
    ))
    .map_err(|error| format!("无法序列化本地化导入计划：{error}"))?;
    Ok(digest("worldline-localization-import-plan-v1", &bytes))
}

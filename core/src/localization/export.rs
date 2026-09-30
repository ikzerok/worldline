use super::*;
use crate::ast::{Expr, Stmt, TextPart, UnOp};
use crate::project::Project;
use std::collections::BTreeMap;
use std::path::Path;

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
            collect_units(&event.body, Path::new(file), &self.root, &mut all)?;
        }

        for fragment in &compiled.program.fragments {
            collect_units(
                &fragment.body,
                Path::new(&fragment.file),
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
        let plan_digest =
            export_plan_digest(&selection, &exchange, &content_baseline, &diagnostics)?;
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
        if destination
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("json")
        {
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

pub(super) fn normalize_selection(
    selection: &LocalizationSelection,
) -> Result<LocalizationSelection, String> {
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
    if string_ids
        .iter()
        .any(|id| !crate::workspace_documents::valid_id(id))
    {
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

pub(super) fn collect_units(
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
            Stmt::Say(say) => {
                if let Some(id) = &say.text.localization_id {
                    let source = source_reference(root, file, say.loc.line, "say")?;
                    out.entry(id.clone()).or_default().push(SourceUnit {
                        source,
                        parts: source_parts(&say.text.parts),
                        source_revision: source_revision("say", &say.text.parts, say.text.glue),
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

pub(super) fn source_baseline(project: &Project) -> Result<String, String> {
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

pub(super) fn digest(domain: &str, bytes: &[u8]) -> String {
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

use super::*;
use crate::localization::LocalizationPart;
use std::fmt::Write;

impl ProductionScriptSnapshot {
    pub fn export(
        &self,
        options: &ProductionExportOptions,
    ) -> Result<ProductionArtifact, ProductionError> {
        options.validate()?;
        if self.request.target_locale.is_some()
            && self.request.locale_policy == ProductionLocalePolicy::Strict
            && self
                .rows
                .iter()
                .any(|row| row.status != ProductionStatus::Translated)
        {
            return Err(ProductionError::new(
                "LOCALE_INCOMPLETE",
                "所选交付单元仍有缺译、过期或无效译文；严格交付未产生部分文件",
            ));
        }
        let mut rows = self.rows.clone();
        if options.include_direction {
            for row in &mut rows {
                row.direction = self.directions.get(&row.row_key).cloned();
            }
        }
        let document = ProductionDocument {
            schema_version: 1,
            snapshot_key: self.key.clone(),
            summary: self.summary.clone(),
            scope_kind: match self.request.scope {
                ProductionScope::CurrentTarget { .. } => "current_target",
                ProductionScope::Manuscript { .. } => "manuscript",
                ProductionScope::Project => "project",
            }
            .into(),
            speaker: self.request.speaker.clone(),
            target_locale: self.request.target_locale.clone(),
            locale_policy: self.request.locale_policy,
            metadata_language: "source".into(),
            direction_included: options.include_direction,
            definitions: self.definitions.clone(),
            chapter_occurrences: self.chapter_occurrences.clone(),
            call_sites: self.call_sites.clone(),
            rows,
        };
        bounded_size(&document, self.request.limits.export_bytes)?;
        let bytes = match options.format {
            ProductionFormat::Json => {
                let mut output = Limited::new(self.request.limits.export_bytes);
                serde_json::to_writer_pretty(&mut output, &document)
                    .map_err(|_| ProductionError::budget())?;
                output.bytes
            }
            ProductionFormat::Markdown => {
                markdown(&document, self.request.limits.export_bytes)?.into_bytes()
            }
            ProductionFormat::Csv => csv(&document, self.request.limits.export_bytes)?.into_bytes(),
        };
        Ok(ProductionArtifact {
            bytes,
            format: options.format,
            snapshot_key: self.key.clone(),
        })
    }
}
struct Limited {
    bytes: Vec<u8>,
    limit: usize,
}
impl Limited {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}
impl std::io::Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            return Err(std::io::Error::other("production_limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn text(parts: &[LocalizationPart]) -> String {
    parts
        .iter()
        .map(|part| match part {
            LocalizationPart::Text { text } => text.clone(),
            LocalizationPart::Link { label, .. } => label.clone(),
            LocalizationPart::Placeholder { token } => format!("〔{token} · 动态未求值〕"),
        })
        .collect()
}
fn encoded(value: &impl Serialize) -> String {
    serde_json::to_string(value).expect("生产台本字段可序列化")
}
fn append(output: &mut String, value: &str, limit: usize) -> Result<(), ProductionError> {
    if output.len().saturating_add(value.len()) > limit {
        return Err(ProductionError::budget());
    }
    output.push_str(value);
    Ok(())
}
fn escape(value: &str) -> String {
    let mut output = String::new();
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '!'
            | '|' | '.' => {
                output.push('\\');
                output.push(character);
            }
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            c if c.is_control()
                || matches!(c,'\u{2028}'|'\u{2029}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') =>
            {
                let _ = write!(output, "\\u{{{:04x}}}", c as u32);
            }
            c => output.push(c),
        }
    }
    output
}
fn field(
    output: &mut String,
    label: &str,
    value: &str,
    limit: usize,
) -> Result<(), ProductionError> {
    append(output, &format!("- {label}：{}\n", escape(value)), limit)
}
fn markdown(document: &ProductionDocument, limit: usize) -> Result<String, ProductionError> {
    let mut output = String::new();
    append(&mut output,"# 角色制作台本\n\n作者私密材料 · 静态定义 · 条件、实参与动态内容均未求值\n\n角色名称和演出备注是源语言作者元数据。来源锚不是持久行 ID；没有稳定 ID 的行明确显示为空。\n\n",limit)?;
    field(&mut output, "快照", &document.snapshot_key, limit)?;
    field(&mut output, "范围", &encoded(&document.summary), limit)?;
    field(&mut output, "范围类型", &document.scope_kind, limit)?;
    field(
        &mut output,
        "正式角色筛选",
        &encoded(&document.speaker),
        limit,
    )?;
    field(
        &mut output,
        "目标 locale",
        document.target_locale.as_deref().unwrap_or("源文"),
        limit,
    )?;
    field(
        &mut output,
        "locale 策略",
        &encoded(&document.locale_policy),
        limit,
    )?;
    field(
        &mut output,
        "包含演出备注",
        &document.direction_included.to_string(),
        limit,
    )?;
    if !document.summary.includes_fragment_closure {
        append(
            &mut output,
            "\n直接范围：被调用的片段定义未纳入，不能称为完整角色台本。\n",
            limit,
        )?;
    }
    if document.locale_policy == ProductionLocalePolicy::SourceFallback {
        append(
            &mut output,
            "\n已明确选择源文回退；每行保留真实译文状态，回退行另作标记。\n",
            limit,
        )?;
    }
    append(&mut output, "\n## 定义与使用关系\n\n", limit)?;
    for definition in &document.definitions {
        field(&mut output, "定义", &encoded(definition), limit)?;
    }
    for occurrence in &document.chapter_occurrences {
        field(&mut output, "章节出现", &encoded(occurrence), limit)?;
    }
    for call in &document.call_sites {
        field(&mut output, "静态调用点", &encoded(call), limit)?;
    }
    append(&mut output, "\n## 台词与所选文字\n", limit)?;
    for (index, row) in document.rows.iter().enumerate() {
        append(&mut output, &format!("\n### {}\n\n", index + 1), limit)?;
        field(
            &mut output,
            "正式说话者",
            &row.speaker
                .as_ref()
                .map(|speaker| {
                    format!(
                        "{} · {}:{}",
                        speaker.display, speaker.target.kind, speaker.target.id
                    )
                })
                .unwrap_or_else(|| "无（旁白或选项）".into()),
            limit,
        )?;
        field(&mut output, "来源", &encoded(&row.source), limit)?;
        field(&mut output, "所属声明", &encoded(&row.declaration), limit)?;
        field(
            &mut output,
            "稳定行 ID",
            row.stable_line_id.as_deref().unwrap_or("无持久行 ID"),
            limit,
        )?;
        field(&mut output, "源版本", &row.source_revision, limit)?;
        field(&mut output, "当前行锚", &row.row_key, limit)?;
        field(&mut output, "类别", build::kind_name(row.kind), limit)?;
        field(
            &mut output,
            "locale",
            row.target_locale.as_deref().unwrap_or("源文"),
            limit,
        )?;
        field(&mut output, "状态", &encoded(&row.status), limit)?;
        field(
            &mut output,
            "源文回退",
            &row.used_source_fallback.to_string(),
            limit,
        )?;
        field(&mut output, "源文", &text(&row.source_parts), limit)?;
        field(&mut output, "所选文字", &text(&row.selected_parts), limit)?;
        field(
            &mut output,
            "源文 typed parts",
            &encoded(&row.source_parts),
            limit,
        )?;
        field(
            &mut output,
            "所选 typed parts",
            &encoded(&row.selected_parts),
            limit,
        )?;
        field(
            &mut output,
            "定义内部控制祖先",
            &encoded(&row.control_ancestry),
            limit,
        )?;
        field(
            &mut output,
            "外部直接调用使用",
            &encoded(&row.external_call_uses),
            limit,
        )?;
        if let Some(direction) = &row.direction {
            field(&mut output, "演出备注（源语言）", direction, limit)?;
        }
    }
    Ok(output)
}
fn csv(document: &ProductionDocument, limit: usize) -> Result<String, ProductionError> {
    let mut output = String::new();
    let mut header: Vec<String> = [
        "record_type",
        "metadata",
        "row_key",
        "kind",
        "speaker",
        "declaration",
        "file",
        "line",
        "column",
        "stable_line_id",
        "source_revision",
        "target_locale",
        "status",
        "used_source_fallback",
        "source_parts",
        "selected_parts",
        "source_text",
        "selected_text",
        "control_ancestry",
        "external_call_uses",
    ]
    .iter()
    .map(|value| (*value).into())
    .collect();
    if document.direction_included {
        header.push("direction".into());
    }
    record(&mut output, &header, limit)?;
    let metadata = serde_json::json!({"schema_version":document.schema_version,"snapshot_key":document.snapshot_key,"summary":document.summary,
        "scope_kind":document.scope_kind,"speaker":document.speaker,"target_locale":document.target_locale,
        "locale_policy":document.locale_policy,"metadata_language":document.metadata_language,"direction_included":document.direction_included,
        "csv_notice":"每个单元格均有单引号显示前缀；不是无损回导。精确原值使用 JSON。不保证所有表格软件及再次另存后的通用安全。"});
    let mut values = vec![String::new(); header.len()];
    values[0] = "metadata".into();
    values[1] = encoded(&metadata);
    record(&mut output, &values, limit)?;
    for (kind, items) in [
        (
            "definition",
            document.definitions.iter().map(encoded).collect::<Vec<_>>(),
        ),
        (
            "chapter_occurrence",
            document.chapter_occurrences.iter().map(encoded).collect(),
        ),
        (
            "call_site",
            document.call_sites.iter().map(encoded).collect(),
        ),
    ] {
        for item in items {
            let mut values = vec![String::new(); header.len()];
            values[0] = kind.into();
            values[1] = item;
            record(&mut output, &values, limit)?;
        }
    }
    for row in &document.rows {
        let mut values = vec![
            "row".into(),
            String::new(),
            row.row_key.clone(),
            build::kind_name(row.kind).into(),
            encoded(&row.speaker),
            encoded(&row.declaration),
            row.source.file.clone(),
            row.source.line.to_string(),
            row.source.column.to_string(),
            row.stable_line_id.clone().unwrap_or_default(),
            row.source_revision.clone(),
            row.target_locale.clone().unwrap_or_default(),
            encoded(&row.status),
            row.used_source_fallback.to_string(),
            encoded(&row.source_parts),
            encoded(&row.selected_parts),
            text(&row.source_parts),
            text(&row.selected_parts),
            encoded(&row.control_ancestry),
            encoded(&row.external_call_uses),
        ];
        if document.direction_included {
            values.push(row.direction.clone().unwrap_or_default());
        }
        record(&mut output, &values, limit)?;
    }
    Ok(output)
}
fn record(output: &mut String, cells: &[String], limit: usize) -> Result<(), ProductionError> {
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            append(output, ",", limit)?;
        }
        append(output, "\"'", limit)?;
        for part in cell.split_inclusive('"') {
            append(output, part, limit)?;
            if part.ends_with('"') {
                append(output, "\"", limit)?;
            }
        }
        append(output, "\"", limit)?;
    }
    append(output, "\r\n", limit)
}

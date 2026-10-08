use super::*;
use serde::ser::SerializeSeq;
use std::io::{self, Write};

pub(super) fn check_counts(
    limits: Option<&TemplateImpactLimits>,
    instances: usize,
    fields: usize,
) -> Result<(), String> {
    if let Some(limits) = limits {
        if instances > limits.max_instances {
            return Err(limit_error("实例数量超过预算"));
        }
        if fields > limits.max_field_values {
            return Err(limit_error("字段值数量超过预算"));
        }
    }
    Ok(())
}

pub(super) fn limit_error(message: &str) -> String {
    format!("TemplateImpactLimit：{message}")
}

pub(super) struct ReportHead<'a> {
    pub(super) revision: Revision,
    pub(super) baseline: &'a str,
    pub(super) complete: bool,
    pub(super) incomplete_reason: Option<&'a str>,
    pub(super) current: Option<&'a ProjectTemplate>,
    pub(super) proposed: Option<&'a ProjectTemplate>,
    pub(super) template_diagnostics: &'a [Diagnostic],
    pub(super) source_diagnostics: &'a [Diagnostic],
    pub(super) changed_files: &'a [PathBuf],
}

#[derive(Serialize)]
struct SummaryRef<'a> {
    id: &'a str,
    title: &'a str,
    applies_to: &'a TargetRef,
    applies_to_entity_type: &'a Option<String>,
}
impl<'a> From<&'a ProjectTemplate> for SummaryRef<'a> {
    fn from(template: &'a ProjectTemplate) -> Self {
        Self {
            id: &template.id,
            title: &template.title,
            applies_to: &template.applies_to,
            applies_to_entity_type: &template.applies_to_entity_type,
        }
    }
}

struct CombinedDiagnostics<'a>(&'a [Diagnostic], &'a [Diagnostic]);
impl Serialize for CombinedDiagnostics<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        for diagnostic in self.0.iter().chain(self.1) {
            sequence.serialize_element(diagnostic)?;
        }
        sequence.end()
    }
}

pub(super) struct ImpactBudget {
    maximum: Option<usize>,
    used: usize,
}

impl ImpactBudget {
    pub(super) fn new(
        limits: Option<&TemplateImpactLimits>,
        head: ReportHead<'_>,
    ) -> Result<Self, String> {
        let mut budget = Self {
            maximum: limits.map(|limits| limits.max_output_bytes),
            used: 0,
        };
        #[derive(Serialize)]
        struct Report<'a> {
            expected_revision: Revision,
            expected_baseline: &'a str,
            complete: bool,
            incomplete_reason: Option<&'a str>,
            current_template: Option<SummaryRef<'a>>,
            proposed_template: Option<SummaryRef<'a>>,
            field_changes: &'a [ProjectTemplateFieldChange],
            instances: &'a [ProjectTemplateInstanceImpact],
            diagnostics: CombinedDiagnostics<'a>,
            changed_files: &'a [PathBuf],
        }
        budget.json(&Report {
            expected_revision: head.revision,
            expected_baseline: head.baseline,
            complete: head.complete,
            incomplete_reason: head.incomplete_reason,
            current_template: head.current.map(SummaryRef::from),
            proposed_template: head.proposed.map(SummaryRef::from),
            field_changes: &[],
            instances: &[],
            diagnostics: CombinedDiagnostics(head.template_diagnostics, head.source_diagnostics),
            changed_files: head.changed_files,
        })?;
        Ok(budget)
    }

    /// 借用序列化到计数 sink；巨大属性值不会先克隆或创建 JSON Vec。
    pub(super) fn json<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), String> {
        if let Some(maximum) = self.maximum {
            let mut writer = CountingWriter {
                maximum,
                used: &mut self.used,
            };
            serde_json::to_writer(&mut writer, value)
                .map_err(|_| limit_error("影响报告字节超过预算"))?;
        }
        Ok(())
    }

    pub(super) fn comma(&mut self, needed: bool) -> Result<(), String> {
        if needed {
            self.ensure_additional(1)?;
            self.used = self
                .used
                .checked_add(1)
                .ok_or_else(|| limit_error("报告字节计数溢出"))?;
        }
        Ok(())
    }

    pub(super) fn ensure_additional(&self, bytes: usize) -> Result<(), String> {
        if self
            .maximum
            .is_some_and(|maximum| bytes > maximum.saturating_sub(self.used))
        {
            return Err(limit_error("影响报告字节超过预算"));
        }
        Ok(())
    }
}

struct CountingWriter<'a> {
    maximum: usize,
    used: &'a mut usize,
}
impl Write for CountingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .used
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("报告字节计数溢出"))?;
        if next > self.maximum {
            return Err(io::Error::other("影响报告字节超过预算"));
        }
        *self.used = next;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

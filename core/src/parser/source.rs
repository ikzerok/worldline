//! Parser 与分析共享明确结构 slot；原稿片段在词法生产时确定，不做文本搜索。
use super::*;
use crate::source_provenance::{ExpressionSource, ExpressionSlot};

impl Parser<'_> {
    pub(super) fn sourced_expr(
        &mut self, source: &str, file: &str, line: u32, owner: u32,
        base: u32, slot: ExpressionSlot,
    ) -> Expr {
        let (expr, provenance) = crate::expression::parse_expr_with_source(source, file, line, base, self.diags);
        self.sources.insert_expression(file, owner, slot, provenance);
        expr
    }

    pub(super) fn sourced_text(
        &mut self, source: &str, file: &str, line: u32, base: u32, quoted: bool,
    ) -> Vec<TextPart> {
        let parse = if quoted { crate::expression::parse_quoted_with_sources } else { crate::expression::parse_text_with_sources };
        let (parts, sources) = parse(source, file, line, base, self.diags, self.options());
        self.record_text_sources(file, line, sources);
        parts
    }

    pub(super) fn record_text_sources(&mut self, file: &str, owner: u32, sources: Vec<ExpressionSource>) {
        for (index, source) in sources.into_iter().enumerate() {
            self.sources.insert_expression(file, owner, ExpressionSlot::Text(index as u32), source);
        }
    }

    pub(super) fn sourced_label(&mut self, label: &str, line: &Line) -> Vec<TextPart> {
        let base = line.source.base + line.source.label;
        let start = self.diags.len();
        let (mut parts, mut sources) = crate::expression::parse_text_with_sources(label, &line.file, line.no, base, self.diags, self.options());
        if !line.source.label_boundaries.is_empty() {
            crate::expression::remap_parts(&mut parts, base, &line.source.label_boundaries);
            for source in &mut sources { source.map_boundaries(base, &line.source.label_boundaries); }
            for diagnostic in &mut self.diags[start..] {
                crate::source_provenance::map_span(&mut diagnostic.span, base, &line.source.label_boundaries);
            }
        }
        self.record_text_sources(&line.file, line.no, sources);
        parts
    }

    pub(super) fn source_line(&self, file: &str, line: u32) -> Option<&Line> {
        self.lines.iter().find(|source| source.file == file && source.no == line)
    }

    pub(super) fn source_remainder(&self, file: &str, line: u32) -> u32 {
        self.source_line(file, line).map(|line| line.source.base + line.source.remainder).unwrap_or(0)
    }
}

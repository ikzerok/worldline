use crate::{ChoiceIdentity, Story};
use worldline_core::ast::Stmt;

#[derive(Clone)]
pub(crate) struct RawSource {
    pub file: String,
    pub line: u32,
    pub column: u32,
}
#[derive(Default)]
pub(crate) struct OutputSources {
    pub(super) pending: Vec<Option<RawSource>>,
    pub(super) exhausted: bool,
    bytes: usize,
}
impl Story<'_> {
    /// 仅独立报告 Story 开启；不改变 Output、trace、存档或 RNG。
    pub(crate) fn capture_report_output(&mut self, frame: usize) {
        if self.action_capture.report_outputs.is_none() {
            return;
        }
        let source = self.report_statement_source(frame, self.frames[frame].idx);
        let capture = self
            .action_capture
            .report_outputs
            .as_mut()
            .expect("报告捕获已开启");
        let bytes = source
            .as_ref()
            .map_or(1, |s| s.file.len().saturating_add(32));
        if capture.pending.len() >= 32768 || capture.bytes.saturating_add(bytes) > 1024 * 1024 {
            capture.exhausted = true;
            return;
        }
        capture.bytes += bytes;
        capture.pending.push(source);
    }
    pub(super) fn report_statement_source(&self, frame: usize, index: usize) -> Option<RawSource> {
        let statement = self.frames.get(frame)?.stmts.get(index)?;
        let loc = match statement {
            Stmt::Text(text) => text.loc,
            Stmt::Say(say) => say.loc,
            Stmt::Choice(choice) => choice.loc,
            _ => return None,
        };
        let node = self.current_node()?;
        let file = worldline_core::evidence_source::runtime_output_source_file(
            self.program,
            &node,
            statement,
        )?;
        if file.len() > 8192 {
            return None;
        }
        Some(RawSource {
            file: file.into(),
            line: loc.line,
            column: loc.column,
        })
    }
    pub(crate) fn report_choice(
        &self,
        identity: &ChoiceIdentity,
    ) -> Option<(ChoiceIdentity, Option<RawSource>)> {
        let pause = self.paused.as_ref()?;
        let actual = pause
            .explanations
            .iter()
            .find(|c| c.available && c.choice.id == identity.id)?;
        let source =
            self.report_statement_source(pause.frame_depth, pause.start + actual.choice.offset);
        Some((actual.choice.clone(), source))
    }
}
impl OutputSources {
    pub(super) fn take(&mut self) -> Vec<Option<RawSource>> {
        self.bytes = 0;
        std::mem::take(&mut self.pending)
    }
}

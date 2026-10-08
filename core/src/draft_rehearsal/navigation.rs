use super::DraftRehearsalSnapshot;
use crate::{
    evidence_source::{EvidenceSource, EvidenceSourceTarget},
    search_replace::SearchMatch,
    state_inspection_source::DeclarationSource,
};

impl DraftRehearsalSnapshot {
    /// 只从本次不可变编译稿解析声明头；调用者回源前还必须 verify_navigation。
    pub fn evidence_source(&self, source: &EvidenceSource) -> Result<SearchMatch, String> {
        self.source_hit(crate::evidence_source::resolve_evidence_source(
            &self.compiled,
            source,
        )?)
    }

    pub fn declaration_source(&self, source: &DeclarationSource) -> Result<SearchMatch, String> {
        self.source_hit(
            crate::state_inspection_source::resolve_state_inspection_source(
                &self.compiled,
                source,
            )?,
        )
    }

    fn source_hit(&self, target: EvidenceSourceTarget) -> Result<SearchMatch, String> {
        let text = self
            .compiled
            .sources
            .get(&target.path)
            .ok_or("试演来源文件不存在")?;
        let preview = text
            .get(target.range.clone())
            .ok_or("试演来源范围无法确认")?
            .into();
        let relative = target
            .path
            .strip_prefix(&self.root)
            .map_err(|_| "试演来源越界")?;
        let draft = self
            .request
            .drafts
            .iter()
            .any(|draft| draft.path == relative);
        Ok(SearchMatch {
            path: target.path,
            range: target.range,
            line: target.line,
            column: target.column,
            preview,
            context: None,
            identity: None,
            replaceable: false,
            draft,
        })
    }
}

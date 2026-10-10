//! 正式说话者的同稿静态生产台本；唯一实现见 spec/production-script.md。
mod build;
mod collect;
mod guards;
mod input;
#[cfg(not(target_arch = "wasm32"))]
mod native;
mod render;
mod scope;
#[cfg(test)]
mod tests;
mod types;
use crate::manuscript::{ManuscriptQueryDraft, ManuscriptQuerySnapshot, WritingBuffer};
use crate::{project::Project, TargetRef};
pub use input::{parse_production_export_options, parse_production_script_request};
#[cfg(not(target_arch = "wasm32"))]
pub use native::write_production_script_new;
use serde::Serialize;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
pub use types::*;

#[derive(Debug, Clone)]
pub struct ProductionScriptSnapshot {
    key: String,
    input_key: String,
    root: PathBuf,
    request: ProductionScriptRequest,
    query: Arc<ManuscriptQuerySnapshot>,
    summary: ProductionScopeSummary,
    definitions: Vec<ProductionDefinition>,
    chapter_occurrences: Vec<ProductionChapterOccurrence>,
    call_sites: Vec<ProductionCallUse>,
    rows: Vec<ProductionRow>,
    directions: BTreeMap<String, String>,
}
impl ProductionScriptSnapshot {
    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn summary(&self) -> &ProductionScopeSummary {
        &self.summary
    }
    pub fn definitions(&self) -> &[ProductionDefinition] {
        &self.definitions
    }
    pub fn chapter_occurrences(&self) -> &[ProductionChapterOccurrence] {
        &self.chapter_occurrences
    }
    pub fn call_sites(&self) -> &[ProductionCallUse] {
        &self.call_sites
    }
    pub fn author_direction(&self, row_key: &str) -> Option<&str> {
        self.directions.get(row_key).map(String::as_str)
    }
    pub fn page(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<ProductionScriptPage, ProductionError> {
        if !(1..=100).contains(&limit) {
            return Err(ProductionError::new(
                "INVALID_LIMIT",
                "制作台本每页数量必须为1到100",
            ));
        }
        let end = offset.saturating_add(limit).min(self.rows.len());
        Ok(ProductionScriptPage {
            schema_version: 1,
            snapshot_key: self.key.clone(),
            summary: self.summary.clone(),
            offset,
            limit,
            total: self.rows.len(),
            next_offset: (end < self.rows.len()).then_some(end),
            rows: self.rows.iter().skip(offset).take(limit).cloned().collect(),
        })
    }
}
#[derive(Debug, Clone)]
pub struct ProductionArtifact {
    bytes: Vec<u8>,
    format: ProductionFormat,
    snapshot_key: String,
}
impl ProductionArtifact {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn format(&self) -> ProductionFormat {
        self.format
    }
    pub fn snapshot_key(&self) -> &str {
        &self.snapshot_key
    }
}
impl Project {
    pub fn validate_production_script(
        &self,
        buffers: &[WritingBuffer],
        drafts: &[ManuscriptQueryDraft],
        snapshot: &ProductionScriptSnapshot,
    ) -> Result<(), ProductionError> {
        if self.root != snapshot.root
            || self.manuscript_query_key(buffers, drafts) != snapshot.input_key
        {
            return Err(ProductionError::new(
                "STALE_SNAPSHOT",
                "当前正文、角色、编排或 locale 已变化，请重新生成台本",
            ));
        }
        self.verify_manuscript_delivery_observation(&snapshot.query)
            .map_err(|e| ProductionError::new(&e.code, e.message))
    }
    pub fn production_script_source_hit(
        &self,
        buffers: &[WritingBuffer],
        drafts: &[ManuscriptQueryDraft],
        snapshot: &ProductionScriptSnapshot,
        row_key: &str,
    ) -> Result<crate::search_replace::SearchMatch, ProductionError> {
        self.validate_production_script(buffers, drafts, snapshot)?;
        let row = snapshot
            .rows
            .iter()
            .find(|row| row.row_key == row_key)
            .ok_or_else(ProductionError::source)?;
        let source = crate::localization::LocalizationSource {
            file: row.source.file.clone(),
            line: row.source.line,
            kind: match row.kind {
                ProductionKind::Say => "say",
                ProductionKind::Text => "text",
                ProductionKind::Choice => "choice",
            }
            .into(),
        };
        crate::localization::localization_source_hit(
            snapshot.query.compiled(),
            &self.root,
            &source,
            buffers.iter().any(|buffer| {
                buffer.path() == self.root.join(&row.source.file) && buffer.is_changed()
            }),
        )
        .map_err(|e| ProductionError::new("INVALID_SOURCE", e))
    }
}
pub(super) fn bounded_size(value: &impl Serialize, limit: usize) -> Result<usize, ProductionError> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("production_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(limit);
    serde_json::to_writer(&mut counter, value).map_err(|_| ProductionError::budget())?;
    Ok(limit - counter.0)
}
fn digest(value: &impl Serialize) -> String {
    struct Hash(u64);
    impl std::io::Write for Hash {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            for byte in bytes {
                self.0 ^= u64::from(*byte);
                self.0 = self.0.wrapping_mul(0x100000001b3);
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut hash = Hash(0xcbf29ce484222325);
    serde_json::to_writer(&mut hash, value).expect("生产台本字段可序列化");
    format!("production-{:016x}", hash.0)
}

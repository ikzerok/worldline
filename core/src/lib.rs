//! worldline-core:语言解析、工程编辑与唯一分析真相。
pub mod analysis;
mod analysis_helpers;
mod analysis_metadata;
pub mod anchors;
pub mod ast;
pub mod authoring;
pub mod catalog;
pub mod catalog_edit;
mod catalog_syntax;
mod compiler;
mod content_baseline;
pub mod deletion_content_references;
pub mod diagnostic;
pub mod expression;
pub mod file_access;
mod fingerprint;
pub mod graph;
pub mod lexer;
pub mod map_creation;
pub mod migration;
pub mod navigation;
pub mod parser;
pub mod presentation;
pub mod presentation_commands;
pub mod project;
pub mod recovery;
pub mod reference_impact;
pub mod relation_context;
pub mod states;
mod storage;
pub mod timeline;
pub mod wiki;
mod workspace_documents;
pub mod workspace_snapshot;

pub use analysis::{Analysis, Stats, Symbols};
pub use ast::{Program, ValueKind};
pub use compiler::{
    compile_path, compile_path_with_options, compile_source, compile_source_with_options,
    compile_sources, compile_sources_with_options, compile_text_with_disk_includes,
    compile_text_with_disk_includes_with_options, CompileOptions, LanguageVersion,
};
pub use diagnostic::{sort_diagnostics, Diagnostic, Severity, Span};
pub use fingerprint::fingerprint_program;
pub use graph::{AnchorDecl, EdgeKind, GraphEdge, GraphNode, RelationGraph};
pub use presentation::{
    MapCanvas, MapDocument, MapGeometry, MapIndex, MapLayer, MapNavigation, MapPlacement,
    MapPlacementRef, MapRasterLayer,
};

/// 编译快照:源文件、程序、分析与全部诊断。
pub struct CompileResult {
    pub program: Program,
    pub analysis: Analysis,
    pub diagnostics: Vec<Diagnostic>,
    pub sources: std::collections::BTreeMap<std::path::PathBuf, String>,
    pub options: CompileOptions,
}
impl CompileResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

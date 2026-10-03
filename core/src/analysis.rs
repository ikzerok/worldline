//! 语义分析:唯一的分析真相 —— 规范见 `spec/diagnostics.md` 与 `spec/relations.md`。
//! 职责:符号收集、引用解析、类型检查、流分析(可达性/终止性)、关系图。

use crate::ast::Program;
use crate::diagnostic::Diagnostic;
pub use crate::fingerprint::fingerprint_program;

mod builder;
mod projection;

pub use projection::{
    Analysis, CharacterInfo, CharacterRelationInfo, NodePath, Stats, StorylineInfo, Symbols,
    VarInfo, WorldInfo,
};

pub fn analyze(program: &Program, parse_diags: Vec<Diagnostic>) -> (Analysis, Vec<Diagnostic>) {
    builder::analyze(program, parse_diags)
}

/// 显式开始试玩时追加到编译诊断的只读提示；普通历史资料检查不调用。
/// Program 与 Analysis 必须来自同一次编译；只检查当前 Program.entry，不改指纹。
pub fn execution_diagnostics(program: &Program, analysis: &Analysis) -> Vec<Diagnostic> {
    let mut diagnostics = builder::execution_diagnostics(program, analysis);
    program
        .source_provenance
        .resolve_diagnostics(&mut diagnostics);
    diagnostics
}

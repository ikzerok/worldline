//! 运行检查器的声明来源；声明不代表最后写入或因果解释。
use crate::evidence_source::{EvidenceSourcePrecision, EvidenceSourceTarget};
use crate::lexer::LineKind;
use crate::{catalog::CatalogDecl, CompileResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarationKind {
    GlobalVariable,
    State,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclarationSource {
    pub kind: DeclarationKind,
    pub id: String,
    pub file: String,
    pub line: u32,
}

pub fn resolve_state_inspection_source(
    snapshot: &CompileResult,
    source: &DeclarationSource,
) -> Result<EvidenceSourceTarget, String> {
    if snapshot.has_errors() || source.line == 0 || source.file.is_empty() {
        return Err("当前编译稿或声明来源无效".into());
    }
    let valid = match source.kind {
        DeclarationKind::GlobalVariable => snapshot
            .analysis
            .symbols
            .vars
            .get(&source.id)
            .is_some_and(|v| v.decl_file == source.file && v.decl_span.line == source.line),
        DeclarationKind::State => snapshot
            .analysis
            .catalog
            .states
            .get(&source.id)
            .is_some_and(|v| v.file == source.file && v.line == source.line),
    };
    if !valid {
        return Err("声明身份与此编译快照不一致".into());
    }
    let path = PathBuf::from(&source.file);
    let text = snapshot
        .sources
        .get(&path)
        .ok_or("声明来源不属于此编译快照")?;
    let lines = crate::lexer::lex_source_with_options(
        &source.file,
        text,
        &mut Vec::new(),
        snapshot.options,
    );
    let matching = lines
        .into_iter()
        .filter(|line| line.no == source.line)
        .filter(|line| match (&source.kind, line.physical().kind) {
            (
                DeclarationKind::GlobalVariable,
                LineKind::Let { name, .. } | LineKind::Const { name, .. },
            ) => name == source.id,
            (DeclarationKind::State, LineKind::Catalog(CatalogDecl::State(state))) => {
                state.id == source.id
            }
            _ => false,
        })
        .count();
    if matching != 1 {
        return Err("无法唯一确认真实声明头".into());
    }
    let mut offset = 0;
    for (index, raw) in text.split_inclusive('\n').enumerate() {
        if index + 1 == source.line as usize {
            let header = raw.trim_end_matches(['\r', '\n']);
            let start = header.len() - header.trim_start().len();
            if start == header.len() {
                return Err("声明头为空".into());
            }
            return Ok(EvidenceSourceTarget {
                path,
                range: offset + start..offset + header.len(),
                line: source.line,
                column: header[..start].chars().count() as u32 + 1,
                precision: EvidenceSourcePrecision::StatementHeader,
            });
        }
        offset += raw.len();
    }
    Err("声明物理行不存在".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspection_declaration_rejects_forged_or_changed_source() {
        let snapshot = crate::compile_source("world.wl", "let count = 0\nevent start\n  -> END\n");
        assert!(!snapshot.has_errors(), "{:?}", snapshot.diagnostics);
        let info = &snapshot.analysis.symbols.vars["count"];
        let mut source = DeclarationSource {
            kind: DeclarationKind::GlobalVariable,
            id: "count".into(),
            file: info.decl_file.clone(),
            line: info.decl_span.line,
        };
        let hit = resolve_state_inspection_source(&snapshot, &source).unwrap();
        assert_eq!(&snapshot.sources[&hit.path][hit.range], "let count = 0");
        source.id = "other".into();
        assert!(resolve_state_inspection_source(&snapshot, &source).is_err());
        source.id = "count".into();
        source.line += 1;
        assert!(resolve_state_inspection_source(&snapshot, &source).is_err());
    }
}

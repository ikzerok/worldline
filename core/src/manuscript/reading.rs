//! 当前稿只读编译与静态阅读投影；不执行表达式或运行状态。
use super::WritingBuffer;
use crate::ast::{Stmt, TextPart};
use crate::catalog::TargetRef;
use crate::project::Project;
use crate::CompileResult;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadingPart {
    pub text: String,
    pub target: Option<TargetRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadingProjection {
    pub target: TargetRef,
    pub lines: Vec<Vec<ReadingPart>>,
}

impl Project {
    /// 只读草稿覆盖。不应用、不保存；相同文件的不同草稿不得择一覆盖。
    pub fn compile_writing_drafts(
        &self,
        buffers: &[WritingBuffer],
    ) -> Result<CompileResult, String> {
        let mut candidate = self.clone();
        let baseline = self.content_baseline();
        let mut sources = BTreeMap::new();
        for buffer in buffers.iter().filter(|buffer| buffer.is_changed()) {
            if buffer.baseline() != baseline {
                return Err("草稿基线已过期，当前输入完整保留；请核对工程变化".into());
            }
            if let Some(previous) = sources.insert(buffer.path(), buffer.source()) {
                if previous != buffer.source() {
                    return Err("同一文件存在不同草稿，不能确定当前阅读来源".into());
                }
            }
            candidate.set_text(buffer.path(), buffer.source().to_owned())?;
        }
        let result = candidate.compile_current();
        if result.has_errors() {
            let diagnostic = result
                .diagnostics
                .iter()
                .find(|item| item.severity == crate::diagnostic::Severity::Error);
            return Err(diagnostic.map_or_else(
                || "当前稿暂不能解析；输入已保留".into(),
                |item| {
                    format!(
                        "当前稿暂不能解析：{}:{} {} {}",
                        item.file, item.span.line, item.code, item.message
                    )
                },
            ));
        }
        Ok(result)
    }
}

/// 从统一编译结果读取静态正文，所有分支按源码顺序展示。
pub fn reading_projection(
    result: &CompileResult,
    target: &TargetRef,
) -> Result<ReadingProjection, String> {
    if result.has_errors() {
        return Err("当前编译结果含错误，不能作为当前稿阅读预览".into());
    }
    let object = result
        .analysis
        .catalog
        .object(target)
        .ok_or("正文目标不存在或尚未解析")?;
    let body = match target.kind.as_str() {
        "event" => result
            .program
            .events
            .iter()
            .find(|event| event.name == target.id)
            .map(|event| event.body.as_slice()),
        "scene" => result
            .program
            .events
            .iter()
            .zip(&result.program.event_files)
            .filter(|(_, file)| *file == &object.file)
            .find_map(|(event, _)| find_scene(&event.body, object.line)),
        "fragment" => result
            .program
            .fragments
            .iter()
            .find(|fragment| fragment.name == target.id)
            .map(|fragment| fragment.body.as_slice()),
        "entity" => {
            let entity = result
                .analysis
                .catalog
                .entities
                .get(&target.id)
                .ok_or("实体正文来源不可用")?;
            return Ok(ReadingProjection {
                target: target.clone(),
                lines: vec![vec![ReadingPart {
                    text: entity.description.clone(),
                    target: None,
                }]],
            });
        }
        _ => return Err("此目标类型不支持静态正文阅读".into()),
    }
    .ok_or("正文来源无法在当前 core 编译结果中定位")?;
    let mut lines = Vec::new();
    collect(body, &mut lines);
    Ok(ReadingProjection {
        target: target.clone(),
        lines,
    })
}

fn find_scene(statements: &[Stmt], line: u32) -> Option<&[Stmt]> {
    for statement in statements {
        let found = match statement {
            Stmt::Scene(scene) if scene.loc.line == line => Some(scene.body.as_slice()),
            Stmt::Scene(scene) => find_scene(&scene.body, line),
            Stmt::Choice(choice) => find_scene(&choice.body, line),
            Stmt::If(branches) => branches
                .branches
                .iter()
                .find_map(|(_, body)| find_scene(body, line)),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn collect(statements: &[Stmt], lines: &mut Vec<Vec<ReadingPart>>) {
    for statement in statements {
        match statement {
            Stmt::Text(text) => lines.push(parts(&text.parts, None)),
            Stmt::Say(say) => {
                lines.push(vec![ReadingPart {
                    text: format!("{}：", say.speaker),
                    target: Some(TargetRef::new("character", &say.speaker)),
                }]);
                lines.push(parts(&say.text.parts, None));
            }
            Stmt::Call(call) => lines.push(vec![ReadingPart {
                text: format!("[调用片段 {}，静态预览不展开]", call.name),
                target: Some(TargetRef::new("fragment", &call.name)),
            }]),
            Stmt::Choice(choice) => {
                lines.push(parts(&choice.label, Some("选项：")));
                collect(&choice.body, lines);
            }
            Stmt::If(branches) => {
                for (_, body) in &branches.branches {
                    collect(body, lines);
                }
            }
            Stmt::Scene(scene) => collect(&scene.body, lines),
            _ => {}
        }
    }
}

fn parts(source: &[TextPart], prefix: Option<&str>) -> Vec<ReadingPart> {
    let mut parts = Vec::new();
    if let Some(prefix) = prefix {
        parts.push(ReadingPart {
            text: prefix.into(),
            target: None,
        });
    }
    for part in source {
        parts.push(match part {
            TextPart::Str(text) => ReadingPart {
                text: text.clone(),
                target: None,
            },
            TextPart::Link(link) => ReadingPart {
                text: link.label.clone(),
                target: Some(link.target.clone()),
            },
            TextPart::Expr(_) => ReadingPart {
                text: "〔动态内容〕".into(),
                target: None,
            },
        });
    }
    parts
}

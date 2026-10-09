//! 只读 locale 快照与同次求值的源译展示；不参与程序指纹。
mod consumers;
mod replay;
pub use consumers::{
    compare_routes_with_presentation, generate_playthrough_report_with_presentation,
};

use crate::{RunError, Story};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use worldline_core::{
    ast::{Program, Stmt, TextPart},
    localization::{
        LocalizationPart, LocalizationPresentationRequest, LocalizationPresentationSnapshot,
        LocalizationSource, LocalizationStatus,
    },
    navigation::RenderedLink,
    Analysis,
};

pub const LOCALIZATION_PRESENTATION_CAPABILITY: &str = "runtime.localization.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeLocalizationIdentity {
    pub request: LocalizationPresentationRequest,
    pub presentation_digest: String,
}

/// 位置来自当前冻结快照；source_content/source_links 来自实际求值，不是预览样例。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalizedPresentation {
    pub id: Option<String>,
    pub status: LocalizationStatus,
    pub source: LocalizationSource,
    pub source_revision: String,
    pub source_baseline: String,
    pub sidecar_path: Option<String>,
    pub translation_pointer: Option<String>,
    pub source_content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_links: Vec<RenderedLink>,
}

#[derive(Clone)]
pub(crate) struct PresentationContext {
    pub snapshot: Arc<LocalizationPresentationSnapshot>,
    pub identity: RuntimeLocalizationIdentity,
}

impl PresentationContext {
    pub fn new(snapshot: &LocalizationPresentationSnapshot) -> Self {
        Self {
            snapshot: Arc::new(snapshot.clone()),
            identity: RuntimeLocalizationIdentity {
                request: LocalizationPresentationRequest {
                    schema_version: 1,
                    target_locale: snapshot.target_locale().into(),
                    policy: snapshot.policy(),
                },
                presentation_digest: snapshot.presentation_digest().into(),
            },
        }
    }
    pub fn validate(&self, program: &Program, analysis: &Analysis) -> Result<(), RunError> {
        self.snapshot
            .validate_program(program, analysis)
            .map_err(|error| RunError::new(error.to_string()))
    }
}

pub(crate) fn require_identity(
    expected: Option<&RuntimeLocalizationIdentity>,
    context: Option<&PresentationContext>,
) -> Result<(), RunError> {
    if expected != context.map(|context| &context.identity) {
        return Err(RunError::new(
            "运行展示身份不匹配：请准备记录中的 locale 与回退策略，并使用同一译文快照入口",
        ));
    }
    Ok(())
}

pub(crate) fn output_sources(
    program: &Program,
) -> Result<worldline_core::evidence_source::RuntimeOutputSourceIndex<'_>, RunError> {
    let sources = worldline_core::evidence_source::RuntimeOutputSourceIndex::new(program);
    if sources.has_unresolved_file_links() {
        return Err(RunError::new(
            "正文文件链接来源缺失或有歧义，拒绝猜测相对目标",
        ));
    }
    Ok(sources)
}

pub(crate) struct RenderedText {
    pub content: String,
    pub links: Vec<RenderedLink>,
    pub localization: Option<LocalizedPresentation>,
}
impl RenderedText {
    pub fn source_content(&self) -> &str {
        self.localization
            .as_ref()
            .map_or(self.content.as_str(), |value| value.source_content.as_str())
    }
}

impl<'p> Story<'p> {
    pub fn new_localized(
        program: &'p Program,
        analysis: &'p Analysis,
        snapshot: &LocalizationPresentationSnapshot,
    ) -> Result<Self, RunError> {
        Self::new_with_presentation(program, analysis, crate::util::seed_now(), snapshot)
    }

    /// 校验完整快照后才初始化全局变量、准入和效果。
    pub fn new_with_presentation(
        program: &'p Program,
        analysis: &'p Analysis,
        seed: u64,
        snapshot: &LocalizationPresentationSnapshot,
    ) -> Result<Self, RunError> {
        Self::new_with_context(
            program,
            analysis,
            seed,
            Some(PresentationContext::new(snapshot)),
        )
    }

    pub fn presentation_identity(&self) -> Option<&RuntimeLocalizationIdentity> {
        self.presentation.as_ref().map(|context| &context.identity)
    }

    pub(super) fn render_statement(&self, statement: &Stmt) -> Result<RenderedText, RunError> {
        let (parts, line, kind) = match statement {
            Stmt::Text(text) => (text.parts.as_slice(), text.loc.line, "text"),
            Stmt::Say(say) => (say.text.parts.as_slice(), say.loc.line, "say"),
            Stmt::Choice(choice) => (choice.label.as_slice(), choice.loc.line, "choice"),
            _ => return Err(RunError::new("当前语句不是正文、台词或选择标签")),
        };
        let source_file = self.output_sources.get(statement);
        let Some(context) = &self.presentation else {
            let (content, links) = self.render_parts(parts, source_file)?;
            return Ok(RenderedText {
                content,
                links,
                localization: None,
            });
        };
        let entry = source_file
            .and_then(|file| context.snapshot.find_entry(file, line, kind))
            .ok_or_else(|| RunError::new("本次语句没有匹配的已验证译文来源，请重新准备运行"))?;
        let mut source_content = String::new();
        let mut source_links = Vec::new();
        let mut values = BTreeMap::new();
        let mut targets = BTreeMap::new();
        // token 来自 core 的同一 source_parts；求值顺序完全由原 AST 决定。
        for (part, token) in parts.iter().zip(&entry.source_parts) {
            match (part, token) {
                (TextPart::Str(text), LocalizationPart::Text { .. }) => {
                    source_content.push_str(text);
                }
                (TextPart::Expr(expression), LocalizationPart::Placeholder { token }) => {
                    let value = self.eval(expression)?.display();
                    source_content.push_str(&value);
                    values.insert(token.as_str(), value);
                }
                (TextPart::Link(link), LocalizationPart::Link { token, .. }) => {
                    let mut target = link.target.clone();
                    if target.kind == "file" {
                        if let Some(file) = source_file {
                            target.id = worldline_core::catalog::resolved_asset(file, &target.id)
                                .to_string_lossy()
                                .into_owned();
                        }
                    }
                    let start = source_content.len();
                    source_content.push_str(&link.label);
                    source_links.push(RenderedLink {
                        target: target.clone(),
                        start,
                        end: source_content.len(),
                    });
                    targets.insert(token.as_str(), target);
                }
                _ => return Err(RunError::new("已验证译文占位与原语句不一致")),
            }
        }
        let (content, links) = if let Some(translation) = &entry.translation_parts {
            let mut content = String::new();
            let mut links = Vec::new();
            for part in translation {
                match part {
                    LocalizationPart::Text { text } => content.push_str(text),
                    LocalizationPart::Placeholder { token } => content.push_str(
                        values
                            .get(token.as_str())
                            .ok_or_else(|| RunError::new("译文引用不存在的已物化占位"))?,
                    ),
                    LocalizationPart::Link { token, label } => {
                        let target = targets
                            .get(token.as_str())
                            .ok_or_else(|| RunError::new("译文引用不存在的原链接"))?;
                        let start = content.len();
                        content.push_str(label);
                        links.push(RenderedLink {
                            target: target.clone(),
                            start,
                            end: content.len(),
                        });
                    }
                }
            }
            (content, links)
        } else {
            (source_content.clone(), source_links.clone())
        };
        Ok(RenderedText {
            content,
            links,
            localization: Some(LocalizedPresentation {
                id: entry.id.clone(),
                status: entry.status,
                source: entry.source.clone(),
                source_revision: entry.source_revision.clone(),
                source_baseline: context.snapshot.source_baseline().into(),
                sidecar_path: entry.sidecar_path.clone(),
                translation_pointer: entry.translation_pointer.clone(),
                source_content,
                source_links,
            }),
        })
    }
}

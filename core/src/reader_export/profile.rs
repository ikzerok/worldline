use super::*;

/// 一个已分配的公开路由；对象/附件/地图与书稿章节两种身份互斥。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderProfileRoute {
    #[serde(
        default,
        deserialize_with = "super::serde_target::deserialize_optional"
    )]
    pub target: Option<TargetRef>,
    pub manuscript_id: Option<String>,
    pub chapter_id: Option<String>,
    pub output_path: String,
}

/// 工程中的发布配置；未知可选顶层字段由文档保存流程按原 JSON 保留。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderPublicationProfile {
    pub schema_version: u32,
    pub required_features: Vec<String>,
    pub id: String,
    pub title: String,
    pub selection: ReaderExportSelection,
    pub routes: Vec<ReaderProfileRoute>,
}

/// 保存计划只描述允许的配置变更，应用时仍须重新计算并核对全部字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderProfileSavePlan {
    pub profile: ReaderPublicationProfile,
    pub content_baseline: String,
    pub document_path: String,
    pub document_before_hash: Option<String>,
    pub plan_digest: String,
}

/// 版本迁移明确列出新增公开语义；应用后返回候选，不自动保存或发布。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderProfileMigrationPlan {
    pub before: ReaderPublicationProfile,
    pub after: ReaderPublicationProfile,
    pub authorization_changes: Vec<String>,
    pub content_baseline: String,
    pub plan_digest: String,
}

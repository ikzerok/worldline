//! 显式授权的静态阅读包；与完整工程备份保持独立。

mod audit;
mod fields;
mod inputs;
mod map_geometry;
mod maps;
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod native_rename;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native_rename::rename_new;
mod paths;
pub(crate) mod plan;
mod profile;
mod profile_api;
mod profile_io;
mod progress;
mod relation_graph;
mod render;
mod routes;
mod semantic_pages;
mod semantics;
mod serde_target;
mod site;
mod site_assets;
mod story;
use crate::catalog::TargetRef;
use crate::manuscript::ManuscriptIndex;
use crate::project::Project;
use crate::CompileResult;
pub use paths::portable_output_path;
pub use profile::{
    ReaderProfileMigrationPlan, ReaderProfileRoute, ReaderProfileSavePlan, ReaderPublicationProfile,
};
pub use progress::ReaderExportProgress;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub const READER_EXPORT_SCHEMA_VERSION: u32 = 1;
pub const READER_FIELDS_SCHEMA_VERSION: u32 = 2;
pub const READER_FIELDS_FEATURE: &str = "reader.fields.v1";
pub const READER_SITE_SCHEMA_VERSION: u32 = 3;
pub const READER_SITE_FEATURE: &str = "reader.world_site.v1";
pub const READER_STORY_FEATURE: &str = "reader.story_details.v1";
pub const READER_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const READER_PROFILES_FEATURE: &str = "reader.profiles.v1";
const MAX_OBJECTS: usize = 500;
const MAX_SITE_OBJECTS: usize = 2_000;
const MAX_OUTPUT_FILES: usize = 10_000;
const MAX_MANUSCRIPTS: usize = 100;
const MAX_CHAPTERS: usize = 5_000;
const MAX_ATTACHMENTS: usize = 128;
const MAX_ATTACHMENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_ATTACHMENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_PACKAGE_BYTES: usize = 128 * 1024 * 1024;

/// 单册显式选择；章节数组顺序决定阅读导航顺序。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderManuscriptSelection {
    pub id: String,
    pub chapters: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderMapSelection {
    pub id: String,
    pub placements: Vec<String>,
    pub raster_layers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderFieldSelection {
    #[serde(deserialize_with = "serde_target::deserialize")]
    pub target: TargetRef,
    pub keys: Vec<String>,
}

/// 作者专用候选值，不代表已授权公开。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderFieldCandidate {
    pub key: String,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderContentPreview {
    pub title: String,
    pub output_path: String,
    pub text: String,
    pub empty_content: bool,
}

/// 阅读包的唯一公开边界。引用不会自动扩大选择范围。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderExportSelection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_features: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<ReaderFieldSelection>,
    pub schema_version: u32,
    pub site_title: String,
    #[serde(deserialize_with = "serde_target::deserialize_vec")]
    pub objects: Vec<TargetRef>,
    pub manuscripts: Vec<ReaderManuscriptSelection>,
    pub attachments: Vec<String>,
    #[serde(default)]
    pub maps: Vec<ReaderMapSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderExportIncluded {
    pub target: Option<TargetRef>,
    pub manuscript_id: Option<String>,
    pub chapter_id: Option<String>,
    pub title: String,
    pub output_path: String,
}

/// 作者预览中的排除报告；该结构只从 preview API 返回，绝不写入站点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderExportExclusion {
    pub target: Option<TargetRef>,
    pub manuscript_id: Option<String>,
    pub chapter_id: Option<String>,
    pub source_path: Option<String>,
    pub reason_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderExportPreview {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ReaderContentPreview>,
    pub schema_version: u32,
    pub plan_digest: String,
    pub content_baseline: String,
    pub included: Vec<ReaderExportIncluded>,
    pub exclusions: Vec<ReaderExportExclusion>,
}

#[derive(Debug, Clone)]
struct PublicPage {
    title: String,
    output_path: PathBuf,
    body_html: String,
    searchable_text: String,
    kind: String,
    aliases: Vec<String>,
    anchors: Vec<PublicAnchor>,
    empty_content: bool,
}

#[derive(Debug, Clone)]
struct PublicAnchor {
    id: String,
    label: String,
    text: String,
}

#[derive(Debug, Clone)]
struct PublicAttachment {
    id: String,
    display: String,
    source_path: PathBuf,
    output_path: PathBuf,
    bytes: Vec<u8>,
}

struct PreparedExport {
    preview: ReaderExportPreview,
    pages: Vec<PublicPage>,
    attachments: Vec<PublicAttachment>,
    site_title: String,
    world_site: bool,
}

struct ExclusionInput<'a> {
    compiled: &'a CompileResult,
    indexes: &'a BTreeMap<String, ManuscriptIndex>,
    selected_objects: &'a BTreeSet<TargetRef>,
    selected_chapters: &'a BTreeSet<(String, String)>,
    selected_assets: &'a BTreeSet<String>,
    selected_asset_paths: &'a BTreeSet<PathBuf>,
    workspace_paths: &'a [PathBuf],
    project: &'a Project,
}

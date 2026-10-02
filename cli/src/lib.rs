//! wl 命令行库:check / play / graph / timeline / catalog 的实现。
//! 以库形式暴露,供 main.rs 与集成测试共用。
//! `--json` 机器输出契约见 `worldline/spec/agent-protocol.md`。

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use worldline_core::ast::PropertyValue;
use worldline_core::authoring::EntityDraft;
use worldline_core::authoring_intents::AuthoringIntent;
use worldline_core::catalog::TargetRef;
use worldline_core::localization::{LocalizationExchange, LocalizationSelection};
use worldline_core::markdown_import::MarkdownImportRequest;
use worldline_core::project::Project;
use worldline_core::queries::{CatalogQuery, CatalogQueryCursor, CatalogQueryOptions};
use worldline_core::reader_export::ReaderExportSelection;
use worldline_core::{
    compile_path, compile_path_with_options, CompileOptions, CompileResult, Diagnostic,
    LanguageVersion, RelationDirection, RelationDraft, RelationQueryDirection,
    RelationQueryOptions, RelationTypeDraft, Severity,
};
use worldline_runtime::{Output, ReplayBudget, ReplayStatus, ReplayTrace, Story};

/// 单个故事文件的公共参数。
struct FileArgs {
    path: PathBuf,
    json: bool,
    load: Option<PathBuf>,
    save: Option<PathBuf>,
    seed: Option<u64>,
    trace_output: Option<PathBuf>,
    choice_presentation: bool,
    bounded_continue: bool,
    continuation_budget: ReplayBudget,
    language_version: Option<LanguageVersion>,
}

struct ReplayArgs {
    path: PathBuf,
    trace_json: String,
    budget: ReplayBudget,
    json: bool,
    language_version: Option<LanguageVersion>,
}

struct CatalogArgs {
    file: FileArgs,
    tag: Option<String>,
    recursive: bool,
    kind: Option<String>,
}

struct CatalogQueryArgs {
    path: PathBuf,
    query_json: String,
    cursor_json: Option<String>,
    options: CatalogQueryOptions,
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReaderExportOperation {
    Preview,
    Apply,
}

struct ReaderExportArgs {
    path: PathBuf,
    operation: ReaderExportOperation,
    selection_json: String,
    plan_digest: Option<String>,
    output: Option<PathBuf>,
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalizationDirection {
    Export,
    Import,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalizationOperation {
    Preview,
    Apply,
}

struct LocalizationArgs {
    path: PathBuf,
    direction: LocalizationDirection,
    operation: LocalizationOperation,
    selection_json: String,
    plan_digest: Option<String>,
    package: Option<PathBuf>,
    output: Option<PathBuf>,
    json: bool,
}

struct CatalogQueryFailure<'a> {
    code: &'a str,
    message: &'a str,
    result: Option<&'a CompileResult>,
    baseline: Option<&'a str>,
    workspace_diagnostics: &'a [Diagnostic],
    exit_code: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthoringIntentOperation {
    Preview,
    Apply,
}

impl AuthoringIntentOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Apply => "apply",
        }
    }
}

struct AuthoringIntentArgs {
    path: PathBuf,
    operation: AuthoringIntentOperation,
    intent_json: String,
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkdownImportOperation {
    Preview,
    Apply,
}

impl MarkdownImportOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Apply => "apply",
        }
    }
}

struct MarkdownImportArgs {
    path: PathBuf,
    source: PathBuf,
    baseline: String,
    operation: MarkdownImportOperation,
    id_map_json: Option<String>,
    namespace: Option<String>,
    plan_digest: Option<String>,
    accept_losses: bool,
    allow_language_upgrade: bool,
    json: bool,
}

struct AuthoringIntentFailure<'a> {
    code: &'a str,
    message: &'a str,
    result: Option<&'a CompileResult>,
    baseline: Option<&'a str>,
    workspace_diagnostics: &'a [Diagnostic],
    exit_code: i32,
}

struct WorkspaceArgs {
    path: PathBuf,
    json: bool,
}

struct MapsArgs {
    path: PathBuf,
    json: bool,
}

struct RelationsArgs {
    offset: usize,
    path: PathBuf,
    target: TargetRef,
    depth: u8,
    direction: RelationQueryDirection,
    relation_type: Option<String>,
    scope_refs: Vec<TargetRef>,
    include_unscoped: bool,
    include_period_children: bool,
    json: bool,
}

struct TopicProjectionArgs {
    path: PathBuf,
    target: TargetRef,
    role_mapping: std::collections::BTreeMap<String, String>,
    offset: usize,
    history_offset: usize,
    depth: u8,
    direction: RelationQueryDirection,
    scope_refs: Vec<TargetRef>,
    include_unscoped: bool,
    include_period_children: bool,
    max_nodes: usize,
    max_edges: usize,
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelationEditOperation {
    Create,
    Update,
    Delete,
}

struct RelationTypeEditArgs {
    path: PathBuf,
    operation: RelationEditOperation,
    id: String,
    display: Option<String>,
    inverse_display: Option<String>,
    clear_inverse_display: bool,
    direction: Option<RelationDirection>,
    from_kind: Option<String>,
    clear_from_kind: bool,
    to_kind: Option<String>,
    clear_to_kind: bool,
    baseline: Option<String>,
    json: bool,
}

struct RelationEditArgs {
    path: PathBuf,
    operation: RelationEditOperation,
    id: String,
    relation_type: Option<String>,
    from: Option<TargetRef>,
    to: Option<TargetRef>,
    description: Option<String>,
    source_note: Option<String>,
    clear_source_note: bool,
    scope_refs: Vec<TargetRef>,
    clear_scope_refs: bool,
    properties: Vec<(String, PropertyValue)>,
    clear_properties: bool,
    baseline: Option<String>,
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromotionOperation {
    Preview,
    Commit,
}

struct PromotionArgs {
    path: PathBuf,
    operation: PromotionOperation,
    source: TargetRef,
    target: TargetRef,
    label: String,
    occurrence: u32,
    relation_id: String,
    relation_type: String,
    description: Option<String>,
    source_note: Option<String>,
    scope_refs: Vec<TargetRef>,
    properties: Vec<(String, PropertyValue)>,
    baseline: Option<String>,
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntityOperation {
    Create,
    Update,
    Delete,
}

struct EntityArgs {
    path: PathBuf,
    operation: EntityOperation,
    id: Option<String>,
    entity_type: Option<String>,
    display: Option<String>,
    description: Option<String>,
    properties: Vec<(String, PropertyValue)>,
    baseline: Option<String>,
    json: bool,
}

struct CompileSnapshot {
    result: CompileResult,
    workspace_diagnostics: Vec<Diagnostic>,
    read_only: bool,
}

impl CompileSnapshot {
    fn plain(result: CompileResult) -> Self {
        Self {
            result,
            workspace_diagnostics: Vec::new(),
            read_only: false,
        }
    }
}

#[path = "lib/authoring_intent.rs"]
mod authoring_intent;
#[path = "lib/catalog.rs"]
mod catalog;
#[path = "lib/catalog_query.rs"]
mod catalog_query;
#[path = "lib/entity.rs"]
mod entity;
#[path = "lib/localization.rs"]
mod localization;
#[path = "lib/markdown_import.rs"]
mod markdown_import;
#[path = "lib/parse_files.rs"]
mod parse_files;
#[path = "lib/parse_relation_edits.rs"]
mod parse_relation_edits;
#[path = "lib/parse_relation_query.rs"]
mod parse_relation_query;
#[path = "lib/play.rs"]
mod play;
#[path = "lib/reader_export.rs"]
mod reader_export;
#[path = "lib/relation_edit.rs"]
mod relation_edit;
#[path = "lib/relation_query.rs"]
mod relation_query;
#[path = "lib/scene.rs"]
mod scene;
#[path = "lib/source_edit.rs"]
mod source_edit;
#[path = "lib/story.rs"]
mod story;
#[path = "lib/support.rs"]
mod support;
#[path = "lib/workspace.rs"]
mod workspace;

/// 返回值 = 进程退出码。
pub fn run(args: &[String], out: &mut impl Write, input: &mut impl BufRead) -> Result<i32, String> {
    let Some(cmd) = args.first() else {
        return Err("缺少子命令".into());
    };
    let rest = &args[1..];
    match cmd.as_str() {
        "scene" => scene::command(rest, out),
        "workspace" => workspace::cmd_workspace(&workspace::parse_workspace_args(rest)?, out),
        "maps" => workspace::cmd_maps(&workspace::parse_maps_args(rest)?, out),
        "relations" => {
            if rest.first().is_some_and(|arg| arg == "promote") {
                relation_edit::cmd_promotion(&parse_relation_edits::parse_promotion_args(rest)?, out)
            } else if rest.first().is_some_and(|arg| arg == "project") {
                relation_query::cmd_topic_projection(&parse_relation_query::parse_topic_projection_args(rest)?, out)
            } else {
                relation_query::cmd_relations(&parse_relation_query::parse_relations_args(rest)?, out)
            }
        }
        "relation" => relation_edit::cmd_relation_edit(&parse_relation_edits::parse_relation_edit_args(rest)?, out),
        "relation-type" | "relation_type" => {
            relation_edit::cmd_relation_type_edit(&parse_relation_edits::parse_relation_type_edit_args(rest)?, out)
        }
        "check" => {
            let f = parse_files::parse_file_args(cmd, rest, false)?;
            story::cmd_check(&f, out)
        }
        "graph" => {
            let f = parse_files::parse_file_args(cmd, rest, false)?;
            story::cmd_graph(&f, out)
        }
        "timeline" => {
            let f = parse_files::parse_file_args(cmd, rest, false)?;
            story::cmd_timeline(&f, out)
        }
        "catalog" => catalog::cmd_catalog(&catalog::parse_catalog_args(rest)?, out),
        "catalog-query" => catalog_query::cmd_catalog_query(&catalog_query::parse_catalog_query_args(rest)?, out),
        "source-edit" => source_edit::command(rest, out),
        "schema-index" => source_edit::schema_index(rest, out),
        "schema-preview" => source_edit::schema_command(rest, out, false),
        "schema-apply" => source_edit::schema_command(rest, out, true),
        "reader-export" => reader_export::cmd_reader_export(&reader_export::parse_reader_export_args(rest)?, out),
        "localization" => localization::cmd_localization(&localization::parse_localization_args(rest)?, out),
        "markdown" => markdown_import::cmd_markdown_import(&markdown_import::parse_markdown_import_args(rest)?, out),
        "authoring-intent" => {
            authoring_intent::cmd_authoring_intent(&authoring_intent::parse_authoring_intent_args(rest)?, out)
        }
        "entity" => entity::cmd_entity(&entity::parse_entity_args(rest)?, out),
        "play" => {
            let f = parse_files::parse_file_args(cmd, rest, true)?;
            play::cmd_play(&f, out, input)
        }
        "replay" => play::cmd_replay(&play::parse_replay_args(rest)?, out),
        other => Err(format!(
                "未知子命令 `{other}`(可用:workspace / maps / scene / relations / relation / relation-type / check / play / replay / graph / timeline / catalog / catalog-query / reader-export / localization / markdown / authoring-intent / source-edit / schema-index / schema-preview / schema-apply / entity)"
        )),
    }
}

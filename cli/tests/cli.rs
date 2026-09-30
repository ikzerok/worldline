//! wl CLI 集成测试:直接驱动 lib 层的 run(),不启动进程。

#[path = "cli/authoring_intent.rs"]
mod authoring_intent;
#[path = "cli/catalog.rs"]
mod catalog;
#[path = "cli/catalog_query.rs"]
mod catalog_query;
#[path = "cli/common.rs"]
mod common;
#[path = "cli/entity.rs"]
mod entity;
#[path = "cli/localization.rs"]
mod localization;
#[path = "cli/markdown.rs"]
mod markdown;
#[path = "cli/protocol.rs"]
mod protocol;
#[path = "cli/reader_export.rs"]
mod reader_export;
#[path = "cli/relations.rs"]
mod relations;
#[path = "cli/story.rs"]
mod story;
#[path = "cli/workspace.rs"]
mod workspace;

#[path = "cli/language_111.rs"]
mod language_111;

#[path = "cli/source_edit.rs"]
mod source_edit;

//! 显式1.12持续资料约束。所有消费者复用此模型与校验器。
mod edit;
mod model;
pub(crate) mod parse;
mod validate;
pub use edit::{
    SchemaEditPreview, SchemaFieldChange, SchemaIncompleteReason, SchemaInstanceImpact,
};
pub use model::{SchemaBinding, SchemaDecl, SchemaField, SchemaIndex, SchemaInstance, SchemaType};
pub(crate) use validate::add_binding_references;
pub use validate::validate;

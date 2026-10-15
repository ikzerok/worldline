//! 有类型条件的有界投影与精确往返；唯一真源仍是既有 DSL。
mod candidates;
mod draft;
mod model;
mod projection;
mod protocol;
mod source;
mod transaction;
mod tree;
pub use model::*;
pub use protocol::{parse_condition_authoring_request, parse_condition_selector};

//! 同一次编译驱动的单对象资料填写与原子事务。
mod model;
mod projection;
mod protocol;
mod source;
mod transaction;
pub use model::*;
pub use protocol::{parse_schema_authoring_request, parse_schema_authoring_target};

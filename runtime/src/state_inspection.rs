//! 真实Story的有界只读投影；不使用JSON推断类型或制造观察。
mod capture;
mod model;
mod query;
mod search;
mod stamp_wire;
#[cfg(test)]
mod tests;
pub(crate) use capture::InspectionHistory;
pub use model::*;

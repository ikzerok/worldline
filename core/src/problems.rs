//! 只读、同快照工程问题；完整边界见 spec/problems.md。
mod build;
mod context;
mod coverage;
mod location;
mod observation;
mod query;
mod types;
mod version;
pub use types::*;

pub(crate) fn digest(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub(crate) fn clipped(text: &str, limit: usize) -> (String, bool) {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), end < text.len())
}

#[cfg(test)]
mod budget_tests;
#[cfg(test)]
mod compat_tests;
#[cfg(test)]
mod context_performance_tests;
#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
thread_local! {
    pub(crate) static COMPILE_RUNS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

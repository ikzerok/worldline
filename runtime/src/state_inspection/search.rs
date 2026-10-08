//! 对当前长值进行流式Unicode小写匹配，只分配查询词大小的辅助存储。
use super::capture::BorrowedValue;
use crate::Value;
pub(super) struct FoldedSearch {
    pattern: Vec<char>,
    fallback: Vec<usize>,
}
impl FoldedSearch {
    pub fn new(text: &str) -> Self {
        let pattern: Vec<_> = text.chars().flat_map(char::to_lowercase).collect();
        let mut fallback = vec![0; pattern.len()];
        let mut matched = 0;
        for i in 1..pattern.len() {
            while matched > 0 && pattern[i] != pattern[matched] {
                matched = fallback[matched - 1];
            }
            if pattern[i] == pattern[matched] {
                matched += 1;
            }
            fallback[i] = matched;
        }
        Self { pattern, fallback }
    }
    pub fn is_empty(&self) -> bool {
        self.pattern.is_empty()
    }
    pub fn contains(&self, text: &str) -> bool {
        if self.is_empty() {
            return true;
        }
        let mut matched = 0;
        for ch in text.chars().flat_map(char::to_lowercase) {
            while matched > 0 && ch != self.pattern[matched] {
                matched = self.fallback[matched - 1];
            }
            if ch == self.pattern[matched] {
                matched += 1;
            }
            if matched == self.pattern.len() {
                return true;
            }
        }
        false
    }
}
impl BorrowedValue<'_> {
    pub(super) fn matches(&self, search: &FoldedSearch) -> bool {
        match self {
            Self::Value(Value::Str(text) | Value::Tag(text) | Value::StateRef(text)) => {
                search.contains(text)
            }
            Self::Value(Value::TagSet(tags)) => tags.iter().any(|tag| search.contains(tag)),
            Self::Tags(tags) => tags.iter().any(|tag| search.contains(tag)),
            Self::Value(value) => search.contains(&value.display()),
            Self::Uninitialized => false,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspection_search_handles_unicode_overlap_and_long_value_tails() {
        assert!(FoldedSearch::new("aaba").contains("aaaaba"));
        assert!(FoldedSearch::new("İ甲").contains("前缀i\u{307}甲后缀"));
        assert!(!FoldedSearch::new("xyz").contains("xyxyxy"));
        assert!(FoldedSearch::new("TAIL").contains(&format!("{}Tail", "前缀".repeat(10000))));
    }
}

//! 显式活动源码集合与归档边界。

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceSelection {
    pub active: Vec<PathBuf>,
    pub archived: Vec<PathBuf>,
}

impl SourceSelection {
    pub fn is_active(&self, path: &Path) -> bool {
        let path = crate::compiler::source_path(path);
        self.active.binary_search(&path).is_ok()
    }

    pub fn is_archived(&self, path: &Path) -> bool {
        let path = crate::compiler::source_path(path);
        self.archived.binary_search(&path).is_ok()
    }

    pub(crate) fn normalize(&mut self) {
        self.active.sort();
        self.active.dedup();
        self.archived.sort();
        self.archived.dedup();
    }
}

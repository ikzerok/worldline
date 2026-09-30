//! 编排树草稿操作。只改展示条目，不接触源码或运行对象。
use super::{ManuscriptDraft, ManuscriptEntryKind};
use std::collections::HashSet;

impl ManuscriptDraft {
    pub fn entry_subtree(&self, id: &str) -> Result<Vec<String>, String> {
        if !self.entries.iter().any(|entry| entry.id == id) {
            return Err("编排项不存在".into());
        }
        let mut selected = HashSet::from([id.to_owned()]);
        loop {
            let before = selected.len();
            for entry in &self.entries {
                if entry
                    .parent_id
                    .as_ref()
                    .is_some_and(|parent| selected.contains(parent))
                {
                    selected.insert(entry.id.clone());
                }
            }
            if selected.len() == before {
                break;
            }
        }
        Ok(self
            .entries
            .iter()
            .filter(|entry| selected.contains(&entry.id))
            .map(|entry| entry.id.clone())
            .collect())
    }

    pub fn move_to_section(&mut self, id: &str, parent: Option<&str>) -> Result<bool, String> {
        let descendants = self.entry_subtree(id)?;
        if let Some(parent) = parent {
            if descendants.iter().any(|descendant| descendant == parent) {
                return Err("不能移动到自身或其下级分节".into());
            }
            if !self
                .entries
                .iter()
                .any(|entry| entry.id == parent && entry.kind == ManuscriptEntryKind::Section)
            {
                return Err("目标必须是存在的分节".into());
            }
        }
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or("编排项不存在")?;
        let parent = parent.map(str::to_owned);
        if entry.parent_id == parent {
            return Ok(false);
        }
        entry.parent_id = parent;
        Ok(true)
    }

    /// 删除此条及所有编排后代；源码不在此 API 的能力范围中。
    pub fn remove_entry_subtree(&mut self, id: &str) -> Result<Vec<String>, String> {
        let removed = self.entry_subtree(id)?;
        self.entries.retain(|entry| !removed.contains(&entry.id));
        Ok(removed)
    }
}

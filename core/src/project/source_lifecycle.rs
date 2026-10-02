use super::*;

impl Project {
    /// 保留旧路径保存基线与删除墓碑，供 journal 和保存后的单步撤销使用。
    pub(crate) fn relocate_source_buffer(&mut self, old: &Path, new: &Path) -> Result<(), String> {
        let document = self.documents.get_mut(old).ok_or("移动源码未载入")?;
        if document.deleted {
            return Err("移动源码已删除".into());
        }
        let text = document.text.clone();
        document.deleted = true;
        self.documents.insert(
            new.to_path_buf(),
            Document {
                text,
                saved: None,
                deleted: false,
            },
        );
        Ok(())
    }
}

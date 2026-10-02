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

impl Project {
    /// 生命周期专用有界基线检查；不改变其它检查点/保存接口的旧契约。
    pub(crate) fn source_lifecycle_disk_baselines_match(&self) -> Result<(), String> {
        let files = crate::source_lifecycle::safety::baseline_inventory(self)?;
        if !self.recovery_conflicts.is_empty() {
            return Err("工程存在未解决的保存事务冲突，不能组织源码".into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        for path in [
            self.root.join(".world"),
            self.root.join(".world/.transactions"),
        ] {
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    if !metadata.is_dir() || crate::file_access::is_link_or_junction(&metadata) {
                        return Err("保存事务父目录必须是非链接目录".into());
                    }
                    if path.ends_with(".transactions")
                        && std::fs::read_dir(&path)
                            .map_err(|error| error.to_string())?
                            .next()
                            .is_some()
                    {
                        return Err("工程存在未解决的保存事务，请重新打开并处理冲突".into());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        let sources = self
            .documents
            .iter()
            .map(|(path, document)| (path, document.saved.as_ref().map(|text| text.as_bytes())));
        let authoring = self
            .authoring_documents
            .iter()
            .map(|(path, document)| (path, document.saved.as_deref()));
        let mut size = 0usize;
        for (path, baseline) in sources.chain(authoring) {
            size = size.saturating_add(baseline.map_or(0, <[u8]>::len));
            if baseline.is_some_and(|bytes| bytes.len() > 64 * 1024 * 1024)
                || size > 256 * 1024 * 1024
            {
                return Err("源码组织保存基线超过单项 64 MiB/总计 256 MiB 预算".into());
            }
            let disk = if files.binary_search(path).is_ok() {
                Some(crate::source_lifecycle::resources::disk_bytes(self, path)?)
            } else {
                None
            };
            if disk.as_deref() != baseline {
                return Err(format!(
                    "{} 已被外部修改或删除，源码组织未提交",
                    path.display()
                ));
            }
        }
        Ok(())
    }
}

use super::*;

impl Project {
    /// 返回最近一次打开时发现的保存事务冲突。
    ///
    /// 冲突文件保留磁盘上的第三方内容，未完成事务和日志继续保留，直到
    /// 用户完成合并；保存与导出会拒绝在此状态下继续覆盖工程。
    pub fn recovery_conflicts(&self) -> &[PathBuf] {
        &self.recovery_conflicts
    }

    /// 返回当前缓冲与磁盘相对于保存基线发生交叉变化的文件。
    ///
    /// 查询只复制缓冲字节，并通过 `file_access` 读取磁盘；不会刷新、修改
    /// 或推进任何工程状态。`.wl` 以 UTF-8 文本缓冲保存，注册展示 JSON
    /// 则直接保留原始字节，因而两类文件都能安全展示缺失和坏 UTF-8 状态。
    pub fn conflict_snapshots(&self) -> Result<Vec<ConflictSnapshot>, String> {
        let disk_files = match crate::file_access::workspace_files(&self.root) {
            Ok(paths) => paths,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(format!("无法读取工作区文件清单：{error}")),
        };
        let mut snapshots = Vec::new();
        for (path, document) in &self.documents {
            if !document.is_dirty() {
                continue;
            }
            let baseline = document.saved.as_ref().map(|text| text.as_bytes().to_vec());
            let local = (!document.deleted).then(|| document.text.as_bytes().to_vec());
            let disk = read_conflict_disk(&self.root, path, &disk_files)?;
            if disk != baseline {
                snapshots.push(ConflictSnapshot {
                    path: path.clone(),
                    baseline,
                    local,
                    disk,
                });
            }
        }
        for (path, document) in &self.authoring_documents {
            if !document.is_dirty() {
                continue;
            }
            let baseline = document.saved.clone();
            let local = (!document.deleted).then(|| document.bytes.clone());
            let disk = read_conflict_disk(&self.root, path, &disk_files)?;
            if disk != baseline {
                snapshots.push(ConflictSnapshot {
                    path: path.clone(),
                    baseline,
                    local,
                    disk,
                });
            }
        }
        Ok(snapshots)
    }
    /// 返回一个已跟踪文件的保存基线和当前缓冲；不读取磁盘、不推进基线。
    pub fn tracked_file_state(&self, path: &Path) -> Option<TrackedFileState> {
        let path = source_path(path);
        if let Some(document) = self.documents.get(&path) {
            return Some(TrackedFileState {
                path,
                baseline: document.saved.as_ref().map(|text| text.as_bytes().to_vec()),
                current: (!document.deleted).then(|| document.text.as_bytes().to_vec()),
                authoring: false,
            });
        }
        self.authoring_documents
            .get(&path)
            .map(|document| TrackedFileState {
                path,
                baseline: document.saved.clone(),
                current: (!document.deleted).then(|| document.bytes.clone()),
                authoring: true,
            })
    }

    /// 只返回相对保存基线发生变化的受控文档；普通附件不进入结构化提案。
    pub fn dirty_tracked_files(&self) -> Vec<TrackedFileState> {
        let mut out = Vec::new();
        for (path, document) in &self.documents {
            if document.is_dirty() {
                if let Some(state) = self.tracked_file_state(path) {
                    out.push(state);
                }
            }
        }
        for (path, document) in &self.authoring_documents {
            if document.is_dirty() {
                if let Some(state) = self.tracked_file_state(path) {
                    out.push(state);
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }
}

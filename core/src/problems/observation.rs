use super::{digest, ProblemsError};
use crate::project::Project;

impl Project {
    /// 无全文读取、无编译的文件可读性观测；不是附件内容或实时权限签名。
    pub fn problems_observation_key(&self) -> Result<String, ProblemsError> {
        let paths = match crate::file_access::workspace_files_limited(&self.root, 10_000) {
            Ok(paths) => paths,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && new_root_missing(&self.root) =>
            {
                Vec::new()
            }
            Err(error) => {
                return Err(ProblemsError::new(
                    "OBSERVATION_UNAVAILABLE",
                    format!("无法观察工程外部文件：{error}"),
                ))
            }
        };
        let mut observations = Vec::new();
        for path in paths {
            if self.documents.contains_key(&path) || self.authoring_documents.contains_key(&path) {
                continue;
            }
            let relative = super::location::relative(self, &path).ok_or_else(|| {
                ProblemsError::new("OBSERVATION_UNAVAILABLE", "外部文件路径不在工作区内")
            })?;
            observations.push((relative, crate::file_access::readable(&path)));
        }
        Ok(digest(
            &serde_json::to_vec(&(&self.root, observations, self.recovery_conflicts()))
                .expect("文件观测可序列化"),
        ))
    }
}
fn new_root_missing(root: &std::path::Path) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::symlink_metadata(root).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = root;
        true
    }
}

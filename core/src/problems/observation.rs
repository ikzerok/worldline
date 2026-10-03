use super::{digest, ProblemsError};
use crate::project::Project;

impl Project {
    /// 无全文读取、无编译的文件可读性观测；不是附件内容或实时权限签名。
    pub fn problems_observation_key(&self) -> Result<String, ProblemsError> {
        #[cfg(not(target_arch = "wasm32"))]
        if std::fs::symlink_metadata(&self.root)
            .is_ok_and(|metadata| crate::file_access::is_link_or_junction(&metadata))
            || crate::compiler::source_path(&self.root) != self.root
        {
            return Err(ProblemsError::new(
                "OBSERVATION_UNAVAILABLE",
                "工作区根目录身份已改变或变为链接",
            ));
        }
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
        let bytes = serde_json::to_vec(&(&self.root, observations, self.recovery_conflicts()))
            .map_err(|_| {
                ProblemsError::new(
                    "OBSERVATION_UNAVAILABLE",
                    "工作区路径不能编码为可传输的文件观测",
                )
            })?;
        Ok(digest(&bytes))
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

/// 已编译的资源可读性必须仍与本次观测一致，不借旧内容快照掩盖附件变化。
pub(crate) fn assets_current(project: &Project, content: &crate::CompileResult) -> bool {
    content.analysis.catalog.assets.values().all(|asset| {
        let path = crate::catalog::resolved_asset(&asset.file, &asset.path);
        let available = !std::path::Path::new(&asset.path).is_absolute()
            && path.starts_with(&project.root)
            && crate::file_access::readable(&path)
            && crate::catalog::supported_extension(&asset.kind, &path);
        available == asset.available && path.to_string_lossy() == asset.resolved_path
    })
}

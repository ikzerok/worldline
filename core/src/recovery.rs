//! 未完成保存的显式救援；输出原稿供人工合并，不伪装成已恢复的工程。
#[cfg(not(target_arch = "wasm32"))]
use crate::project::Project;
use serde::Serialize;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct RecoveryDraft {
    pub transaction: String,
    pub path: PathBuf,
    pub before_hash: Option<String>,
    pub current_hash: Option<String>,
    pub after_hash: Option<String>,
    /// None 表示事务原本请求删除；Some(empty) 表示保存空文件。
    #[serde(skip)]
    pub bytes: Option<Vec<u8>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Project {
    /// 读取经过边界与 hash 校验的事务原稿，不覆盖第三方内容或清理日志。
    pub fn recovery_drafts(&self) -> Result<Vec<RecoveryDraft>, String> {
        crate::storage::recovery_drafts(&self.root)
    }

    /// 显式导出救援资料，保留删除意图及原始字节；不会切换当前工程。
    pub fn export_recovery_drafts(&self, destination: &Path) -> Result<(), String> {
        let destination = crate::compiler::source_path(destination);
        if destination.starts_with(&self.root) {
            return Err("救援目录必须在当前工作区外".into());
        }
        let drafts = self.recovery_drafts()?;
        if drafts.is_empty() {
            return Err("没有可导出的保存事务原稿".into());
        }
        // create_dir 拒绝已有目标，避免覆盖任何用户文件。
        std::fs::create_dir(&destination)
            .map_err(|error| format!("无法建立新的救援目录：{error}"))?;
        let mut records = Vec::new();
        for draft in drafts {
            let payload = if let Some(bytes) = &draft.bytes {
                let relative = Path::new("drafts")
                    .join(&draft.transaction)
                    .join(&draft.path);
                let target = destination.join(&relative);
                std::fs::create_dir_all(target.parent().unwrap())
                    .map_err(|error| format!("无法建立救援子目录：{error}"))?;
                std::fs::write(&target, bytes)
                    .map_err(|error| format!("无法写入救援原稿：{error}"))?;
                Some(relative)
            } else {
                None
            };
            records.push(serde_json::json!({
                "transaction": draft.transaction,
                "path": draft.path,
                "before_hash": draft.before_hash,
                "current_hash": draft.current_hash,
                "after_hash": draft.after_hash,
                "operation": if payload.is_some() { "write" } else { "delete" },
                "payload": payload,
            }));
        }
        let manifest = serde_json::to_vec_pretty(&serde_json::json!({
            "kind": "worldline-recovery-drafts",
            "version": 1,
            "files": records,
        }))
        .map_err(|error| error.to_string())?;
        std::fs::write(destination.join("recovery.json"), manifest)
            .map_err(|error| format!("无法写入救援清单：{error}"))
    }
}

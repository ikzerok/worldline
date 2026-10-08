//! 普通外改冲突的全量受保护候选；不保存、不恢复事务、不猜测合并。
mod apply;
mod candidate;
mod capture;
mod protocol;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
mod types;
use super::{AuthoringDocument, Document, Project};
pub use protocol::{ReconciliationInput, ReconciliationInputFile};
use std::path::Path;
pub use types::*;

type Progress<'a> = &'a mut dyn FnMut(ReconciliationStage) -> bool;

fn checkpoint(progress: Progress<'_>, stage: ReconciliationStage) -> Result<(), String> {
    if progress(stage) {
        Ok(())
    } else {
        Err("外部改稿处理已取消，工程和磁盘未修改".into())
    }
}

fn relative(root: &Path, path: &Path) -> Result<std::path::PathBuf, String> {
    // 在source_path递归解析不存在的父目录之前检查，不只依赖机器适配层。
    path_budget(path.strip_prefix(root).map_err(|_| "外部改稿路径越界")?)?;
    let checked = crate::file_access::within(root, path)?;
    if checked != path {
        return Err("外部改稿文件身份不是规范工作区路径".into());
    }
    let relative = path.strip_prefix(root).map_err(|_| "外部改稿路径越界")?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || relative.to_str().is_none()
    {
        return Err("外部改稿文件须为可传输的工作区内相对路径".into());
    }
    Ok(relative.to_path_buf())
}

fn digest(value: &impl serde::Serialize) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    Ok(crate::problems::digest(&bytes))
}

fn path_budget(path: &Path) -> Result<(), String> {
    if path.as_os_str().len() > 4096 || path.components().take(129).count() > 128 {
        return Err("外部改稿路径超过4096字节或128层预算".into());
    }
    Ok(())
}

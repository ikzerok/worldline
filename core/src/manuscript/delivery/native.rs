//! 目标目录暂存完整字节，再复用已审计的不覆盖原子发布。
use super::*;
use crate::reader_export::rename_new;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

pub fn write_manuscript_markdown_new(
    workspace: &Path,
    destination: &Path,
    report: &ManuscriptDeliveryReport,
    before_publish: &mut dyn FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let markdown = report
        .markdown()
        .filter(|_| report.complete())
        .ok_or("审稿材料不完整，不能交付")?;
    write_private_bytes_new(
        workspace,
        destination,
        markdown.as_bytes(),
        "md",
        before_publish,
    )
}

/// 同一私密材料事务；仅参数化已验证完整字节与预期扩展名。
pub(crate) fn write_private_bytes_new(
    workspace: &Path,
    destination: &Path,
    bytes: &[u8],
    extension: &str,
    before_publish: &mut dyn FnMut() -> Result<(), String>,
) -> Result<(), String> {
    if !destination.is_absolute()
        || destination.extension().and_then(|value| value.to_str()) != Some(extension)
    {
        return Err(format!("目标必须是工作区外新 .{extension} 文件的绝对路径"));
    }
    let parent = destination.parent().ok_or("目标缺少父目录")?;
    if destination
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("目标路径不能包含父目录跳转".into());
    }
    check_directory_chain(parent)?;
    let parent = std::fs::canonicalize(parent).map_err(|error| error.to_string())?;
    let root = crate::compiler::source_path(workspace);
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let destination = parent.join(destination.file_name().ok_or("目标缺少文件名")?);
    if destination.starts_with(root) {
        return Err("作者审稿本必须位于当前工作区外".into());
    }
    missing(&destination)?;
    before_publish()?;
    let (stage, mut file) = create_stage(&parent)?;
    let result = (|| {
        file.write_all(bytes)
            .and_then(|()| file.flush())
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("暂存审稿本失败：{error}"))?;
        drop(file);
        check_directory_chain(&parent)?;
        before_publish()?;
        rename_new(&stage, &destination)
            .map_err(|error| format!("审稿本原子发布失败（不会覆盖已有目标）：{error}"))
    })();
    if let Err(error) = &result {
        if let Err(cleanup) = std::fs::remove_file(&stage) {
            if cleanup.kind() != std::io::ErrorKind::NotFound {
                return Err(format!("{error}；暂存文件清理失败：{cleanup}"));
            }
        }
    }
    result
}
fn missing(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err("目标已存在，请选择新的文件名；没有覆盖".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
fn check_directory_chain(path: &Path) -> Result<(), String> {
    for current in directory_chain(path) {
        let metadata = std::fs::symlink_metadata(current).map_err(|error| error.to_string())?;
        if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
            return Err("审稿本目标目录链不能包含链接、联接或非目录".into());
        }
    }
    Ok(())
}
fn directory_chain(path: &Path) -> Vec<&Path> {
    // 完整祖先保留Windows盘符/UNC/verbatim根；不对裸Prefix做IO。
    // 仍从根向叶核验，先拒绝父级链接/联接，再观察其下的目录。
    let mut chain: Vec<_> = path.ancestors().collect();
    chain.reverse();
    chain
}
fn create_stage(parent: &Path) -> Result<(PathBuf, std::fs::File), String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    for _ in 0..100 {
        let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = parent.join(format!(
            ".worldline-manuscript-{}-{next}.tmp",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("无法建立审稿本暂存文件：{error}")),
        }
    }
    Err("不能分配唯一审稿本暂存文件".into())
}

#[cfg(test)]
mod tests;

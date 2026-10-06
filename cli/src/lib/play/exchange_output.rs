//! 仅显式有界交换输出；普通trace-output继续使用原覆盖契约。
use std::{
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

pub(super) fn write(target: &Path, bytes: &[u8]) -> Result<(), String> {
    write_with(target, bytes, |file, bytes| {
        file.write_all(bytes).and_then(|()| file.sync_all())
    })
}

fn write_with(
    target: &Path,
    bytes: &[u8],
    writer: impl FnOnce(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
) -> Result<(), String> {
    if target.file_name().is_none() {
        return Err("路径交换目标必须是文件".into());
    }
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut opened = None;
    for _ in 0..32 {
        let temporary = parent.join(format!(
            ".wl-trace-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => {
                opened = Some((temporary, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("无法创建路径交换暂存文件：{error}")),
        }
    }
    let (temporary, mut file) = opened.ok_or("无法创建唯一的路径交换暂存文件")?;
    let written = writer(&mut file, bytes);
    drop(file);
    // hard_link发布一个新名字，不论平台均不替换已有目标（含检查后的竞态创建）。
    let result = written.and_then(|()| std::fs::hard_link(&temporary, target));
    if let Err(error) = result {
        let cleanup = std::fs::remove_file(&temporary);
        return Err(match cleanup {
            Ok(()) => format!(
                "路径交换发布失败（目标须不存在，且文件系统须支持硬链接），原目标保留：{error}"
            ),
            Err(cleanup) => {
                format!("路径交换发布失败，原目标保留：{error}；暂存清理失败：{cleanup}")
            }
        });
    }
    if let Err(error) = std::fs::remove_file(&temporary) {
        eprintln!(
            "路径交换已发布；暂存文件 {} 清理失败：{error}",
            temporary.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "wl-exchange-delivery-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }
    #[test]
    fn racing_publishers_never_replace_the_winner() {
        let root = directory("race");
        let target = root.join("route.json");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = [b"first".as_slice(), b"second".as_slice()]
            .into_iter()
            .map(|bytes| {
                let target = target.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    (bytes, write(&target, bytes))
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(
            results.iter().filter(|(_, result)| result.is_ok()).count(),
            1
        );
        let winner = results.iter().find(|(_, result)| result.is_ok()).unwrap().0;
        assert_eq!(std::fs::read(&target).unwrap(), winner);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn partial_staging_write_failure_cleans_up_and_keeps_existing_target() {
        let root = directory("write-failure");
        let target = root.join("route.json");
        std::fs::write(&target, b"keep").unwrap();
        let result = write_with(&target, b"new bytes", |file, bytes| {
            file.write_all(&bytes[..1])?;
            Err(std::io::Error::other("injected write failure"))
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}

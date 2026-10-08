use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn temporary_directory() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "manuscript-native-root-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn native_directory_chain_checks_complete_real_and_canonical_roots() {
    let directory = temporary_directory();
    let nested = directory.join("parent").join("child");
    std::fs::create_dir_all(&nested).unwrap();
    let canonical = std::fs::canonicalize(&nested).unwrap();
    for path in [&nested, &canonical] {
        let chain = directory_chain(path);
        let root = *chain.first().unwrap();
        assert!(root.is_absolute());
        assert!(root.parent().is_none());
        assert_eq!(chain.last().copied(), Some(path.as_path()));
        assert!(chain.iter().all(|part| part.is_absolute()));
        check_directory_chain(root).unwrap();
        check_directory_chain(path).unwrap();
    }
    #[cfg(windows)]
    assert!(matches!(
        canonical.components().next(),
        Some(Component::Prefix(prefix)) if prefix.kind().is_verbatim()
    ));
    let file = nested.join("ordinary-file");
    std::fs::write(&file, b"must stay unchanged").unwrap();
    assert!(check_directory_chain(&file).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), b"must stay unchanged");
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_directory_chain_keeps_drive_unc_and_verbatim_roots_complete() {
    // 仅检查Windows自身的路径拆分；不连接虚构UNC共享或模拟网络权限。
    for (path, root) in [
        (r"C:\parent\child", r"C:\"),
        (r"\\?\C:\parent\child", r"\\?\C:\"),
        (r"\\server\share\parent\child", r"\\server\share\"),
        (
            r"\\?\UNC\server\share\parent\child",
            r"\\?\UNC\server\share\",
        ),
    ] {
        let path = Path::new(path);
        let chain = directory_chain(path);
        assert_eq!(chain.first().copied(), Some(Path::new(root)));
        assert_eq!(chain.len(), 3);
        assert_eq!(chain.last().copied(), Some(path));
        assert!(chain.iter().all(|part| part.is_absolute()));
        for pair in chain.windows(2) {
            assert_eq!(pair[1].parent(), Some(pair[0]));
        }
    }
    for root in [
        r"C:\",
        r"\\?\C:\",
        r"\\server\share",
        r"\\?\UNC\server\share",
    ] {
        let root = Path::new(root);
        assert_eq!(directory_chain(root), vec![root]);
    }
    assert!(!Path::new(r"C:relative\review.md").is_absolute());
}

#[cfg(unix)]
#[test]
fn directory_chain_still_rejects_a_linked_parent() {
    let directory = temporary_directory();
    let target = directory.join("actual");
    std::fs::create_dir_all(target.join("child")).unwrap();
    let alias = directory.join("alias");
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    assert!(check_directory_chain(&alias.join("child")).is_err());
    check_directory_chain(&target.join("child")).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

//! Windows 保护句柄的实际系统回归；不模拟所有并发 reparse 变更。
use super::{open, verify_final_path};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    home: PathBuf,
    root: PathBuf,
    outside: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let home = std::env::temp_dir().join(format!(
            "worldline-protected-open-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(home.join("workspace/nested/deeper")).unwrap();
        fs::create_dir_all(home.join("outside")).unwrap();
        Self {
            root: home.join("workspace").canonicalize().unwrap(),
            outside: home.join("outside").canonicalize().unwrap(),
            home,
        }
    }
    fn source(&self) -> PathBuf {
        let path = self.root.join("nested/deeper/资源.txt");
        fs::write(&path, b"inside resource").unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // 显式移除本测试建立的 junction 本身，不依赖递归删除是否跟随 reparse。
        let junction = self.root.join("junction");
        if fs::symlink_metadata(&junction).is_ok() {
            let _ = fs::remove_dir(&junction);
        }
        let _ = fs::remove_dir_all(&self.home);
    }
}

#[test]
fn normal_nested_file_opens_and_reads_its_original_bytes() {
    let fixture = Fixture::new();
    let path = fixture.source();
    let mut protected = open(&fixture.root, &path, 1024).unwrap();
    let mut bytes = Vec::new();
    protected.file.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"inside resource");
}

#[test]
fn held_ancestors_deny_rename_delete_access_and_leaf_write_until_drop() {
    let fixture = Fixture::new();
    let path = fixture.source();
    let parent = path.parent().unwrap();
    let renamed = fixture.root.join("nested/renamed");
    let delete_handle = || {
        OpenOptions::new()
            .access_mode(0x0001_0000) // DELETE，不执行删除；直接验证所需权限的共享冲突。
            .share_mode(0x0001 | 0x0002 | 0x0004)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(parent)
    };
    // 先证明夹具本来具有这些操作权限，排除偶然 ACL 拒绝导致假阳性。
    drop(delete_handle().expect("fixture must allow an unpinned DELETE handle"));
    let protected = open(&fixture.root, &path, 1024).unwrap();
    assert!(fs::rename(parent, &renamed).is_err());
    assert!(
        delete_handle().is_err(),
        "held directory must deny DELETE access"
    );
    assert!(fs::remove_file(&path).is_err());
    assert!(OpenOptions::new().write(true).open(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"inside resource");
    drop(protected);
    drop(delete_handle().expect("DELETE access must resume after protected handles close"));
    drop(OpenOptions::new().write(true).open(&path).unwrap());
    fs::rename(parent, &renamed).expect("parent rename must resume after handles close");
}

#[test]
fn existing_test_owned_junction_is_rejected_before_any_content_read() {
    let fixture = Fixture::new();
    fs::write(
        fixture.outside.join("canary.txt"),
        b"synthetic outside canary",
    )
    .unwrap();
    let junction = fixture.root.join("junction");
    let output = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(fixture.home.join("workspace/junction"))
        .arg(fixture.home.join("outside"))
        .output()
        .expect("Windows regression requires cmd.exe and directory-junction support");
    assert!(
        output.status.success(),
        "junction test capability unavailable; this test was NOT validated: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        open(&fixture.root, &junction.join("canary.txt"), 1024).is_err(),
        "protected opener must never return an outside-content handle"
    );
}

#[test]
fn final_handle_path_rejects_outside_file_against_inside_expectation_before_read() {
    let fixture = Fixture::new();
    let outside = fixture.outside.join("canary.txt");
    fs::write(&outside, b"synthetic outside canary").unwrap();
    let handle = File::open(&outside).unwrap();
    // 只取得已授权测试夹具的句柄与最终路径，不调用任何文件内容读取。
    assert!(verify_final_path(&handle, &fixture.root.join("canary.txt")).is_err());
    verify_final_path(&handle, &outside).unwrap();
}

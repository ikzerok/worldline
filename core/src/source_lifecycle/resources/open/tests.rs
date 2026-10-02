//! Windows 保护句柄的实际系统回归；不模拟所有并发 reparse 变更。
use super::{open, open_before_leaf, verify_final_path};
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
        for name in ["junction", "race"] {
            let junction = self.root.join(name);
            if fs::symlink_metadata(&junction).is_ok() {
                let _ = fs::remove_dir(&junction);
            }
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
fn held_ancestors_allow_independent_sibling_publication() {
    let fixture = Fixture::new();
    let path = fixture.source();
    let staging = fixture.home.join("publish-staging");
    let destination = fixture.home.join("published");
    // 先分配暂存目录，使持锁阶段只检查独立文件写入和生产同款 no-replace 发布。
    fs::create_dir(&staging).unwrap();
    let mut protected = open(&fixture.root, &path, 1024).unwrap();
    fs::write(staging.join("index.html"), b"independent publication").unwrap();
    // home 既是资源的已持有祖先，也是独立发布的直接目标父目录；Temp 同样被持有。
    rename_no_replace(&staging, &destination)
        .expect("protected resource ancestors must not block independent sibling publication");
    assert!(!staging.exists());
    assert_eq!(
        fs::read(destination.join("index.html")).unwrap(),
        b"independent publication"
    );
    let mut resource = Vec::new();
    protected.file.read_to_end(&mut resource).unwrap();
    assert_eq!(resource, b"inside resource");
}

fn rename_no_replace(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileW(source: *const u16, destination: *const u16) -> i32;
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: 独立测试夹具生成的路径无内嵌 NUL，两份 UTF-16 缓冲在调用期间存活且已终止。
    if unsafe { MoveFileW(source.as_ptr(), destination.as_ptr()) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
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
        .arg(fixture.home.join("workspace").join("junction"))
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

#[test]
fn junction_added_after_parent_checks_is_rejected_before_any_content_read() {
    let fixture = Fixture::new();
    let race = fixture.root.join("race");
    fs::create_dir(&race).unwrap();
    fs::write(
        fixture.outside.join("canary.txt"),
        b"synthetic outside canary",
    )
    .unwrap();
    let mut redirected = false;
    let result = open_before_leaf(&fixture.root, &race.join("canary.txt"), 1024, || {
        set_junction(&race, &fixture.outside)
            .expect("mid-open junction mutation capability unavailable; regression NOT validated");
        assert!(crate::file_access::is_link_or_junction(
            &fs::symlink_metadata(&race).unwrap()
        ));
        redirected = true;
    });
    assert!(
        redirected,
        "the real reparse mutation must run after parent checks"
    );
    let error = result
        .err()
        .expect("redirected outside handle must not escape the protected opener");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("最终解析路径变化"), "{error}");
    // open_before_leaf 不读取内容，且没有向调用方返回可读取的外部叶句柄。
}

fn set_junction(directory: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn DeviceIoControl(
            device: *mut std::ffi::c_void,
            code: u32,
            input: *const std::ffi::c_void,
            input_size: u32,
            output: *mut std::ffi::c_void,
            output_size: u32,
            returned: *mut u32,
            overlapped: *mut std::ffi::c_void,
        ) -> i32;
    }
    let target = target.to_str().expect("test-owned target must be Unicode");
    let target = target.strip_prefix(r"\\?\").unwrap_or(target);
    let substitute: Vec<u16> = format!(r"\??\{target}").encode_utf16().collect();
    let print: Vec<u16> = target.encode_utf16().collect();
    let sub_bytes = u16::try_from(substitute.len() * 2).unwrap();
    let print_bytes = u16::try_from(print.len() * 2).unwrap();
    // REPARSE_DATA_BUFFER 的 mount-point 分支：公共头 8 字节，偏移/长度 8 字节，UTF-16 路径。
    let data_len = 8u16
        .checked_add(sub_bytes)
        .unwrap()
        .checked_add(print_bytes)
        .unwrap()
        .checked_add(4)
        .unwrap();
    let mut buffer = Vec::new();
    buffer.extend_from_slice(&0xA000_0003u32.to_le_bytes()); // IO_REPARSE_TAG_MOUNT_POINT
    for value in [data_len, 0, 0, sub_bytes, sub_bytes + 2, print_bytes] {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    for unit in substitute
        .into_iter()
        .chain(Some(0))
        .chain(print)
        .chain(Some(0))
    {
        buffer.extend_from_slice(&unit.to_le_bytes());
    }
    assert!(
        buffer.len() <= 16 * 1024,
        "fixture exceeds reparse buffer limit"
    );
    let handle = OpenOptions::new()
        .write(true)
        .share_mode(0x0001 | 0x0002 | 0x0004)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(directory)?;
    let mut returned = 0;
    // SAFETY: 测试独占目录的有效句柄；有界 buffer 在同步调用期间存活，输出为空且长度为 0。
    let result = unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            0x0009_00A4,
            buffer.as_ptr().cast(),
            buffer.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

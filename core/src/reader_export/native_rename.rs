use std::io;
use std::path::Path;

/// 操作系统在最后一次原子操作中保证目标不存在，拒绝检查后的竞争覆盖。
pub(super) fn rename_new(source: &Path, destination: &Path) -> io::Result<()> {
    platform_rename(source, destination)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn platform_rename(source: &Path, destination: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "源路径包含空字节"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "目标路径包含空字节"))?;
    #[cfg(target_os = "linux")]
    unsafe extern "C" {
        fn renameat2(
            oldfd: i32,
            oldpath: *const std::ffi::c_char,
            newfd: i32,
            newpath: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    #[cfg(target_os = "macos")]
    unsafe extern "C" {
        fn renamex_np(
            oldpath: *const std::ffi::c_char,
            newpath: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    // SAFETY: 两个 CString 在调用期间存活并以 NUL 结尾；系统不保存指针。
    // Linux AT_FDCWD=-100、RENAME_NOREPLACE=1；macOS RENAME_EXCL=4。
    let result = unsafe {
        #[cfg(target_os = "linux")]
        {
            renameat2(-100, source.as_ptr(), -100, destination.as_ptr(), 1)
        }
        #[cfg(target_os = "macos")]
        {
            renamex_np(source.as_ptr(), destination.as_ptr(), 4)
        }
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "windows")]
fn platform_rename(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    fn wide(path: &Path) -> io::Result<Vec<u16>> {
        let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "路径包含空字节",
            ));
        }
        value.push(0);
        Ok(value)
    }
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileW(source: *const u16, destination: *const u16) -> i32;
    }
    let source = wide(source)?;
    let destination = wide(destination)?;
    // SAFETY: 两个向量是存活且以 NUL 结尾的 UTF-16 路径，API 不保留指针。
    let result = unsafe { MoveFileW(source.as_ptr(), destination.as_ptr()) };
    if result != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_rename(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "当前系统不支持保证目标不被覆盖的原子发布",
    ))
}

//! 工作区资源在读取前以不跟随链接的组件句柄固定；不先打开后判断越界。
use std::fs::File;
use std::io;
#[cfg(unix)]
use std::path::Component;
use std::path::Path;

pub(super) struct PinnedFile {
    pub file: File,
    #[allow(dead_code)] // Windows 目录句柄在全部读取与复核期间固定父路径。
    directories: Vec<File>,
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(unix)]
pub(super) fn open(root: &Path, path: &Path, max_bytes: u64) -> io::Result<PinnedFile> {
    use std::os::unix::fs::OpenOptionsExt;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| invalid("素材越出工作区"))?;
    if !root.is_absolute()
        || relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid("素材路径必须是规范工作区文件路径"));
    }
    let mut directories = vec![std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open("/")?];
    let all_directories = root
        .components()
        .chain(relative.parent().into_iter().flat_map(Path::components));
    for component in all_directories {
        match component {
            Component::RootDir => continue,
            Component::Normal(name) => {
                let parent = directories.last().ok_or_else(|| invalid("目录句柄缺失"))?;
                directories.push(open_at(
                    parent,
                    name,
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )?);
            }
            _ => return Err(invalid("素材父目录不是规范路径")),
        }
    }
    let parent = directories
        .last()
        .ok_or_else(|| invalid("素材父目录缺失"))?;
    let name = path.file_name().ok_or_else(|| invalid("素材文件名缺失"))?;
    let before = stat_at(parent, name)?;
    if before.st_mode & libc::S_IFMT != libc::S_IFREG
        || before.st_size < 0
        || before.st_size as u64 > max_bytes
    {
        return Err(invalid("素材必须是 64 MiB 内的非链接普通文件"));
    }
    let file = open_at(
        parent,
        name,
        libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
    )?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() > max_bytes || !same_identity(&opened, &before) {
        return Err(invalid("素材在打开时改变了类型、大小或文件身份"));
    }
    Ok(PinnedFile { file, directories })
}

#[cfg(unix)]
#[allow(clippy::unnecessary_cast)] // dev_t/ino_t 在 Linux/macOS/BSD 的宽度和有符号性不同。
fn same_identity(metadata: &std::fs::Metadata, stat: &libc::stat) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.dev() == stat.st_dev as u64 && metadata.ino() == stat.st_ino as u64
}

#[cfg(unix)]
fn open_at(parent: &File, name: &std::ffi::OsStr, flags: i32) -> io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(name.as_bytes()).map_err(|_| invalid("路径含 NUL"))?;
    // SAFETY: parent 是仍存活的目录 File；CString 在调用中存活，flags 不含 O_CREAT。
    let descriptor = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: 成功 openat 返回此函数独占的新描述符，立刻交给 File 管理且仅接管一次。
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(unix)]
fn stat_at(parent: &File, name: &std::ffi::OsStr) -> io::Result<libc::stat> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(name.as_bytes()).map_err(|_| invalid("路径含 NUL"))?;
    let mut result = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: 有效目录描述符和存活 CString；result 提供完整且正确对齐的 stat 写入空间。
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            result.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstatat 成功已初始化完整 stat 结构。
    Ok(unsafe { result.assume_init() })
}

#[cfg(windows)]
pub(super) fn open(root: &Path, path: &Path, max_bytes: u64) -> io::Result<PinnedFile> {
    open_before_leaf(root, path, max_bytes, || {})
}

#[cfg(windows)]
fn open_before_leaf(
    root: &Path,
    path: &Path,
    max_bytes: u64,
    before_leaf: impl FnOnce(),
) -> io::Result<PinnedFile> {
    use std::os::windows::fs::OpenOptionsExt;
    if !path.starts_with(root) || path == root || !path.is_absolute() {
        return Err(invalid("素材越出工作区"));
    }
    let mut ancestors: Vec<_> = path
        .parent()
        .ok_or_else(|| invalid("素材父目录缺失"))?
        .ancestors()
        .collect();
    ancestors.reverse();
    let mut directories = Vec::new();
    for directory in ancestors {
        // BACKUP_SEMANTICS | OPEN_REPARSE_POINT；允许独立子目录发布所需的写访问，
        // 但不共享 DELETE，阻止祖先删除/改名；重定向防线是下方读取前最终句柄路径校验。
        // 保留普通读取访问：access_mode(FILE_READ_ATTRIBUTES) 会覆盖 read(true)，
        // 而属性专用访问不受 CreateFile 共享约束，不能据此证明父目录已固定。
        let handle = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x0001 | 0x0002)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(directory)?;
        let metadata = handle.metadata()?;
        if !metadata.is_dir() || crate::file_access::is_link_or_junction(&metadata) {
            return Err(invalid("素材父目录必须是非 reparse 普通目录"));
        }
        verify_final_path(&handle, directory)?;
        directories.push(handle);
    }
    // 私有同步测试缝隙；生产调用恒为空，用于真实制造检查父目录后、打开叶文件前的竞态。
    before_leaf();
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0x0001)
        .custom_flags(0x0020_0000)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || crate::file_access::is_link_or_junction(&metadata)
        || metadata.len() > max_bytes
    {
        return Err(invalid("素材必须是 64 MiB 内的非 reparse 普通文件"));
    }
    // 目录可以被并发设为 reparse；读取字节前确认已打开叶句柄没有被重定向到外部。
    verify_final_path(&file, path)?;
    Ok(PinnedFile { file, directories })
}

#[cfg(windows)]
fn verify_final_path(file: &File, expected: &Path) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFinalPathNameByHandleW(
            handle: *mut std::ffi::c_void,
            buffer: *mut u16,
            size: u32,
            flags: u32,
        ) -> u32;
    }
    let mut buffer = vec![0u16; 32768];
    // SAFETY: 存活 File 的有效句柄，buffer 是声明长度的可写 u16 数组；flags=规范 DOS 最终路径。
    let size = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    } as usize;
    if size == 0 {
        return Err(io::Error::last_os_error());
    }
    if size >= buffer.len() {
        return Err(invalid("素材最终路径超过 Windows 路径预算"));
    }
    let actual =
        String::from_utf16(&buffer[..size]).map_err(|_| invalid("素材最终路径不是有效 Unicode"))?;
    fn normalized(path: &str) -> String {
        let path = path.replace('/', "\\");
        let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else {
            path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
        };
        path.trim_end_matches('\\').to_lowercase()
    }
    if normalized(&actual) != normalized(&expected.to_string_lossy()) {
        return Err(invalid("素材句柄最终解析路径变化；未读取文件内容"));
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub(super) fn open(_root: &Path, _path: &Path, _max_bytes: u64) -> io::Result<PinnedFile> {
    Err(invalid("当前原生平台不支持固定非链接资源路径"))
}

#[cfg(all(test, windows))]
mod tests;

use super::ManuscriptQueryDraft;
use crate::manuscript::WritingBuffer;
use crate::project::Project;
use std::path::{Component, Path, Prefix};

impl Project {
    /// 所有已载入依赖及全部草稿的只读指纹；不访问磁盘或重新编译。
    /// 这是缓存标记，不是安全签名，也不替代任何提交时检查。
    pub fn manuscript_query_key(
        &self,
        buffers: &[WritingBuffer],
        drafts: &[ManuscriptQueryDraft],
    ) -> String {
        self.manuscript_query_key_refs(buffers.iter(), drafts)
    }

    /// UI 每帧可借用缓冲计算 key，无需克隆整份正文。
    pub fn manuscript_query_key_refs<'a>(
        &self,
        buffers: impl IntoIterator<Item = &'a WritingBuffer>,
        drafts: &[ManuscriptQueryDraft],
    ) -> String {
        let mut key = Key::new("worldline-manuscript-query-v1");
        key.path(&self.root);
        key.path(&self.entry);
        key.bytes(self.language_version().as_bytes());
        key.bytes(&self.search_refresh_generation().to_le_bytes());
        key.bytes(self.manuscript_observation_key().as_bytes());
        for (path, document) in &self.documents {
            key.bytes(b"source");
            key.path(path);
            key.bytes(&[u8::from(document.is_deleted())]);
            key.bytes(document.text.as_bytes());
        }
        for (path, document) in &self.authoring_documents {
            key.bytes(b"authoring");
            key.path(path);
            key.bytes(&[
                u8::from(document.is_deleted()),
                u8::from(document.is_read_only()),
            ]);
            key.bytes(document.bytes());
        }
        key.bytes(b"diagnostics");
        key.bytes(&serde_json::to_vec(self.authoring_diagnostics()).expect("诊断可序列化"));
        key.bytes(b"source-selection");
        if let Some(selection) = self.source_selection() {
            key.bytes(b"explicit");
            for path in &selection.active {
                key.bytes(b"active");
                key.path(path);
            }
            for path in &selection.archived {
                key.bytes(b"archived");
                key.path(path);
            }
        } else {
            key.bytes(b"implicit");
        }
        key.bytes(b"recovery");
        for path in self.recovery_conflicts() {
            key.path(path);
        }
        key.bytes(b"writing-buffers");
        let mut buffers: Vec<_> = buffers.into_iter().collect();
        buffers.sort_by(|a, b| {
            (
                a.path(),
                a.baseline(),
                a.generation(),
                a.original(),
                a.source(),
            )
                .cmp(&(
                    b.path(),
                    b.baseline(),
                    b.generation(),
                    b.original(),
                    b.source(),
                ))
        });
        for buffer in buffers {
            key.bytes(b"buffer");
            key.path(buffer.path());
            key.bytes(buffer.baseline().as_bytes());
            key.bytes(&buffer.generation().to_le_bytes());
            key.bytes(buffer.original().as_bytes());
            key.bytes(buffer.source().as_bytes());
        }
        key.bytes(b"manuscript-drafts");
        let mut drafts: Vec<_> = drafts
            .iter()
            .map(|draft| {
                (
                    &draft.draft.id,
                    &draft.expected_baseline,
                    serde_json::to_vec(draft).expect("书稿草稿可序列化"),
                )
            })
            .collect();
        drafts.sort();
        for (_, _, bytes) in drafts {
            key.bytes(&bytes);
        }
        key.finish()
    }
}

pub(super) struct Key {
    first: u64,
    second: u64,
}
impl Key {
    pub(super) fn new(domain: &str) -> Self {
        let mut value = Self {
            first: 0xcbf29ce484222325,
            second: 0x84222325cbf29ce4,
        };
        value.bytes(domain.as_bytes());
        value
    }
    pub(super) fn bytes(&mut self, bytes: &[u8]) {
        for &byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            self.first = (self.first ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            self.second = (self.second ^ u64::from(byte))
                .wrapping_mul(0x100000001b3)
                .rotate_left(5);
        }
    }
    fn path(&mut self, path: &Path) {
        // 与文档映射的本机路径分量一致；不能为每帧 key 调用 canonicalize。
        // 数量和逐项长度共同分隔路径，保留父目录分量和非 Windows 的字面反斜线。
        self.bytes(&(path.components().count() as u64).to_le_bytes());
        for component in path.components() {
            match component {
                Component::Prefix(prefix) => {
                    self.bytes(b"prefix");
                    self.prefix(prefix.kind());
                }
                Component::RootDir => self.bytes(b"root"),
                Component::CurDir => self.bytes(b"current"),
                Component::ParentDir => self.bytes(b"parent"),
                Component::Normal(value) => {
                    self.bytes(b"normal");
                    self.bytes(value.as_encoded_bytes());
                }
            }
        }
    }
    fn prefix(&mut self, prefix: Prefix<'_>) {
        // UNC 的原始前缀也可能使用混合分隔符；按解析后的种类和原始名字编码。
        match prefix {
            Prefix::Disk(drive) => self.bytes(&[0, drive]),
            Prefix::VerbatimDisk(drive) => self.bytes(&[1, drive]),
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                let kind = if matches!(prefix, Prefix::UNC(..)) {
                    2
                } else {
                    3
                };
                self.bytes(&[kind]);
                self.bytes(server.as_encoded_bytes());
                self.bytes(share.as_encoded_bytes());
            }
            Prefix::DeviceNS(value) | Prefix::Verbatim(value) => {
                let kind = if matches!(prefix, Prefix::DeviceNS(..)) {
                    4
                } else {
                    5
                };
                self.bytes(&[kind]);
                self.bytes(value.as_encoded_bytes());
            }
        }
    }
    pub(super) fn finish(self) -> String {
        format!("{:016x}{:016x}", self.first, self.second)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path_key(path: impl AsRef<Path>) -> String {
        let mut key = Key::new("query-path-test");
        key.path(path.as_ref());
        key.finish()
    }

    #[test]
    fn query_key_path_preserves_component_identity_and_boundaries() {
        for (left, right) in [
            (
                "work/.world/manuscripts/book.json",
                "work//.world/./manuscripts/book.json",
            ),
            ("work/.world/", "work/.world"),
        ] {
            assert_eq!(Path::new(left), Path::new(right));
            assert_ne!(left.as_bytes(), right.as_bytes());
            assert_eq!(path_key(left), path_key(right));
        }
        for (left, right) in [
            ("work/ab/c", "work/a/bc"),
            ("work/a/../b", "work/b"),
            ("work/a", "/work/a"),
            ("", "."),
        ] {
            assert_ne!(Path::new(left), Path::new(right));
            assert_ne!(path_key(left), path_key(right));
        }
    }

    #[test]
    fn query_key_path_obeys_native_separator_semantics() {
        let portable = "work/.world/manuscripts/book.json";
        let native = r"work\.world\manuscripts\book.json";
        if cfg!(windows) {
            assert_eq!(Path::new(portable), Path::new(native));
            assert_eq!(path_key(portable), path_key(native));
        } else {
            assert_ne!(Path::new(portable), Path::new(native));
            assert_ne!(path_key(portable), path_key(native));
        }
    }

    #[test]
    #[cfg(unix)]
    fn query_key_path_keeps_non_utf8_components() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let left = Path::new(OsStr::from_bytes(b"work/\xff/book.json"));
        let equal = Path::new(OsStr::from_bytes(b"work//\xff/./book.json"));
        let distinct = Path::new(OsStr::from_bytes(b"work/\xfe/book.json"));
        assert_eq!(left, equal);
        assert_eq!(path_key(left), path_key(equal));
        assert_ne!(left, distinct);
        assert_ne!(path_key(left), path_key(distinct));
    }

    #[test]
    #[cfg(windows)]
    fn query_key_path_windows_prefixes_keep_identity() {
        for (left, right) in [
            (r"C:\work\.world/book.json", "C:/work/.world/book.json"),
            (
                r"\\server\share\work/book.json",
                "//server/share/work/book.json",
            ),
        ] {
            assert_eq!(Path::new(left), Path::new(right));
            assert_eq!(path_key(left), path_key(right));
        }
        for (left, right) in [
            (r"C:\work\book.json", r"D:\work\book.json"),
            (r"C:work\book.json", r"C:\work\book.json"),
            (r"C:\work\book.json", r"\\?\C:\work\book.json"),
            (r"\\server\share\book.json", r"\\server\other\book.json"),
            (
                r"\\server\share\book.json",
                r"\\?\UNC\server\share\book.json",
            ),
            (r"\\.\device\book.json", r"\\?\device\book.json"),
        ] {
            assert_ne!(Path::new(left), Path::new(right));
            assert_ne!(path_key(left), path_key(right));
        }
    }
}

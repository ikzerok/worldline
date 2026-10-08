use super::ManuscriptQueryDraft;
use crate::manuscript::WritingBuffer;
use crate::project::Project;
use std::path::Path;

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
        self.bytes(path.as_os_str().as_encoded_bytes());
    }
    pub(super) fn finish(self) -> String {
        format!("{:016x}{:016x}", self.first, self.second)
    }
}

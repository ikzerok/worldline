//! 编辑请求的内容基线，与存档使用的运行指纹分离。

use crate::project::Project;
use std::path::Path;

impl Project {
    /// 当前源码、清单及已载入展示文档缓冲的确定性版本标记。
    /// 查询不会刷新或写入工作区；未载入的外部资源不属于该基线。
    ///
    /// 该标记用于拒绝陈旧编辑请求，不是密码学签名，也不能替代保存时的磁盘冲突检查。
    pub fn content_baseline(&self) -> String {
        let mut hash = 0xcbf29ce484222325;
        mix(&mut hash, b"worldline-content-v1");
        mix(&mut hash, relative_path(self, &self.entry).as_bytes());
        for (path, document) in &self.documents {
            mix(&mut hash, b"source");
            mix(&mut hash, relative_path(self, path).as_bytes());
            mix(&mut hash, &[u8::from(document.is_deleted())]);
            mix(&mut hash, document.text.as_bytes());
        }
        for (path, document) in &self.authoring_documents {
            mix(&mut hash, b"authoring");
            mix(&mut hash, relative_path(self, path).as_bytes());
            mix(&mut hash, &[u8::from(document.is_deleted())]);
            mix(&mut hash, &document.bytes);
        }
        format!("{hash:016x}")
    }
}

fn relative_path(project: &Project, path: &Path) -> String {
    path.strip_prefix(&project.root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn mix(hash: &mut u64, bytes: &[u8]) {
    // 长度分隔防止不同路径/内容切分组成同一字节流。
    for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

use super::{PlaythroughReportError, PlaythroughSource, PlaythroughSourceFile};
use std::collections::BTreeMap;
use std::path::Component;
use worldline_core::CompileResult;

pub(super) struct Sources {
    pub manifest: Vec<PlaythroughSourceFile>,
    pub snapshot: String,
    pub paths: BTreeMap<String, String>,
}
impl Sources {
    pub fn new(snapshot: &CompileResult, maximum: usize) -> Result<Self, PlaythroughReportError> {
        let invalid = || {
            PlaythroughReportError::new(
                "invalid_snapshot",
                "当前源快照缺失、过大或不能形成安全相对路径",
            )
        };
        let mut used = 0usize;
        if snapshot.sources.is_empty() || snapshot.sources.len() > 4096 {
            return Err(invalid());
        }
        let mut root = snapshot
            .sources
            .keys()
            .next()
            .and_then(|path| path.parent())
            .ok_or_else(invalid)?
            .to_path_buf();
        for (path, source) in &snapshot.sources {
            used = used
                .checked_add(path.as_os_str().len())
                .and_then(|n| n.checked_add(source.len()))
                .ok_or_else(invalid)?;
            if used > 64 * 1024 * 1024
                || path.components().any(|c| matches!(c, Component::ParentDir))
            {
                return Err(invalid());
            }
            while !path.starts_with(&root) {
                if !root.pop() {
                    return Err(invalid());
                }
            }
        }
        let mut projected_bytes = 2usize;
        let mut paths = BTreeMap::new();
        let mut manifest = Vec::new();
        let mut hash = 0xcbf29ce484222325u64;
        for (path, source) in &snapshot.sources {
            let full = path.to_str().ok_or_else(invalid)?;
            let relative = path.strip_prefix(&root).map_err(|_| invalid())?;
            if relative.as_os_str().is_empty()
                || relative
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
            {
                return Err(invalid());
            }
            let relative = relative
                .to_str()
                .ok_or_else(invalid)?
                .replace(std::path::MAIN_SEPARATOR, "/");
            if relative.contains(['\\', ':']) || relative.chars().any(char::is_control) {
                return Err(invalid());
            }
            // 每项在复制到manifest前计量，固定外壳保守预留。
            projected_bytes = projected_bytes.saturating_add(64);
            projected_bytes =
                projected_bytes.saturating_add(crate::route_comparison::encoded_size(
                    &relative,
                    maximum.saturating_sub(projected_bytes),
                )?);
            for bytes in [relative.as_bytes(), source.as_bytes()] {
                hash_bytes(&mut hash, &(bytes.len() as u64).to_le_bytes());
                hash_bytes(&mut hash, bytes);
            }
            paths.insert(full.into(), relative.clone());
            manifest.push(PlaythroughSourceFile {
                file: relative,
                bytes: source.len(),
                digest: digest(source.as_bytes()),
            });
        }
        Ok(Self {
            manifest,
            paths,
            snapshot: format!("fnv1a64:{hash:016x}"),
        })
    }
    pub fn location(&self, source: &super::capture::RawSource) -> Option<PlaythroughSource> {
        Some(PlaythroughSource {
            file: self.paths.get(&source.file)?.clone(),
            line: source.line,
            column: source.column,
            precision: "statement".into(),
        })
    }
}
pub(super) fn digest(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325;
    hash_bytes(&mut hash, bytes);
    format!("fnv1a64:{hash:016x}")
}
fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
}

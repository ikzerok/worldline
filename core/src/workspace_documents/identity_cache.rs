//! 单次注册表解析的有界身份集；保留Handle而非提前关闭的身份快照。
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
const MAX_HANDLES: usize = 512;

pub(super) struct IdentityCache {
    #[cfg(not(target_arch = "wasm32"))]
    handles: Option<HashSet<same_file::Handle>>,
}

impl IdentityCache {
    pub(super) fn new(manifest: &Path) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                handles: open(manifest).ok().map(|handle| HashSet::from([handle])),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = manifest;
            Self {}
        }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(super) fn cached_handle_count(&self) -> Option<usize> {
        self.handles.as_ref().map(HashSet::len)
    }

    pub(super) fn duplicates<'a>(
        &mut self,
        registered: impl Iterator<Item = &'a PathBuf>,
        candidate: &Path,
    ) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(handles) = &mut self.handles {
                if handles.len() < MAX_HANDLES {
                    if let Ok(handle) = open(candidate) {
                        return !handles.insert(handle);
                    }
                }
                // 先释放所有持有句柄，避免资源不足时连原来的逐对检查也失败。
                self.handles = None;
            }
            let mut registered = registered;
            registered.any(|path| {
                #[cfg(test)]
                COUNTS.with(|counts| counts.borrow_mut().1 += 1);
                same_file::is_same_file(path, candidate).unwrap_or(false)
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (registered, candidate);
            false
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn open(path: &Path) -> std::io::Result<same_file::Handle> {
    #[cfg(test)]
    COUNTS.with(|counts| counts.borrow_mut().0 += 1);
    same_file::Handle::from_path(path)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
thread_local! {
    static COUNTS: std::cell::RefCell<(usize, usize)> = const { std::cell::RefCell::new((0, 0)) };
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn reset_counts() {
    COUNTS.with(|counts| *counts.borrow_mut() = (0, 0));
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn counts() -> (usize, usize) {
    COUNTS.with(|counts| *counts.borrow())
}

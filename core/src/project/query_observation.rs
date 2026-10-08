//! 浏览依赖观测与作者撤销代次分离；只在载入/刷新时访问外部元数据。
use super::*;

#[derive(Default)]
pub(super) struct Observation {
    fingerprint: String,
    epoch: u64,
    error: Option<String>,
}

impl Project {
    /// Project候选和撤销快照共享观测记录，旧候选提交不能把附件状态倒回。
    pub(super) fn record_query_observation(&self, refresh_error: Option<&str>) {
        let observed = self.problems_observation_key();
        let error = refresh_error
            .map(str::to_owned)
            .or_else(|| observed.as_ref().err().map(ToString::to_string));
        let mut current = self
            .query_observation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match (observed, error) {
            (Ok(fingerprint), None) => {
                if current.fingerprint != fingerprint || current.error.is_some() {
                    current.epoch = current.epoch.wrapping_add(1);
                    current.fingerprint = fingerprint;
                }
                current.error = None;
            }
            (_, error) => {
                // 失败没有证实依赖不变；每次明确刷新重试都使旧可信缓存失效。
                current.epoch = current.epoch.wrapping_add(1);
                current.error = error.or_else(|| Some("外部文件观测未完成".into()));
            }
        }
    }

    /// 无IO的缓存依赖标记；不能当作当前磁盘签名。
    pub(crate) fn manuscript_observation_key(&self) -> String {
        let current = self
            .query_observation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        format!(
            "{}|{}|{:?}",
            current.epoch, current.fingerprint, current.error
        )
    }

    pub(crate) fn manuscript_observation_error(&self) -> Option<String> {
        self.query_observation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .error
            .clone()
    }
}

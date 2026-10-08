use super::*;

pub(super) const MAX_REQUEST: usize = 32 * 1024 * 1024;
pub(super) const MAX_RESPONSE: usize = 32 * 1024 * 1024;

pub(super) fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>, String> {
    encode_limited(value, MAX_RESPONSE)
}

pub(super) fn encode_limited(
    value: &impl serde::Serialize,
    limit: usize,
) -> Result<Vec<u8>, String> {
    struct Buffer {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("reconciliation_output_limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut out = Buffer {
        bytes: Vec::new(),
        limit: limit.saturating_sub(1),
    };
    serde_json::to_writer(&mut out, value)
        .map_err(|_| "外改完整响应超过32 MiB编码预算；不返回截断候选".to_owned())?;
    out.bytes.push(b'\n');
    Ok(out.bytes)
}

pub(super) fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = worldline_core::file_access::read_limited(path, MAX_REQUEST)
        .map_err(|error| format!("无法读取请求：{error}"))?;
    let value = worldline_core::parse_unique_json(&bytes)?;
    for key in ["files", "choices"] {
        if let Some(items) = value.get(key).and_then(Value::as_array) {
            if items.len() > 4096 {
                return Err("外改输入超过4096项预算".into());
            }
            for item in items {
                if let Some(path) = item.get("path").and_then(Value::as_str) {
                    if path.len() > 4096 || path.split(['/', '\\']).count() > 128 {
                        return Err("外改路径超过4096字节或128层预算".into());
                    }
                }
            }
        }
    }
    serde_json::from_value(value).map_err(|_| "外改JSON材料或候选DTO无效".into())
}

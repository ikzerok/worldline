//! 作者路径交换；独立于报告/比较的业务预算，见 spec/replay-exchange.md。
use crate::{ReplayObservation, ReplayTrace, REPLAY_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use std::io::Write;

pub const MAX_REPLAY_EXCHANGE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_REPLAY_EXCHANGE_STEPS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayExchangeError {
    pub code: String,
    pub message: String,
}

impl ReplayExchangeError {
    fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ReplayExchangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}：{}", self.code, self.message)
    }
}
impl std::error::Error for ReplayExchangeError {}

/// 只读格式导入；旧 runtime 可查看，真正执行仍须通过重放的兼容守卫。
pub fn decode_replay_trace(bytes: &[u8]) -> Result<ReplayTrace, ReplayExchangeError> {
    if bytes.len() > MAX_REPLAY_EXCHANGE_BYTES {
        return Err(ReplayExchangeError::new(
            "input_limit",
            "路径交换 JSON 超过 4 MiB 字节上限",
        ));
    }
    let value = worldline_core::parse_unique_json(bytes).map_err(|error| {
        ReplayExchangeError::new("invalid_json", &format!("路径 JSON 无效：{error}"))
    })?;
    let trace: ReplayTrace = serde_json::from_value(value).map_err(|error| {
        ReplayExchangeError::new("invalid_trace", &format!("路径格式不兼容：{error}"))
    })?;
    validate(&trace)?;
    Ok(trace)
}

fn validate(trace: &ReplayTrace) -> Result<(), ReplayExchangeError> {
    if trace.schema_version != REPLAY_SCHEMA_VERSION {
        return Err(ReplayExchangeError::new(
            "unsupported_schema",
            "路径 schema_version 不受支持",
        ));
    }
    if trace.steps.len() > MAX_REPLAY_EXCHANGE_STEPS {
        return Err(ReplayExchangeError::new(
            "step_limit",
            "路径交换超过 20,000 步上限",
        ));
    }
    Ok(())
}

/// 有界紧凑 JSON；成功产物可由同版解码，绝不返回一段错误文本冒充 JSON。
pub fn encode_replay_trace(trace: &ReplayTrace) -> Result<String, ReplayExchangeError> {
    validate(trace)?;
    // 外部 JSON 已有默认深度保护；内存中构造的 Value 也不能递归序列化至栈溢出。
    let mut count = 0;
    if let Some(observation) = &trace.initial_observation {
        check_depth(observation, 2, &mut count)?;
    }
    for step in &trace.steps {
        if let Some(observation) = &step.observation {
            check_depth(observation, 4, &mut count)?;
        }
    }
    let mut writer = LimitedWriter(Vec::new());
    serde_json::to_writer(&mut writer, trace).map_err(|_| {
        ReplayExchangeError::new("output_limit", "路径交换 JSON 无法在 4 MiB 字节上限内编码")
    })?;
    // 与真实读取同一深度/结构门禁，不用自写深度估计替代解析器的最终决定。
    let decoded = decode_replay_trace(&writer.0)?;
    if decoded != *trace {
        return Err(ReplayExchangeError::new(
            "invalid_trace",
            "路径 JSON 无法无损往返；原路径保留",
        ));
    }
    String::from_utf8(writer.0)
        .map_err(|_| ReplayExchangeError::new("invalid_json", "路径编码未产生有效 UTF-8"))
}

fn check_depth(
    observation: &ReplayObservation,
    depth: usize,
    count: &mut usize,
) -> Result<(), ReplayExchangeError> {
    check_value(&observation.state, depth + 1, count)?;
    for value in observation
        .outputs
        .iter()
        .chain(&observation.choice_presentation)
    {
        check_value(value, depth + 2, count)?;
    }
    Ok(())
}

fn check_value(
    value: &serde_json::Value,
    level: usize,
    count: &mut usize,
) -> Result<(), ReplayExchangeError> {
    // 固定深度递归，不建立与巨大容器长度同阶的临时引用栈。
    if level > 128 {
        return Err(depth_error());
    }
    let minimum = match value {
        serde_json::Value::String(text) => text.len().saturating_add(1),
        _ => 1,
    };
    *count = count.saturating_add(minimum);
    if *count > MAX_REPLAY_EXCHANGE_BYTES {
        return Err(ReplayExchangeError::new(
            "output_limit",
            "路径 JSON 记录数量超过字节上限",
        ));
    }
    match value {
        serde_json::Value::Array(items) => {
            for value in items {
                check_value(value, level + 1, count)?;
            }
        }
        serde_json::Value::Object(items) => {
            for (key, value) in items {
                if key.len() > MAX_REPLAY_EXCHANGE_BYTES {
                    return Err(string_error());
                }
                *count = count.saturating_add(key.len());
                check_value(value, level + 1, count)?;
            }
        }
        serde_json::Value::String(text) if text.len() > MAX_REPLAY_EXCHANGE_BYTES => {
            return Err(string_error())
        }
        _ => {}
    }
    Ok(())
}
fn string_error() -> ReplayExchangeError {
    ReplayExchangeError::new("output_limit", "路径 JSON 字符串超过 4 MiB 字节上限")
}

fn depth_error() -> ReplayExchangeError {
    ReplayExchangeError::new("invalid_json", "路径 JSON 嵌套超过受保护的解析深度")
}

struct LimitedWriter(Vec<u8>);
impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_REPLAY_EXCHANGE_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("路径 JSON 字节超额"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

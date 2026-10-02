//! CLI/RPC 共用的场景预览摘要，不承载文件寻址或保存副作用。
use crate::vector_scene::{SceneBatch, SceneError, ScenePlan};

pub const CAPABILITY: &str = "authoring.vector_scene.v1";

/// 绑定原请求、内容基线与真实候选；非密码学签名，apply仍须core重算。
pub fn plan_digest(
    baseline: &str,
    batch: &SceneBatch,
    plan: &ScenePlan,
) -> Result<String, SceneError> {
    let bytes = serde_json::to_vec(&("worldline-scene-plan-v1", baseline, batch, &plan.after_hash))
        .map_err(|error| SceneError::new("SCENE_STORAGE", format!("场景摘要无法编码：{error}")))?;
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(format!("{hash:016x}"))
}

use crate::{project::Project, CompileResult};
use serde_json::Value;
use std::path::Path;

pub(super) fn equivalent(
    before_project: &Project,
    after_project: &Project,
    before: &CompileResult,
    old: &Path,
    new: &Path,
) -> Result<(), String> {
    let after = after_project.compile_current();
    if after.has_errors() {
        let error = after
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity == crate::Severity::Error)
            .unwrap();
        return Err(format!("移动候选无效：{} {}", error.code, error.message));
    }
    let old = old.to_string_lossy();
    let new = new.to_string_lossy();
    let map = |value: &str| {
        if value == old {
            new.to_string()
        } else {
            value.to_owned()
        }
    };
    let files: Vec<_> = before.program.files.iter().map(|file| map(file)).collect();
    if files != after.program.files || before.program.entry != after.program.entry {
        return Err(
            "路径移动会改变语义加载顺序或默认入口；请先显式安排 include 顺序再重新预览，工程未修改"
                .into(),
        );
    }
    if before.analysis.fingerprint != after.analysis.fingerprint {
        return Err(format!("移动改变运行身份/指纹（{} → {}）；旧 Story 存档和检查点不可直接沿用，trace 须重新验证。本版不迁移，工程未修改", before.analysis.fingerprint, after.analysis.fingerprint));
    }
    // 全目录保留声明、静态属性、引用、状态与显示信息；只归一允许变化的来源路径和原始素材路径。
    let mut left =
        serde_json::to_value(&before.analysis.catalog).map_err(|error| error.to_string())?;
    let mut right =
        serde_json::to_value(&after.analysis.catalog).map_err(|error| error.to_string())?;
    normalize(&mut left, &old, &new);
    normalize(&mut right, &new, &new);
    canonical_catalog_order(&mut left);
    canonical_catalog_order(&mut right);
    if left != right {
        return Err(
            "移动候选的对象身份、资料或正式引用目标不同，无法证明语义等价，工程未修改".into(),
        );
    }
    if before_project.compile_options() != after_project.compile_options() {
        return Err("移动不能改变语言版本或 required feature".into());
    }
    Ok(())
}

fn normalize(value: &mut Value, old: &str, new: &str) {
    match value {
        Value::Object(object) => {
            // 官方 TargetRef。
            if object.get("kind").and_then(Value::as_str) == Some("file")
                && object.get("id").and_then(Value::as_str) == Some(old)
            {
                object.insert("id".into(), Value::String(new.into()));
            }
            // 所有目录来源位置沿原文件一一映射，不改变普通描述或作者属性值。
            if object.get("file").and_then(Value::as_str) == Some(old) {
                object.insert("file".into(), Value::String(new.into()));
            }
            if object.get("resolved_path").is_some() && object.get("available").is_some() {
                // 已由正式 token 和资源字节证明相对路径的重基，保留绝对解析路径。
                object.remove("path");
            }
            if object
                .get("target")
                .and_then(|target| target.get("kind"))
                .and_then(Value::as_str)
                == Some("file")
                && object.contains_key("display")
            {
                // 文件目录条目的缺省显示名正是文件名；不影响作者对象显示名。
                if let Some(id) = object
                    .get("target")
                    .and_then(|target| target.get("id"))
                    .and_then(Value::as_str)
                {
                    if id == old || id == new {
                        object.insert(
                            "display".into(),
                            Value::String(
                                Path::new(new)
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                            ),
                        );
                    }
                }
            }
            if object.contains_key("label")
                && object.contains_key("column")
                && object.contains_key("target")
            {
                object.remove("column");
            }
            for (key, child) in object.iter_mut() {
                // properties 为作者值，只有有类型 Ref 才允许身份归一；file 本身非合法 Ref kind。
                if key != "properties" {
                    normalize(child, old, new);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize(item, old, new);
            }
        }
        _ => {}
    }
}

// 目录 objects/relation_index 自身按 TargetRef 排序；路径身份映射后须重排同一派生索引，
// 不能将字典序变动误判为声明顺序变动。真正语义顺序已由 program.files 严格证明。
fn canonical_catalog_order(value: &mut Value) {
    for field in ["objects", "relation_index"] {
        if let Some(items) = value.get_mut(field).and_then(Value::as_array_mut) {
            items.sort_by_cached_key(|item| {
                item.get("target").map(Value::to_string).unwrap_or_default()
            });
        }
    }
}

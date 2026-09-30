//! 存档全局值校验。缺失块内声明表示未初始化，不能与未知字段混淆。
use super::{RunError, Value};
use std::collections::HashMap;
use worldline_core::{
    ast::{Program, ValueKind},
    Analysis,
};

pub(super) fn validate_saved_vars(
    program: &Program,
    analysis: &Analysis,
    vars: &HashMap<String, Value>,
) -> Result<(), RunError> {
    for declaration in &program.lets {
        if !vars.contains_key(&declaration.name) {
            return Err(RunError::new(format!(
                "存档缺少已初始化顶层变量 `{}`",
                declaration.name
            )));
        }
    }
    for (name, value) in vars {
        let Some(info) = analysis.symbols.vars.get(name) else {
            return Err(RunError::new(format!("存档包含未知变量 `{name}`")));
        };
        validate_value(analysis, value, info.kind)
            .map_err(|message| RunError::new(format!("存档变量 `{name}` 无效：{message}")))?;
    }
    Ok(())
}

pub(super) fn validate_value(
    analysis: &Analysis,
    value: &Value,
    expected: Option<ValueKind>,
) -> Result<(), String> {
    let kind = match value {
        Value::Num(number) => {
            if !number.is_finite() {
                return Err("数值不是有限数".into());
            }
            ValueKind::Num
        }
        Value::Str(_) => ValueKind::Str,
        Value::Bool(_) => ValueKind::Bool,
        Value::Tag(id) => {
            if !analysis.catalog.tags.get(id).is_some_and(|t| t.declared) {
                return Err("标签身份不存在".into());
            }
            ValueKind::Tag
        }
        Value::TagSet(ids) => {
            if ids
                .iter()
                .any(|id| !analysis.catalog.tags.get(id).is_some_and(|t| t.declared))
            {
                return Err("集合包含未知标签".into());
            }
            if ids.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err("标签集合必须排序且无重复".into());
            }
            ValueKind::TagSet
        }
        Value::StateRef(id) => {
            if !analysis.catalog.states.contains_key(id) {
                return Err("状态身份不存在".into());
            }
            ValueKind::StateRef
        }
    };
    if expected.is_some_and(|expected| expected != kind) {
        return Err("值类型与声明不一致".into());
    }
    Ok(())
}

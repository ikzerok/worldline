//! 对象身份和静态初值仅从该 Story 冻结的 catalog 取得。
use super::{capture::BorrowedValue, model::*};
use crate::Story;
use serde::{Deserialize, Deserializer, Serialize};
use worldline_core::{states::StateInfo, TargetRef};

pub(super) fn deserialize_owner<'de, D>(deserializer: D) -> Result<Option<TargetRef>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Owner {
        kind: String,
        id: String,
    }
    Ok(
        Option::<Owner>::deserialize(deserializer)?.map(|owner| TargetRef {
            kind: owner.kind,
            id: owner.id,
        }),
    )
}

impl StateInspectionQuery {
    pub fn validate(&self) -> Result<(), StateInspectionError> {
        if self.limit == 0 || self.limit > 100 || self.text.chars().count() > 256 {
            return Err(StateInspectionError {
                code: "INVALID_INSPECTION_QUERY".into(),
                message: "状态查询每页须为1–100项，检索词最多256字符".into(),
            });
        }
        let Some(owner) = &self.owner else {
            return Ok(());
        };
        if self
            .group
            .is_some_and(|group| group != InspectionGroup::State)
            || owner.id.is_empty()
            || !worldline_core::catalog::TARGET_KINDS.contains(&owner.kind.as_str())
            || crate::route_comparison::encoded_size(owner, MAX_INSPECTION_OWNER_BYTES).is_err()
        {
            return Err(StateInspectionError {
                code: "INVALID_INSPECTION_QUERY".into(),
                message: "对象查询须使用完整已知种类和非空ID，身份最多16KiB，且只能筛选state组"
                    .into(),
            });
        }
        Ok(())
    }
}

impl Story<'_> {
    pub(super) fn inspection_owner(
        &self,
        query: &StateInspectionQuery,
    ) -> Result<Option<InspectionOwner>, StateInspectionError> {
        let Some(owner) = &query.owner else {
            return Ok(None);
        };
        let object = self
            .catalog
            .object(owner)
            .ok_or_else(|| StateInspectionError {
                code: "INSPECTION_OWNER_NOT_FOUND".into(),
                message: "选定对象不在这次运行的冻结目录中，请核对完整对象身份与运行版本".into(),
            })?;
        // 先计量借用数据，避免为了返回超额错误而复制任意长的展示文本。
        crate::route_comparison::encoded_size(
            &(&object.target, &object.display),
            MAX_INSPECTION_OUTPUT_BYTES,
        )
        .map_err(|_| output_limit())?;
        Ok(Some(InspectionOwner {
            target: object.target.clone(),
            display: object.display.clone(),
        }))
    }
}

pub(super) fn state_definition(
    info: &StateInfo,
    remaining: &mut usize,
) -> Result<InspectionStateDefinition, StateInspectionError> {
    #[derive(Serialize)]
    struct BorrowedDefinition<'a> {
        owner: &'a TargetRef,
        display: &'a str,
        initial: &'a InspectionCell,
    }
    let initial = BorrowedValue::Tags(&info.tags).cell();
    let bytes = crate::route_comparison::encoded_size(
        &BorrowedDefinition {
            owner: &info.target,
            display: &info.display,
            initial: &initial,
        },
        *remaining,
    )
    .map_err(|_| output_limit())?;
    *remaining -= bytes;
    Ok(InspectionStateDefinition {
        owner: info.target.clone(),
        display: info.display.clone(),
        initial,
    })
}

pub(super) fn output_limit() -> StateInspectionError {
    StateInspectionError {
        code: "INSPECTION_OUTPUT_LIMIT".into(),
        message: "检查页超过1MiB输出预算，请减少每页条数或缩小检索".into(),
    }
}

use super::{CapabilityEnablePlan, CapabilityKeywordChange};
use crate::{CompileOptions, CompileResult, Diagnostic};
use std::collections::BTreeMap;

pub(super) fn added_diagnostics(before: &[Diagnostic], after: &[Diagnostic]) -> Vec<Diagnostic> {
    let mut counts = BTreeMap::<String, usize>::new();
    for diagnostic in before {
        *counts.entry(diagnostic_key(diagnostic)).or_default() += 1;
    }
    after
        .iter()
        .filter(|diagnostic| {
            let count = counts.entry(diagnostic_key(diagnostic)).or_default();
            if *count > 0 {
                *count -= 1;
                false
            } else {
                true
            }
        })
        .cloned()
        .collect()
}

fn diagnostic_key(diagnostic: &Diagnostic) -> String {
    // Diagnostic 公开序列化包含 severity、位置、note、suggestion 和 related。
    serde_json::to_string(diagnostic).expect("Diagnostic 只含可序列化字段")
}

pub(super) fn keyword_changes(
    before: &CompileResult,
    options: CompileOptions,
) -> Vec<CapabilityKeywordChange> {
    let mut changes = Vec::new();
    for (path, source) in &before.sources {
        let file = path.to_string_lossy();
        let old =
            crate::lexer::lex_source_with_options(&file, source, &mut Vec::new(), before.options);
        let new = crate::lexer::lex_source_with_options(&file, source, &mut Vec::new(), options);
        let old_by_line = old
            .iter()
            .map(|line| (line.no, line))
            .collect::<BTreeMap<_, _>>();
        let raw_lines = source.lines().collect::<Vec<_>>();
        for line in new {
            let Some(previous) = old_by_line.get(&line.no) else {
                continue;
            };
            if format!("{:?}", previous.kind) == format!("{:?}", line.kind) {
                continue;
            }
            changes.push(CapabilityKeywordChange {
                file: file.to_string(),
                line: line.no,
                source: raw_lines
                    .get(line.no as usize - 1)
                    .unwrap_or(&"")
                    .to_string(),
                before_kind: kind_name(&previous.kind),
                after_kind: kind_name(&line.kind),
            });
        }
    }
    changes
}

fn kind_name(kind: &crate::lexer::LineKind) -> String {
    use crate::lexer::LineKind;
    match kind {
        LineKind::Text { .. } => "普通正文".into(),
        LineKind::Schema112 { keyword, .. } | LineKind::Language111 { keyword, .. } => {
            format!("语法 {keyword}")
        }
        LineKind::Entity { .. } => "语法 entity".into(),
        LineKind::RelationType { .. } => "语法 relation_type".into(),
        LineKind::RelationDef { .. } => "语法 relation_def".into(),
        LineKind::RelationField { name, .. } => format!("关系字段 {name}"),
        LineKind::Choice { .. } => "choice 语法或身份注记".into(),
        _ => format!("{:?}", kind)
            .split_whitespace()
            .next()
            .unwrap_or("语法")
            .into(),
    }
}

pub(super) fn compatibility_notes(plan: &CapabilityEnablePlan) -> Vec<String> {
    let mut notes = vec![
        "预览完整已应用活动源码，包含已应用但未保存的改稿；未应用的写作或表单草稿不在本计划内。归档源码不作为运行输入。".into(),
        "本次只启用已有能力；成功后只修改内存清单，可撤销，仍需显式保存。不会执行故事、消耗随机数或迁移旧存档。".into(),
        "旧客户端若不支持目标语言或 required_features，应只读保留工程；选择版本不会自动创建资料对象，也不自动翻译正文。".into(),
    ];
    if plan.current_language != plan.target_language {
        notes.push("提高语言版本会按新语法重新解释全文，包括旧稿中同名关键字；词法变化列表只是定位辅助，不能证明语义等价。请核对全部候选诊断及正文。".into());
    }
    if !plan.fingerprint_comparison_reliable {
        notes.push("当前稿或候选含编译错误，显示的指纹不能作为存档兼容证据；候选存在错误时禁止提交，原错误稿保持不变。".into());
    } else if plan.runtime_fingerprint_before != plan.runtime_fingerprint_after {
        notes.push("本次候选的运行 fingerprint 已变化：旧 Story 存档和运行检查点不能直接载入；入口 replay trace 只能重新严格验证，不保证沿用。".into());
    } else {
        notes.push("本次完整编译的前后运行 fingerprint 相同；这只适用于本次候选，不保证其他升级或之后改稿兼容，也不替代旧存档和严格重放的检查。".into());
    }
    notes
}

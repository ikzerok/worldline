# 普通外改的 Rust / CLI / RPC 用法

完整合同见 [workspace-reconciliation.md](../spec/workspace-reconciliation.md)。
本功能只处理普通外改交叉冲突，不绕过保存事务恢复。所有选择与手工候选均由 core 检查，
没有自动合并或隐式保存。

## Rust

```rust,ignore
use worldline_core::project::reconciliation::{
    ReconciliationChoice, ReconciliationDecision, ReconciliationRequest,
};
let session = project.capture_reconciliation()?;
let request = ReconciliationRequest {
    choices: vec![ReconciliationDecision {
        path: "world.wl".into(),
        choice: ReconciliationChoice::Manual { text: reviewed_text },
    }],
    allow_incomplete_source: false,
};
let plan = project.preview_reconciliation(&session, &request)?;
// 显示三方、plan.files完整候选、plan.problems及plan.blockers，等待明确采纳。
let adopted = project.apply_reconciliation(&plan)?;
// 清空旧代次undo/redo，将adopted.undo作为本次唯一撤销快照。
// 此时磁盘未写；下一次明确保存才调用：
project.save()?;
```

后台可使用 `prepare_reconciliation_with_progress` 和
`commit_prepared_reconciliation`，最终提交必须作用于真实当前 Project。准备完成不意味着
采纳完成，调用方不能直接用一个后台 Project clone 替换当前工程。所有拒绝与取消均保留
原 Project；撤销和重做继续通过 `Project::restore`，不能直接赋值旧快照。

## 一次性 CLI

输入 JSON 文件应放在作品目录以外，避免把工作材料本身纳入完整磁盘守卫而导致摘要改变。
input 显式提供旧保存基线与本地稿，磁盘侧由 core 读取。字段 bytes 使用 JSON 字节数组，
`null` 表示缺失，`[]` 表示空文件；不能填入猜测的旧基线。

```json
{"schema_version":1,"files":[{"path":"world.wl","baseline":[47,47,32,111,108,100,10],"local":[47,47,32,108,111,99,97,108,10]}]}
```

这是字节表示示例，实际输入应来自你的完整旧稿/本地稿。候选请求示例：

```json
{"choices":[{"path":"world.wl","choice":{"kind":"local"}}],"allow_incomplete_source":false}
```

```sh
wl reconciliation capture /作品 --input /材料/input.json --json
wl reconciliation preview /作品 --input /材料/input.json --request /材料/request.json --json
wl reconciliation apply /作品 --input /材料/input.json --request /材料/request.json --plan-digest 已审阅摘要 --json
wl reconciliation save /作品 --input /材料/input.json --request /材料/request.json --plan-digest 已审阅摘要 --json
```

preview 输出全部计划；apply 只在本进程内采纳，返回实际候选与 `saved:false`，退出时不会
假装保持一个常驻编辑器会话。独立 save 命令重新构建同一完整计划并核对摘要，再调用原有
保存事务。外部变化、手工候选变化或新的保护状态都会使旧摘要失效。save 部分失败保留实际
事务日志，响应明确 `applied:true,saved:false`；不要用旧内容回滚并覆盖第三方文件。

## 有状态 RPC

使用已有 `project_id`，依次调用 `reconciliation.capture`、
`reconciliation.preview`（含同上 request）、`reconciliation.apply`（含已审阅
`plan_digest`）。原 Project 中已应用未保存稿就是本地侧。采纳仍返回 `saved:false`，
后续明确调用 `project.save {project_id,expected_baseline}`。

能力为 `authoring.workspace_reconciliation.v1`。合法请求遇到陈旧、保护或未解决项是
`ok:false` 领域拒绝；JSON/参数不符合合同才是 RPC -32602。序列化报告不能重新作为受信任
Project 写入授权，RPC缓存正式core计划并在采纳时重新核验。

## 大材料的机器输出

JSON字节数组比原文大得多。CLI input/request各限32 MiB，完整响应连换行限32 MiB；RPC
对请求、完整payload、id及外壳另作编码计数。超大材料返回`OUTPUT_LIMIT`，不截断候选，
不先采纳再报告失败。请保留原材料并在编辑器中继续检查，或缩小明确输入范围后重新预览；
不能删掉同一候选中的必须冲突文件来绕过未解决守卫。巨id/非法参数不会被回显进超大错误。

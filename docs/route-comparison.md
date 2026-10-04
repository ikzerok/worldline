# 比较真实路线并找到状态动作

0.20 的 `wl route-compare` 与 `project.compare_routes` 在同一份当前工程稿上分别验证两条真实录制路径。它们共用 runtime 的比较结果及 core 验证的动作来源，不把静态关系图当执行记录。

## 先录制两条路径

用当前 0.20 工具打开作品，分别走出两种选择。选择相同 seed 有助于控制随机起点；只相同 seed 不代表任意两个检查点可对齐。

```powershell
wl play .\作品 --seed 42 --trace-output=.\route-a.json
wl play .\作品 --seed 42 --trace-output=.\route-b.json
$left = Get-Content .\route-a.json -Raw -Encoding utf8
$right = Get-Content .\route-b.json -Raw -Encoding utf8
wl route-compare .\作品 --left-trace-json $left --right-trace-json $right --json
```

`route-compare` 读取作品的只读快照，不自动恢复未完成保存事务，不迁移、应用或保存作品。若存在未解决事务，先正常打开作品并处理，再比较。独立 CLI 不能读取正在运行的 worldedit 窗口内未应用的草稿。

agent 方法只接受已打开的 `project_id`；比较当前已加载缓冲，不暗中刷新磁盘，也不组合两份旧 Story 会话。

```json
{"jsonrpc":"2.0","id":7,"method":"project.compare_routes","params":{"project_id":"项目ID","left_trace":{},"right_trace":{},"max_steps":100000,"time_budget_ms":30000}}
```

上面的两个空对象仅表示参数位置，实际必须传完整录制得到的 trace。方法能力为 `authoring.route_comparison.v1`，精确字段与错误规则见[正式契约](../spec/route-comparison.md)。

## 如何读结果

- 先看每侧 `status`、`complete`、`ended`。只有 `replayed` 且后两项均为 true 才是完整结束并验证通过；已记录部分区段通过不代表完整结局
- `alignment` 仅比较真实可对齐起点后的已验证选择前缀。不同 seed、entry/checkpoint 混合或不同检查点仍可并列事实，但不能按步骤编号硬配或归因为单一选择
- `state_differences` / `variable_differences` 是本次实际停止值。分歧、失败、超限时不拿原 trace 的预期值冒充当前终态。值相同也不表示两条路线等价
- 每侧 `coverage` 区分 `inherited`、`executed` 与 `total`；检查点旧覆盖不是本段新执行。未访问表示未测试，不表示不可达
- `state_actions` 是实际发生的状态替换/增加标签/移除标签，含顺序、前后标签、所属世界对象与可验证动作来源。同值写入仍可单独出现；证据省略会明确标记
- 一般变量只有值比较，不提供凭猜测得到的动作因果。没有来源时不匹配同名状态、标签或注释

比较输出和来源是只读投影。纯注释移动也会改变源行；编辑器回源必须验证当前完整源码/编译选项/内容基线，不能仅看 runtime fingerprint 相同。

## 限额与退出码

默认且最大合计预算为 100000 个解释器步骤和 30000 ms，两个方向共用；可用 `--max-steps`、`--time-budget-ms` 降低，0 表示零额度。每 trace 最多 4 MiB / 4096 steps；动作证据每侧最多 256 条 / 64 KiB；比较 DTO 最多 1 MiB。完整 CLI/RPC 响应还包括固定有界外壳。更细的记录、输出和单字段边界以正式契约为准。

CLI 两侧 replayed 退出 0（包括明确标注的部分区段）；有结构化分歧、故事失败或预算停止退出 1；参数/结构/版本错误退出 2。RPC 协议错误与 `ok:false` 业务结果分开。

CLI/RPC 是同步有界调用，后续 RPC 不能中断正在处理的比较。runtime 与编辑器支持取消；单条解释器语句、起点建立和检查点恢复不能抢占，不承诺硬实时或所有中间内存分配可抢占。

## 兼容与边界

不新增 DSL，不改变语言默认1.9/最高既有1.13，不修改 Save/ReplayTrace/语义观察形状、choice ID 或 fingerprint。0.19 trace/checkpoint 仍被0.20的 runtime_version 守卫拒绝，应在0.20重新录制，不修改版本字段绕过。普通 Story Save 沿原有保存格式、能力及指纹规则，不把 trace 的版本拒绝扩大到所有旧 Save。

本功能不做路径穷举、全局可达性或完整因果证明，不增加可执行读者站、在线协作或永久路径数据库。是否已发行及本轮真实验证范围，以正式 Release 和配对版本说明为准。

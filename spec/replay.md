# 确定性叙事重放、检查点与条件解释（CAP-06A）

本协议给 runtime 调试 API、CLI 与 agent RPC 共用。它记录实际访问路径，供复现和定位改稿差异；不改变正常故事语义，也不证明未访问分支可达或不可达。

## 版本与起点

重放 DTO 固定包含 `schema_version`、`runtime_version`、来源 `fingerprint` 和显式随机 `seed`。随机种子为零也有固定的规范化方式。运行库版本或 schema 不兼容时拒绝载入，不猜测字段含义。

从故事入口开始的 trace 用 `origin: "entry"` 与 seed 重建初始状态；从中间状态开始的 trace 用 `origin: "checkpoint"`，内含完整 runtime 存档快照、seed 和来源 fingerprint。中途 trace 必须报告 checkpoint 的状态，不可标记为完整入口路径。检查点只可在相同 runtime/schema/fingerprint 下恢复；入口 trace 可以在新 fingerprint 上受控重放，并在首个观察差异或选择不匹配处停止。

检查点与作者 Project、源文件缓冲和内容基线完全分离；生成、恢复和重放不会写入或修改 Project。

## 选择身份与轨迹

每个可见选择包含稳定 `id`、节点、源行、组内偏移和渲染标签。ID 由节点、原始选择标签、条件的结构化表示、`once` 标志及相同签名选择的出现序号生成，不含行号；插入其他不同选项不会改变 ID。行号与偏移只用于定位和当前呈现，不能作为静默回退的选择依据。

Trace 记录起点首次推进观察，以及每次实际选择后的观察。观察包含输出、可选选择的 ID、状态视图和访问覆盖。回放只在当前暂停选择中找到完全相同 ID 时才做选择；节点或选择缺失时立即返回带预期/实际位置的 divergence，不按旧索引替选。输出、可用选择或状态观察与记录不同时也立即停止，并报告差异及当前节点/源行。

若用户从中途 `Story` 开始记录 trace，则 API 自动把当前完整快照记录为 checkpoint origin。轨迹不把这段路径冒充为故事开头。

## 确定性、预算与覆盖

相同 runtime 版本、程序、初始状态、输入路径与 seed 应得到相同输出、状态观察和覆盖。重放可指定最大解释器语句步数与墙钟时限，并可接入取消令牌；任一限制触发时返回已完成的步骤数、当前位置和实际访问覆盖，不返回“完整通过”。

覆盖只投影实际进入的节点及实际选择的稳定 ID 与次数。它不声明未访问节点不可达，也不穷尽一般程序路径。循环路径受步数与时间限制保护。

## 条件解释

`explain_choices` 仅针对当前选择组返回每项条件的表达式、只读求值结果、是否可选及阻断原因（条件为 false 或 `once` 已选择）。它复用 runtime 表达式求值语义，但用 RNG 副本；查询不得推进故事、消耗随机状态、增加访问/回合、更新状态或记录锚点。它是当前状态的解释，不是对一般条件的静态证明。

条件解释失败仍报告故事/表达式运行错误；展示的表达式和原因不改写条件的真实执行语义。协议异常与故事失败保持不同错误域。

## CLI 与 agent RPC

`wl play` 可指定 `--seed` 并将 trace 写至 `--trace-output`。`wl replay <入口> --trace-json '<DTO>'` 可指定 `--max-steps` 与 `--time-budget-ms`。RPC 的 `session.open` 可传 `seed`；`session.trace`、`session.checkpoint` 与 `session.explain_choices` 读取同一 runtime API；`trace.replay` 接收同一 trace/budget DTO。CLI/RPC 的参数或 DTO 结构错误属于调用错误，节点/选择不匹配、预算耗尽、取消或运行失败是结构化故事结果。

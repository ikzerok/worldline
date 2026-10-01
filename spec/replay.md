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
合作式 `ReplaySession` 可按帧推进：`new` 验证 trace 并设定跨帧累计总预算，`advance` 每次最多执行调用方指定的解释器步数或墙钟时间；`Ok(None)` 表示让出，`Ok(Some(ReplayResult))` 为包含明确状态的最终结果，`Err` 表示协议验证、运行创建或检查点恢复错误。会话保留检查点、待完成输出、选择进度、seed 与覆盖；后续推进继续同一状态，不从入口重跑。取消通过 `Cancelled` 和已观察的部分覆盖返回，不修改原 trace 或 Project。单条解释器语句与检查点恢复本身不可抢占；同步 `ReplayTrace::replay`、CLI、RPC 与原生编辑器 worker 保持完整同步调用语义。

覆盖只投影实际进入的节点及实际选择的稳定 ID 与次数。它不声明未访问节点不可达，也不穷尽一般程序路径。循环路径受步数与时间限制保护。

## 条件解释

`explain_choices` 仅针对当前选择组返回每项条件的表达式、只读求值结果、是否可选及阻断原因（条件为 false 或 `once` 已选择）。它复用 runtime 表达式求值语义，但用 RNG 副本；查询不得推进故事、消耗随机状态、增加访问/回合、更新状态或记录锚点。它是当前状态的解释，不是对一般条件的静态证明。

条件解释失败仍报告故事/表达式运行错误；展示的表达式和原因不改写条件的真实执行语义。协议异常与故事失败保持不同错误域。

### 实际求值证据（可选扩展）

`ConditionExplanation.evidence` 是可选的作者调试数据；缺少时不表示 false。
旧字段 `expression/result/error` 保持原形状；旧 JSON 可不含 evidence。
`Story::choice_evidence` 只读取最近暂停组或该组失败尝试的缓存，不执行表达式。
未暂停、尚未执行的预测解释不携带实际证据。选择成功、重启或新一次组求值会使旧缓存失效。

证据包含 `display_expression`（保留 AST 括号的显示式）、`nodes` 和 `omitted`。
节点按 AST 先序提供 `parent`、`label` 与带 `status` 的结果：
`evaluated` 附带原类型的 `value`；`error` 附带原运行错误 `message`；
`not_evaluated` 仅表示该节点因前序错误没有执行。静态函数参数不伪装成运行变量节点。
布尔 false 是正常求值结果，不是错误。证据来自同一次真实求值，不另行执行子表达式。
当前二元运算按左、右顺序急切求值，包括 and/or；左侧错误则右侧不执行。
本扩展不引入短路、不改变 RNG 消费、错误顺序、稳定选择身份或返回值。

证据每个条件最多记录 128 个节点、24 层、16 KiB 文本与字符串值，单段文本最多
2 KiB；同一选择组总计最多 512 个节点、64 KiB 证据文本与字符串值。超额只设置 `omitted:true` 或节点 `omitted`，不停止真实求值；
“证据已省略”和“未求值”不是同一状态。显示文本可截断，但不改写原表达式。
缓存不写入 Project、存档、ReplayTrace、运行观察或读者发布。
失败的编辑器试玩冻结该次尝试，明确显示原错误；作者可显式重新开始。
编辑器显示证据绑定当前运行及选择组；旧运行版本必须标明，不能混用最新编辑稿。

RPC 仅在 `session.explain_choices` 的 `include_evidence:true` 时返回实际缓存证据；
默认调用仍返回旧字段、不输出 evidence，未产生缓存时显式返回空 choices。
新增字段由宽容的 JSON 消费者按可选扩展读取；使用封闭外部 schema 的消费者需要
在选择启用该参数前更新 schema，不宣称所有外部消费者都无需适配。

## CLI 与 agent RPC

`wl play` 可指定 `--seed` 并将 trace 写至 `--trace-output`。`wl replay <入口> --trace-json '<DTO>'` 可指定 `--max-steps` 与 `--time-budget-ms`。RPC 的 `session.open` 可传 `seed`；`session.trace`、`session.checkpoint` 与 `session.explain_choices` 读取同一 runtime API；`trace.replay` 接收同一 trace/budget DTO。CLI/RPC 的参数或 DTO 结构错误属于调用错误，节点/选择不匹配、预算耗尽、取消或运行失败是结构化故事结果。

## 语言1.12锁定选择观察

采用enable的作品在ReplayObservation中添加可选choice_presentation数组，保存实际可见项及启用状态和作者说明。重放比较此投影（忽略源码行号），没有新能力的旧trace不增字段。选择身份加入enable条件、不加入禁用说明；说明与条件均参与运行指纹。choose_id/choose_presentation拒选禁用项时不记录步骤、不消费随机数或once；详见 [choices.md](choices.md)。

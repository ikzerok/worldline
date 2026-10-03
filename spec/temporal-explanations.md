# 偏序解释与环见证（工具 0.17）

本功能只解释已有 `during`、`within` 与显式 `follows`，不新增语言版本、日历、
日期运算或运行状态。core 是唯一求解者；CLI、agent、编辑器只转发或展示同一 DTO。
时间线仍遵守 [relations.md §7](relations.md) 的版本比较范围与 partial 保护。

## 1. 两事件比较

`Timeline::compare(left, right)` 返回 `TemporalComparison`：

- `left`、`right`：调用方给出的稳定事件 ID
- `relation`：`same_event`、`before`、`after`、`unordered_same_root`、
  `different_roots`、`invalid` 或 `unknown`
- `reason`：可空；`partial_timeline`、`unknown_event`、`missing_time`、
  `invalid_time_root` 或 `different_order_scopes`
- `evidence`：真实 `TemporalEdge` 数组，字段沿用 before/after/root/order_scope/file/line
- `status`：被查询时间线的 `complete` 或 `partial`

任一编译错误使比较为 `invalid/partial_timeline` 且 evidence 为空；即使某个局部
看似无环也不绕过全图完整性保护。不存在的事件为 `unknown/unknown_event`；已知
同一事件为 `same_event`，无须日期。不同事件若任一没有 during，为
`unknown/missing_time`。`Timeline.unplaced_events` 是按 ID 排序、去重的无 during
事件 ID 清单，用于区分缺失事件与缺失时间；既有 `events` 不加入虚假时段。

两事件都具有明确时段时，不同根为 `different_roots`；同根且同版本合法比较范围
内，仅沿显式边可达才是 `before` 或 `after`。`before` 的证据从 left 到 right；
`after` 的证据从 right 到 left，始终保持真实边的 before→after 方向。
同根无任一方向可达为 `unordered_same_root`，不能读作同时。1.9–1.12 中同根但
不同直接时段为 `unknown/different_order_scopes`，不借查询放宽旧版范围。
不明确的根或时段为 `invalid/invalid_time_root`。

证据选取最少边数路径；有多个最短路径时按逐边
`(before, after, file, line, order_scope, root)` 的字典序选择。使用稳定广度优先
遍历；不依赖源码声明次序、哈希遍历、rank、画布位置或 UI 排序。每条证据边都
逐字段来自同一 Timeline.edges，file/line 指向书写 follows 的真实后继事件头。
其他比较结果 evidence 为空，不制造隐含边。

## 2. 强连通分量与受阻下游

Timeline 新增两个向后兼容字段：

- `cycles: TemporalCycle[]`：每项含 `id`、`members`、`witness`
- `blocked: TemporalBlockedEvent[]`：每项含 `event`、`cycle_ids`

先在全部合法显式 follows 边上计算强连通分量。包含至少两个事件的分量或带
自环的单事件分量才是真环。members 按事件 ID 排序；id 是该分量最小成员 ID，
仅标识当前快照中的环，不能作为跨编辑永久身份。cycles 按 id 排序。

witness 是起终点均为最小成员、至少一条边的最短真实闭环路径；同长路径采用
上述字典序规则。它用于复核该分量存在环，不承诺遍历分量内所有成员或穷举全部环。
每条边保留真实 follows 位置。自环返回一条 before == after 的真实边。

blocked 只含从一个或多个环可达、但不属于任何真环分量的事件，按 event 排序；
cycle_ids 列出所有可以到达该事件的真环 ID，按 ID 排序。独立事件和上游前驱
不被误标受阻。一个环的下游另有真环时，后者仍归 cycles，不混入 blocked。

每个真环成员和受阻下游仍各有一个 A213 error，旧诊断数量和 partial 含义保持。
环成员消息明确“属于时间约束环”；下游明确“受时间约束环阻断”，不称其为环成员。
note 提供环 ID 与闭环序列，related 保留真实边源位置；不自动删除或修改约束。
跨根、缺失前驱、无时段及其他既有 A213/A219 规则保持。

## 3. 快照与消费者边界

Timeline、comparison、cycles 与 blocked 只描述生成它们的那次编译快照，不声称
可跨源码变化沿用。Project 机器查询必须与当前完整内容基线一起返回；提供旧
expected_baseline 时在查询前拒绝为过期，不能混合旧证据与新源码位置。编辑器在
缓冲编辑、外部刷新、撤销/重做或切换工程后清除旧比较、重新从当前分析查询；
源码导航按当前基线校验。修正错误约束并重编译后，旧 cycles/blocked/诊断消失。

查询只读，不自动解环、不写入日期、不变更 follows 候选保护、运行 fingerprint、
source lifecycle、清单能力、存档或读者发布边界。`at UINT` 保持原有顺序数字含义。

## 4. 回归证据

行为测试覆盖双前驱、传递链、同根无约束、独立根、同一与缺失事件、无时段、
嵌套/跨子时段、旧语言范围、二节点环及两个下游、独立多环、自环、环间依赖、
真实源位置、100 次稳定比较、修复后重编译与非时间错误下的 partial 保护。

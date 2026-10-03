# 真实路线对照与状态动作来源（工具 0.20）

本规范是 runtime、CLI、agent 与 editor 共用的只读作者投影契约。默认语言仍为 1.9，最高既有显式语言为 1.13；不新增 DSL。本次比较显式使用同一份当前已应用的 `CompileResult`，独立重放两条真实 `ReplayTrace`。不运行作者缓冲，不修改 live Story、RNG、工程或原 trace。

## 公共入口与生命周期

runtime 导出 `ROUTE_COMPARISON_SCHEMA_VERSION = 1`、`ROUTE_COMPARISON_CAPABILITY = "authoring.route_comparison.v1"`。

- `compare_routes(snapshot: &CompileResult, left: &ReplayTrace, right: &ReplayTrace, options: RouteComparisonOptions, cancellation: &ReplayCancellation) -> Result<RouteComparisonResult, RouteComparisonError>`：同步便利入口，复用下述合作式实现
- `RouteComparisonSession::new(snapshot: &CompileResult, left: ReplayTrace, right: ReplayTrace, options: RouteComparisonOptions, cancellation: ReplayCancellation) -> Result<Self, RouteComparisonError>`：验证输入并绑定源快照；不借用 snapshot，允许 native worker 与 WASM 跨帧持有
- `RouteComparisonSession::advance(&mut self, snapshot: &CompileResult, slice: ReplayBudget) -> Result<Option<RouteComparisonResult>, RouteComparisonError>`：每次让出返回 `None`，最终返回 `Some`；同会话不得更换快照，完成后不得继续推进。快照绑定包含源文件路径和全部源字节，并分别冻结与核对CompileOptions全部字段及analysis.fingerprint；source_snapshot非安全摘要不能替代选项与fingerprint的显式检查，不仅 runtime fingerprint

会话轮流推进左右两侧，分别保留自己的 Story 检查点、随机流、待完成输出、选择进度和瞬态证据；不从入口重新执行已完成语句。`options.budget` 是两侧合计解释器步数及从会话创建起算的墙钟预算，不是每侧各享一份。`slice` 是一次调用的合计上限；零 slice 让出且不推进。全局取消和总预算耗尽使尚未结束的两侧各自返回明确停止状态，已结束侧保留原结果。零总步数/时间预算仍可验证并建立起点（entry构造可能执行其既有enter效果），但不得推进正文；侧结果保留真实起点与实际效果，不伪装零动作。若令牌在创建前已取消，两侧仍可完成只读输入兼容校验但不创建Story，返回cancelled且states/vars为null。单条解释器语句、起点创建及检查点恢复不可抢占，不承诺硬实时。

CLI/RPC 是同步有界调用，沿现有单线程 agent 模型，不声称能用后来的 RPC 中断正在处理的比较。取消令牌由 runtime 与 editor native/WASM 使用；不新增 agent 作业数据库。

## 输入、预算与错误

`RouteComparisonOptions { budget: ReplayBudget, max_trace_bytes: usize, max_trace_steps: usize, max_output_bytes: usize, max_evidence_records: usize, max_evidence_bytes: usize }`，支持 `Default`。

默认值同时是可请求的硬上限：两侧合计 `budget.max_steps=100000`、`budget.time_budget_ms=30000`；每条序列化 trace `max_trace_bytes=4194304`（4 MiB）、`max_trace_steps=4096`；最终比较 DTO `max_output_bytes=1048576`（1 MiB）；每侧动作证据 `max_evidence_records=256`、`max_evidence_bytes=65536`（64 KiB）。调用方可减小，不得增大；0 表示零额度，不能关闭保护。单段证据字符串最多 2048 UTF-8 bytes，身份和来源超过该值时省略整条记录，不截断可导航身份。

新增报告构造先对借用的实际states/vars、覆盖、选择与动作记录执行流式计数/限额检查，再复制DTO或序列化；不得先构造巨大临时JSON/字符串再以len检查。报告的state/var/覆盖/选择记录总数最多16384；超过数量或输出bytes的整个结果返回output_limit，已执行动作证据另按其额度省略。合作式内部checkpoint及待比较观察亦在复制前做相同有界大小检查；超限停止比较并明确错误。这些是新增比较投影与检查点传输的资源边界，不声称限制整个解释器内部所有值、AST、历史或单语句求值的峰值内存。

实际运行输出另有两侧合计固定上限：32768 条输出和 1 MiB 序列化输出 bytes。在完整语句边界检查；超限停止未完成侧，标 `output_budget_exceeded`，不得将省略后的输出与原 trace 比较为成功。临时单条值的求值仍受解释器既有语义约束，不声称所有内存分配都可抢占。

`RouteComparisonError { code: String, message: String }`：code 为 `invalid_options`、`input_limit`、`invalid_trace`、`invalid_snapshot`、`snapshot_changed`、`session_finished` 或 `output_limit`；message 中文。trace/schema/runtime/checkpoint结构与兼容性错误是调用错误，任一无效输入使整个请求失败，不把错误伪装为一侧空结果。可执行故事失败、分歧、预算与取消属于结构化侧结果。`output_limit` 明确表示完整结果不能在请求额度中返回，不表示无差异；协议宿主为含请求 id 与换行的总响应另留有界 envelope 预算。

只接受完整已编译且无 error 的快照。trace未知可选字段沿既有 serde 读取兼容规则；新 options 不接受未知字段。不得篡改 trace/runtime_version 或移除观察来“修复”输入。

## 结果 DTO

`RouteComparisonResult` 包含：

- `schema_version: u32`、`runtime_version: String`、`source_fingerprint: u64`、`source_snapshot: String`（完整源集合的非安全摘要，仅用于过期检验）
- `left/right: RouteSideResult`
- `alignment: RouteAlignment`
- `state_differences/variable_differences: Vec<RouteValueDifference>`
- `differences_complete: bool`、`omitted: bool`

`RouteSideResult` 包含：

- `origin: RouteOriginSummary { kind: String, seed: u64, checkpoint_fingerprint: Option<u64>, checkpoint_digest: Option<String> }`、`original_fingerprint: u64`
- `status: RouteStatus`（snake_case 字符串枚举：`replayed`、`diverged`、`step_budget_exceeded`、`time_budget_exceeded`、`cancelled`、`incomplete_trace`、`story_failed`、`output_budget_exceeded`）
- `ended: bool`、`complete: bool`、`executed_steps: u64`、`completed_choices: usize`、`current_node: Option<String>`
- `detail: Option<String>`、`divergence_step: Option<usize>`（真实停止说明，不使用旧观察的值冒充当前状态）
- `states: Option<BTreeMap<String, Vec<String>>>`、`vars: Option<BTreeMap<String, Value>>`：本次真实运行停止值；起点创建失败时为 null，不伪造初始/终态
- `coverage: RouteCoverage { inherited: AccessCoverage, executed: AccessCoverage, total: AccessCoverage }`
- `state_actions: StateActionEvidence`、`omitted: bool`

只有 `status=replayed && complete && ended` 可以称“完整结束并验证通过”。`replayed` 且 incomplete/未结束只是已记录区段通过；其它均为真实停止状态。即便一侧失败，另一侧仍独立运行。两边终值相同不表示路线等价。

`RouteValueDifference { id: String, left: Option<serde_json::Value>, right: Option<serde_json::Value> }` 只比较当前实际 states/vars 的值，不做变量来源或全因果推导。null 侧表示实际字典无此 ID；整个实际字典不可用时不出伪差异并设 `differences_complete=false`。结果无法在总输出额度内完整表示时返回 `output_limit`，不得静默截成“没有差异”。

覆盖以次数相减：entry 起点 `inherited` 为空，本次入口进入计入 executed；checkpoint 起点刚恢复的历史全部进入 inherited，后续新增次数进入 executed，total 为二者之和。checkpoint 零新步骤不得把继承访问或选择称本段执行。未访问只表示本次未测试，不表示不可达。

## 真实可比前缀

`RouteAlignment { comparable: bool, reason: Option<String>, common_prefix: usize, first_difference: Option<RouteChoiceDifference>, left_verified_choices: usize, right_verified_choices: usize }`。

`RouteChoiceDifference { index: usize, left: RouteChoiceInput, right: RouteChoiceInput }`；`RouteChoiceInput { choice: ChoiceIdentity, source: Option<EvidenceSource> }` 必须取本次重放实际暂停组中确实选中的选择，不得直接取原 trace 的行号。

同一当前快照内，entry 仅在规范化 seed 相同可对齐；checkpoint 必须两侧均为 checkpoint、seed 相同且完整起点状态相同。entry/checkpoint 混合、不同 checkpoint 或不同 seed 保留并列事实，`comparable=false` 并解释原因。fingerprint 相同不能单独证明起点相同。

只沿两侧本次已验证起点后的实际选择序列，从零开始比较稳定 choice ID；遇到首个不同实际输入后停止，重复节点/循环按发生次序，不按文字或节点集合配对。如果一侧在下一选择之前停止，只报告已验证共同前缀，没有首差异不表示路线相同。某侧初始观察未验证或起点创建失败时不可对齐。两侧可比也只说明控制了起点与随机种子，不证明后续全部结果由单一选择独占导致。

## 有界瞬态状态动作证据

`StateActionEvidence { records: Vec<StateActionRecord>, total_actions: u64, omitted: bool }`；`StateActionRecord { sequence: u64, kind: ChangeKind, state: String, before: Vec<String>, after: Vec<String>, event: Option<String>, node: Option<String>, turn: u32, note: Option<String>, target: Option<TargetRef>, source: Option<EvidenceSource> }`。

sequence 从本次执行1开始，按真实发生顺序递增。仅记录实际执行成功的既有 `Become/AddTags/RemoveTags`，包括动态集合动作落到这三种操作的实际结果；同值写入、重复增加、移除不存在标签仍为独立动作。target 只查同快照 `catalog.states[state].target`，不新增世界所属语义。checkpoint既有 state_history不回填来源；重新恢复合作式执行的内部 checkpoint 不得重复记录已经发生的动作。

新增 `EvidenceSourceOwner::StateAction { node: String, action: ChangeKind, timing: String, effect_index: Option<usize>, action_index: Option<usize> }`，序列化 `kind:"state_action"`。正文/选择/条件/fragment动作 timing=`during`，效果为 `enter/exit/done` 并携带效果块与动作在正式AST中的索引。node为完整事件/场景或 `fragment:NAME`；效果node为定义所属事件，不错误使用离开时最内层场景或调用fragment。

`resolve_evidence_source` 扩展验证以上owner：正式 AST 的真实动作类型、所属、位置、效果索引及 parser 来源侧表必须一致，再以正式词法检查原文动作头，返回现有 `statement_header` 范围。动态动作验证正式动态AST及操作类型，不用实际 state/tag 值猜来源。无法唯一验证时source为空/解析错误，不回退静态候选、同名标签或当前打开文件。记录超过数量、字节或单字段限额时只设置 omitted，不改变真正执行、RNG、错误顺序或 state_history；省略不是 false、未执行或没有动作。

证据只在瞬态 runtime/比较对象中，不写入 SaveState、ReplayTrace、ReplayObservation、Project或发布。来源附属的文件/行/owner不改变稳定choice ID或runtime fingerprint。旧状态历史没有来源时不可回填。

## 源导航与兼容底线

编辑器沿既有来源安全桥核对同一已应用编译快照、完整源字节/内容基线、作者buffer/输入法、工作区边界及外部冲突。仅fingerprint相同不得复用旧行；注释移行或合法移动源码后必须显式重新比较，再导航当前真实位置。打开来源不应用、不保存，不替换固定参考，返回保留对照选择/滚动/焦点。

0.20严格拒绝runtime_version为0.19的trace与checkpoint，不提供跨版本迁移。普通Story Save继续原有schema/语言能力/指纹兼容规则，不因本功能额外拒绝旧Save。支持版本内原Save、trace和观察JSON形状不新增证据字段；纯定位变化继续现有重放语义归一化。

## CLI、RPC与验收

CLI：`wl route-compare PROJECT --left-trace-json DTO --right-trace-json DTO [--max-steps N] [--time-budget-ms N] --json`。RPC：`project.compare_routes {project_id, left_trace, right_trace, max_steps?, time_budget_ms?}`，initialize公布上述capability；使用当前Project一次编译的共同快照，不以两份旧session程序配对。宿主可暴露更低输出额度，但不得突破runtime硬限额。参数/结构/版本错误 CLI exit2 / RPC -32602；故事失败、分歧、取消或资源停止 CLI exit1，并保留结构化对照；两侧replayed（含明确部分区段）exit0。RPC返回 `{ok, comparison}`，ok仅两侧replayed；编译错误沿既有诊断故事层ok:false，不伪装协议错误。CLI磁盘入口使用新增core `Project::open_read_only(path)`：通过既有边界文件加载与`Project::from_snapshot`建立私有Project，不调用open/refresh/recover/migrate_permissions。它最多读取4096文件、64 MiB原始字节，有未完成保存事务时拒绝并保持所有字节不变；不修改普通Project::open的恢复规则。agent使用已打开Project当前缓冲，不refresh。两者都用`Project::compile_read_only(&self) -> Result<CompileResult,String>`：检查工作区诊断与未解决事务，再从当前已加载缓冲编译，禁用磁盘include回退，不迁移/修改缓冲。任何工程或作者文件写入都不属于本接口。

最低验收覆盖：同节点集合但choice/state/vars不同；同值重复动作、循环、fragment和enter/exit/done顺序；不同seed/checkpoint不可对齐；checkpoint零新step继承边界；单侧分歧与故事失败；partial、不完整观察、全局步数/时间/输出预算和两侧取消；合作式与同步一致；超长字符串/动作记录省略；输入/响应总额度；纯注释移行/合法移动后回源；错误owner/行/动作类型拒绝；Save/trace/观察形状与RNG不变；0.19 trace/checkpoint拒绝且普通Save不扩大拒绝。

## 实施与资源采样门

实现完成前须在同一全新fixture的50/200规模及一份限额压力样本上记录：源码bytes、state/var/节点/选择数量、输入trace bytes/steps、实际解释器steps、实际输出条数/bytes、动作总数/保留数/omitted、最终DTO bytes、墙钟耗时与可取得的进程peak RSS。同步与合作式分别采样并核对同一结果；预算停止与预先/执行中取消分别验证两侧状态、合计步数和响应时间。报告包含OS/架构、debug或release、工具版本、单次还是重复次数以及测量方法；不可把单次debug结果当性能承诺。native与WASM的单语句不可抢占边界须保留；未实测平台明确标未测。构建或采样发现显著资源回归须先解决/说明再通过实现门，不能以UI截断掩盖runtime无界分配。

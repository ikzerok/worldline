# 普通演练的有界推进（runtime.bounded_continue.v1）

普通试玩与重放均不得在一次推进中无界解释循环。本能力不新增语言关键字，
不改变执行顺序、选择身份、旧存档字段或运行指纹；预算是宿主执行策略。

## runtime 契约

`Story::continue_story_bounded(ReplayBudget, &ReplayCancellation)` 返回
`Result<BoundedContinuation, RunError>`。成功值含 `outputs`、`outcome`、
`executed_steps`。`ContinuationOutcome` 序列化为 `choice`、`ended`、
`step_budget_exceeded`、`time_budget_exceeded` 或 `cancelled`。
后三项表示可恢复的执行暂停，不是选择暂停或正常结束，也不保证发现无限循环。
`is_paused()` 仍只表示等待选择；预算暂停时通常 `paused=false, ended=false`。

默认普通预算 `DEFAULT_CONTINUATION_BUDGET` 为每次 100000 步、250 毫秒。
`set_continuation_budget` 可调整旧 `continue_story()` 的预算；显式有界调用使用传入预算。
合法长稿可提高预算，零步或零毫秒表示立即暂停，不表示关闭保护。
预算每次调用重新计量；时间使用单调时钟，仅累计此次推进，不累计作者思考时间。
`time_budget_ms=u64::MAX` 明确关闭墙钟上限，步数预算仍必需且保持有限；
确定性测试可仅使用步数界，避免把机器速度当故事语义。
先检查取消，再检查时间和步数。步数计算解释器迭代，包括帧结束处理；已经等待选择
或已经结束时直接返回对应结果，不需要预算。单条语句、表达式、选择组求值和该次
转场包含的 enter/exit/done 效果不可抢占；启动初始化与存档校验也不属推进预算。
因此时间预算是合作式保护，不是严格实时或任意大小输入的资源隔离保证。

暂停发生在完整语句之间。已产生输出仅在此次 `outputs` 返回，游标、局部变量、
状态历史、访问数、once、RNG 和粘接标志保留；续行不重新进入节点、不重算已完成
随机表达式、不重放已提交效果。取消令牌保持取消状态，恢复应使用新令牌。
`save/load` 可保存该语句边界，无新增存档字段；预算策略不入档，载入恢复默认预算。
输出属于宿主显示记录，不写入存档。明确重启才重置状态与随机流。

旧 `continue_story() -> Result<Vec<Output>, RunError>` 保持签名与正常结果。
超限返回带当前位置和中文提示的 `RunError`，已执行状态仍保留；该次尚未返回的
输出由 `take_interrupted_outputs()` 取出，或自动合入下一次继续的返回值，二者只交付一次。
旧调用方在保存或放弃会话前若需显示部分输出，应先取出；新宿主优先使用显式有界接口。
真正表达式/准入失败仍为原 `RunError`，不得将其标为可恢复的预算暂停。

分片推进的 trace 仅在达到下一选择或结束时提交完整观察，期间累积同一段输出，
不把预算边界误记成故事选择。中途导出轨迹仍不完整；重启清空待完成观察，从当前
检查点重新开始 trace 时也清空旧段输出。完成后的 trace 与未分片执行等价。

## 执行用途诊断

普通编译的历史资料不因仅有正文或时段而被迫补 END。显式试玩调用
`worldline_core::analysis::execution_diagnostics` 取得该执行入口的额外 A202 提示；
自然结束仍合法。CLI play 将这些中文提示写至 stderr，不增加旧 JSON 行或字段。
RPC 仅在协商本能力后的 session.open 返回 `execution_diagnostics` 数组；
它不合并为 compile 的全局结论，也不阻止会话创建。

## CLI

`wl play` 无论人类或 JSON 模式均使用有限预算。可选
`--max-steps N`、`--time-budget-ms N` 设置每次推进预算，支持 `--key=value`。
人类模式暂停后明确提示可使用 `--save=...` 保存并以 `--load=...` 续行，退出码 1；
不自动重试同一预算，否则无条件循环仍会令整个命令无界。

JSON 仅显式提供 `--bounded-continue` 时在 turn/ended 中附 `outcome`、
`executed_steps`。预算暂停返回 `type:"suspended"`、`ok:false`、上述字段、
`outputs/choices/state`，退出码 1。旧模式保持正常 turn/ended 字段不变；
超限使用 `type:"run_error"`、`ok:false`、中文 `message/node/line` 以及
部分 `outputs/choices/state`，不会伪装成 ended，也不会丢弃已提交状态。
`--save` 对两种模式的暂停都保存当前语句边界；提高预算后可载入继续。

## JSON-RPC

`initialize.capabilities` 宣告 `runtime.bounded_continue.v1`。
`session.open.capabilities` 显式申请；响应只确认支持的能力，与
`runtime.choice_presentation.v1` 可独立或同时协商。未申请时正常结果字段不变。

协商后的 `session.open` 可设置 `max_steps/time_budget_ms` 作为会话默认预算；
`session.continue` 可用同名参数覆盖本次预算，省略使用会话默认。
未协商而传这些新参数为 `-32602`。类型错误、负数和非整数也是协议错误。
协商后的继续结果附 `outcome/executed_steps`；达到选择或结束仍是原正常结果，
预算/取消暂停额外附 `ok:false`，保留此次 outputs 和当前 choices/state。
未协商会话也始终受默认预算保护，超限返回 `ok:false/run_error` 和
`outputs/choices/state/paused/ended`，不用 JSON-RPC error。

协商后的 `session.cancel` 返回 `{cancel_pending:true,state}`，标记下一次继续取消；下一次 `session.continue`
返回 `outcome:"cancelled"`、零步且不改状态，再次继续可恢复。
若当前已是选择或结束，继续优先返回该既有状态。stdio 仍顺序处理，取消请求
不能抢占已开始的同步请求；有界请求返回后才处理下一行，不能宣传传输层即时取消。
`session.restart` 清除待处理取消，预算策略保留。参数和能力不写入语言或存档。

工具0.31的[state-inspection.md](state-inspection.md)只读检查可查看预算、取消或错误
之后的当前部分状态；上一观测仍是最近真正完成的trace observation，不能把本次预算
暂停算成新观察。已经记录的选择/结束边界则对比严格前一真实观察；缺基线明确不可比较。

# 已验证的试玩审阅记录（工具 0.28）

本规范定义独立的作者交接副本，不是读者发布、可执行播放器或完整世界导出。
输入为同一当前已应用 `CompileResult` 和一条不可信 `ReplayTrace`。runtime 重用既有
重放解释器、选择身份、观察比较与合作式预算；只有真实执行且与记录匹配的观察边界
可以进入已验证正文。不得把传入 trace 的输出、选择标签、来源或状态直接排成可信记录。
core 只提供正式 AST/源字节的来源投影，不依赖 runtime。

## 入口与生命周期

runtime 导出 `PLAYTHROUGH_REPORT_SCHEMA_VERSION=1` 和
`PLAYTHROUGH_REPORT_CAPABILITY="authoring.playthrough_report.v1"`。

- `generate_playthrough_report(&CompileResult, &ReplayTrace, PlaythroughReportOptions, &ReplayCancellation)`
  返回 `Result<PlaythroughReport, PlaythroughReportError>`，复用合作式实现
- `PlaythroughReportSession::new(&CompileResult, ReplayTrace, PlaythroughReportOptions, ReplayCancellation)`
  建立只读验证；`advance(&CompileResult, ReplayBudget)` 返回 `None` 让出、`Some(report)` 完成
- 每次推进核对全部源路径、源字节、编译选项和运行 fingerprint；不只相信展示摘要
- 完成后不再推进；取消、预算耗尽、分歧和未完成记录均是明确停止状态，不等于完整通过
- 创建前已取消不创建 Story；零总预算不推进正文；单语句、起点恢复和摘要计量不可抢占

`PlaythroughReportOptions` 为 `budget: ReplayBudget`、`max_trace_bytes: usize`、
`max_trace_steps: usize`、`max_output_bytes: usize`。默认值同时是上限：100000解释器步、
30000ms、4MiB输入、4096选择、1MiB最终DTO（包含Markdown）。只可降低，零不会关闭保护。
实际输出另外受既有32768条/1MiB限额；检查点/状态在克隆前借用计量。来源快照限4096文件、
路径及源字节合计64MiB；观察/来源投影也在复制前检查记录数和字节。拒绝超额成功产物，
不声称能抢占表达式内部的任意临时分配。Markdown按最终转义后的字节限制流式生成。

错误 `PlaythroughReportError { code, message }` 使用英文code、中文message，code 包括
`invalid_options/input_limit/invalid_trace/invalid_snapshot/snapshot_changed/session_finished/output_limit`。
结构/版本/检查点不兼容是调用错误；真实分歧、故事失败、取消与执行预算是报告状态。
没有初始观察的trace不能生成已验证正文，标 `incomplete_trace`；不执行未验证输入链。

## 产物与信任边界

`PlaythroughReport` 包含版本、编译选项、实际请求额度 `limits`、当前/原始fingerprint、源快照摘要及逐文件manifest，
显式起点种类/seed/entry或checkpoint摘要、checkpoint继承覆盖计数、验证范围、结束状态、
已验证观察、实际执行步骤/选择次数和 `markdown`。

来源manifest是本次已应用活动源码集合：相对文件路径、UTF-8字节数及FNV-1a-64内容摘要。
摘要仅供版本识别，不是密码学签名、真实性保证或跨信任域防篡改机制。路径基准是输入源码
集合的共同父目录；机器绝对路径不进入DTO/Markdown。根目录本身不公开；不生成file://链接。
无法安全形成相对路径或缺少源码时拒绝，不回退到basename冒充唯一来源。

验证时间点由本次绑定的完整快照以及实际生成的Unix毫秒时间标识；时间读不到时为null并
明确说明，不伪造日期。起点为checkpoint时只验证继承状态之后的区段，之前的覆盖为继承，
不伪装为本次已验证入口路线。当前稿标签与speaker展示名明确来自验证快照。

正文按真实观察次序保留实际文本、换行/粘接语义、speaker身份/展示名及选择。
来源由真实执行语句/实际选择证据和同一core快照确认，精度为语句位置；不能以暂停后的
当前节点反推先前每条输出位置。缺失或歧义的来源明确不可用，不猜测。
只有匹配成功的观察输出进入正文；后续观察分歧时保留此前已验证前缀，并明确停止位置。
最后选择后无观察时可保留实际已选输入，标其后正文未验证；不能捏造后续内容。

默认不含变量名/值、任意state JSON、调用locals、作者备注、对象资料、标签元数据、附件或
可点击外链。正文/选择/speaker/源码相对文件名本身仍可能含私人作者内容。实际全局赋值
与状态操作仅提供经验证区段内的次数（同值写入仍计数），不把终值比较冒充写入证据。
入口初始化变量不计为正文赋值；checkpoint之前操作不重建。未访问内容统一为“未探索”，
不等于错误、不可达或完整覆盖。

完整成功需 `status=replayed && complete && ended` 且初始和全部已录观察验证。
未结束或未声明完成是已验证部分区段；diverged/story_failed/step_budget_exceeded/
time_budget_exceeded/cancelled/output_budget_exceeded/incomplete_trace 必须显式展示。
运行错误不原样输出可能携带绝对路径或私人值的message，报告使用安全的通用停止说明。
所有动态文本均转义HTML与Markdown元字符、控制字符和换行边界，不提供原生HTML执行通道。

## CLI、RPC、编辑器

`wl playthrough-report <入口> --trace-json '<DTO>' [--max-steps N] [--time-budget-ms N] [--json]`
默认输出同一Markdown；`--json` 输出同一DTO。命令本身是显式作者审阅导出，内容包含固定
私密范围提示。RPC `project.playthrough_report {project_id,trace,max_steps?,time_budget_ms?}`
使用同一生产者；成功返回 `ok:true,report`，真实未通过也保留报告状态；协议错误按既有
JSON-RPC规则，编译失败为ok:false。不新增异步作业或声称后续RPC能中断同步请求。

编辑器由当前或选定真实路径显式进入预览，显示范围、起点、当前稿/排除未应用草稿、结果
及私密内容说明，再显式确认复制/保存。native保存新Markdown文件，Web沿既有下载 helper；
不写入工作区，不覆盖旧文件。导出前重新核对已应用版本、完整内容基线和来源观察；过期
必须重新验证，不自动应用/保存草稿。取消、预览、生成、复制和保存不得改变live Story、
随机流、源稿、个人选择路线或保存基线，也不扩大reader白名单授权。

回归覆盖：入口完整/暂停/末选无观察、checkpoint继承、来源移动、失配与伪造观察/标签/行号、
旧runtime、零预算/取消/超额、中文长文/emoji/HTML/Markdown、同值赋值、跨文件正文来源、
speaker当前稿、重复生成、导出前外改/未应用草稿、源码和live save字节不变。

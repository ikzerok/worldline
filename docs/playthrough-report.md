# 试玩路径报告（0.28）

作者可把一条真实试玩路径交给 runtime，在当前已应用稿上重新验证后生成 Markdown。
这是一份作者审阅副本：正文、选择、说话者及源码相对文件名仍可能含私人信息，应在分享前
核对接收者与全文。报告不会自动公开对象资料、变量值、调用局部值、作者备注或附件；也不
代替“发布给读者”的独立选择与审核。

## 命令行

```sh
wl playthrough-report ./作品 --trace-json '<ReplayTrace JSON>'
wl playthrough-report ./作品 --trace-json '<ReplayTrace JSON>' --json
wl playthrough-report ./作品 --trace-json '<ReplayTrace JSON>' --max-steps 10000 --time-budget-ms 1000
```

第一种输出 runtime 生成的同一 Markdown；`--json` 返回 `{ok,report}`。需要保存时，使用
操作系统重定向并自行选择工作区外的新 `.md` 文件；命令本身不写文件、不推进保存基线。
CLI 使用当前磁盘稿的只读 Project 快照，不修复保存事务，不更改 trace。

退出 0 表示记录已受控验证（`status=replayed`），不一定是完整结局。只有报告同时满足
`complete=true`、`ended=true` 才是已完整结束并验证通过。尚未结束的 live 路径可产生已验证
部分报告。退出 1 表示编译失败或真实验证停止（分歧、预算、取消、缺少观察、运行失败等）；
有已验证前缀时仍保留明确标记的报告。参数/trace结构或版本不兼容/输入过大/IO失败退出 2。

报告注明当前及原始 fingerprint、当前源码快照摘要/manifest、runtime版本、起点与seed、
生成Unix毫秒时间、实际观察/选择/解释器步骤及验证范围。摘要是版本标识，不是密码学签名。
检查点起点只验证检查点之后区段；继承覆盖不等于本次已经从入口运行。未探索分支不代表
错误、不可达或已验证。分歧观察中的不可信输出不会进入“已验证正文”。

## RPC

先以 `project.open` 打开工程，再调用：

```json
{"jsonrpc":"2.0","id":2,"method":"project.playthrough_report","params":{"project_id":"p1","trace":{},"max_steps":10000,"time_budget_ms":1000}}
```

上面的 `trace:{}` 仅标示输入位置，须替换成完整实际 `ReplayTrace`。RPC读取已打开工程的
已应用缓冲，不刷新外部磁盘，也不触碰 live session。业务结果与 CLI JSON 同义；未知字段、
错误类型、无效trace与超上限预算是 `-32602`，故事/编译失败是正常 `ok:false` 结果。
`initialize` 声明能力 `authoring.playthrough_report.v1`。调用同步且有界，不声称后续RPC能
取消正在处理的请求。UI使用同一合作式runtime会话提供取消。

默认上限为 100000解释器步/30000ms、4MiB trace/4096选择、1MiB最终报告DTO，机器外壳另留
4KiB；含转义的整条响应和巨大的请求id也受保护。额度只能降低，零仍是有效的停止预算。

完整契约见 [试玩报告规范](../spec/playthrough-report.md)。

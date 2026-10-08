# 查询范围巡检用法

资料组合查询可一次生成完整、不可变的世界资料范围。它将相同内容基线上的命中对象、地图逐处绑定和正式关系放在同一 typed 快照；完整契约见 [catalog-scope](../spec/catalog-scope.md)。既有 `catalog-query`、游标与共享查询文件保持兼容。

## Rust

```rust,ignore
let scope = project.catalog_scope_snapshot(&query, 10_000)?;
let page = scope.query().page(0, 50)?;
let next = page.next.as_ref().map(|cursor| scope.query().continue_page(cursor));
let positions = scope.placements_for(&target).collect::<Vec<_>>();
let relations = scope.query_relations(&target, Default::default());
```

完整范围构建只编译一次。调用快照自身的 `page` / `continue_page` / `query_relations` 不读文件、不编译；Project 修改后，消费者必须比较来源基线并显式重建，不能把冻结坐标联接到新内容。`validate_for` 可检查传输 DTO 与请求身份、计数和预算，不能据此获得写入权限。

`query().total()` 是完整命中数；`counts().matching_placements` 是逐处绑定数；`unplaced_objects` 在地图来源不完整时仅表示无已知位置。对象身份使用完整 `TargetRef`，同名不会合并。关系节点角色分为 Match / ContextOnly / Unresolved，正式边保留真实方向、关系 ID 与来源。超过局部显示预算的关系通过 continuation 继续，不改变原查询集合。

## CLI

```sh
wl catalog-scope ./作品 --query '{"schema_version":1,"filters":[{"dimension":"kind","values":["entity"]}]}' --json
wl catalog-scope ./作品 --query '{"schema_version":1}' --focus '{"kind":"entity","id":"harbor"}' --relation-options '{"depth":2,"direction":"both","max_nodes":250,"max_edges":500}' --json
```

当前页可用 `--offset` / `--page-size`，完整 scope 始终在成功响应中返回。此命令不接受旧 query cursor；需要多次内存分页的程序应保留 Rust 快照。独立路径使用只读打开，不保存或恢复磁盘事务。

## RPC

```json
{"jsonrpc":"2.0","id":1,"method":"catalog.scope","params":{"path":"./作品","query":{"schema_version":1},"page_size":50,"focus":{"kind":"entity","id":"harbor"}}}
```

`path` 和已打开的 `project_id` 二选一。成功返回完整 scope、结果页与可选局部关系，来源错误以 incomplete/diagnostics 保留，不假称空作品。结构错误用 -32602，查询、IO、预算等业务错误为 `ok:false`。

`project_id` 只读取会话已应用的 Project 快照，不隐式刷新磁盘或恢复保存事务。外部文件已经修改时，先按明确意图调用已有 `project.analyze` 更新会话，再运行 `catalog.scope`；单独查询继续返回原会话内容。`path` 用只读打开，有未完成保存事务时拒绝。成功响应的 `source_mode` 区分两条来源，`refreshed:false`、`conflicts:null` 表示本方法没有主动确认或处理外部冲突，不能当作实时磁盘状态。

请求参数最多 4 MiB，RPC id 最多 3072 编码字节，完整响应行最多 32 MiB。构建与传输超限拒绝完整范围，不静默截断。范围不写作品、不新增持久集合、不扩大读者公开选择、不改变 runtime 指纹。

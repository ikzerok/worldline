# 同一筛选范围的作者审稿本

工具0.32将已有单书稿查询接入连续全分支审稿和一种实际交付格式：Markdown。它是作者私密材料，不是运行记录、公开阅读站或完整工程备份；不会改变读者发布白名单。

## 核心使用

先创建一次 `Project::manuscript_query_snapshot`，再用同一个 `Arc<ManuscriptQuerySnapshot>` 创建 `ManuscriptDeliverySnapshot`。查询快照已经保留该次真实编译结果，逐章审稿和后续分页都不会再次编译源码。

`ManuscriptDeliveryRequest::new(query)` 默认选择该书稿文字、状态、POV、分节筛选的全部匹配章。树折叠、当前导航页和选中行不会缩小范围。可将 `chapter_ids` 设为明确稳定章ID数组；最终仍按书稿读序输出，重复、歧义和不在筛选中的ID会失败。`None` 是全部匹配，空数组是零章。`expected_snapshot_key` 可绑定刚核对的查询页快照，防止无声切换版本。

`ManuscriptDeliveryJob::advance(1)` 每次处理一个编排出现并返回真实进度；`cancel()` 后没有新材料。同步消费者使用 `generate_manuscript_delivery(snapshot, callback)`，callback返回false即取消。正文与编排草稿只覆盖只读候选，不会应用、保存或改变撤销状态。

结果包含完整范围、稳定章/目标身份、分节路径、全书位置、已纳入文件草稿的代次与字节数、重复源次数、逐章审稿或错误、累计预算和可选Markdown。同一源出现两次仍是两个编排出现，正文在材料中保留两次。坏章、缺源或编译错误不按零字丢弃；结果为 `complete:false`，不提供可交付Markdown。

## CLI

只读生成已应用工程稿的同范围报告：

```sh
wl manuscript-delivery /作品 \
  --request-json '{"schema_version":1,"query":{"schema_version":1,"manuscript_id":"book","text":"灯塔","status":"review","pov":"character:lin","section_id":"part_one"}}' \
  --json
```

明确导出为工作区外的新文件：

```sh
wl manuscript-delivery /作品 \
  --request-json '{"schema_version":1,"query":{"schema_version":1,"manuscript_id":"book"},"chapter_ids":["opening","ending"]}' \
  --output /交付/本次作者审稿.md --json
```

`--drafts-json` 沿用现有绑定完整工程基线的编排草稿DTO。正文WritingBuffer由Rust/编辑器提供；CLI不会猜测其它未提交编辑器输入。目标必须是全新`.md`，不会覆盖已存在文件。输出声明 `applied:false`、`saved:false`；`delivered:true`只表示该次明确原生导出成功，不表示工程已保存。

## RPC

```json
{"jsonrpc":"2.0","id":1,"method":"manuscript.delivery","params":{"project_id":"p1","request":{"schema_version":1,"query":{"schema_version":1,"manuscript_id":"book","status":"review"}}}}
```

返回与Rust/CLI相同的报告和Markdown字符串；RPC不写文件，也不授予任何来源跳转或公开权限。非法DTO返回协议参数错误，过期、超预算或不完整等业务失败返回 `ok:false`。输入最大4MiB，完整响应最大32MiB，不进行成功状态下的截断。

## 静态语义与边界

- if/else互斥分支、各选择、say、场景、结构、call/return与去向均保留；条件、随机及状态不执行
- call保留未展开说明，不夹带调用目标全文；动态文字使用未求值标记
- Markdown只包含选中章的正式审稿语义、范围和身份，不打包源文件、物理路径、隐藏JSON、源码注释、批注、运行值或附件
- 预览、复制、文件交付使用同一个core字符串的UTF-8字节，无第二次拼装或行尾转换
- 默认且最大：4096章、100000节点、4MiB范围身份、16MiB审稿投影、8MiB Markdown；调用方可降低，不能静默钳制。4096是该交付能力的独立预算
- 交付前调用 `Project::validate_manuscript_delivery` 核对全部当前正文/编排及磁盘观察；变更后必须重新生成
- 原生先同步目标目录的暂存文件，再无覆盖原子发布；失败清理暂存。浏览器下载请求不证明已落盘

完整合同见 [manuscript-delivery.md](../spec/manuscript-delivery.md)。

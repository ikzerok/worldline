# 逐处审阅后安全改稿

适用工具版本：0.23。默认语言版本及作品语义不变。

## 作者流程

1. 选择当前文稿、捕获的正文选区或明确的工程文件，输入查找文字
2. 读每处以实际命中为中心的上下文，区分当前导航项与勾选项
3. 只选要改的措辞；例如改客观叙述，保留人物原始证词中的相同文字
4. 预览已选项的真实前后片段及数量，确认后一次应用
5. 当前稿仍留在唯一草稿缓冲；跨文件作为一次 Project 内存事务提交
6. 需要时撤销/重做，确认内容后再保存；预览、取消与应用都不会自动保存

选择依赖当前的原文字节与草稿代次。查看来源再返回不会使有效审阅失效；实际修改来源、切换查询/选项/范围或改变替换计划后，应重新预览。受保护声明、引用、表达式、稳定行 ID、显式链接等可以在源码范围查到，但不能通过措辞替换修改。

## core 接口

既有 `search_drafts`、`preview_search_replace`、`apply_search_replace` 与 `replace_search_draft` 保留。旧预览入口仍预览范围内全部命中，含任一受保护命中即整批拒绝。

逐处替换使用 `Project::preview_search_replace_selected(request, drafts, selected)`。`selected` 是 `search_drafts` 返回的 `SearchMatch` 的克隆集合，不是行号、列表下标或调用方新构造的范围。选择顺序可任意，core 按搜索文件顺序及原文位置规范化。重复命中、空选择、只读导航位置、伪造元数据、过期来源和受保护项均拒绝；不静默跳过。

`SearchMatch.identity` 是不可自行生成的可选身份；真实搜索结果始终提供，纯导航位置可为 `None`。完整源快照共享存储，最终核对精确字节而非仅核对摘要。`SearchMatch.context` 给出真实片段及其局部强调；旧 `preview` 字符串也改为有界片段。

```rust
let hits = project.search_drafts(&request, &drafts)?;
// selected 来自作者勾选的 core 命中；这里只示意选择其中两处。
let selected = vec![hits[0].clone(), hits[2].clone()];
let plan = project.preview_search_replace_selected(&request, &drafts, &selected)?;
// 向作者显示 plan.hits / plan.occurrences，再根据作者确认应用。
project.apply_search_replace(&plan, &drafts)?;
```

应用前 core 重建同一集合的计划。`ReplacePlan.hits` 只含实际选中项；`changes` 是文件完整候选；`occurrences` 与 `hits` 同序，每项提供：

- `path`
- `before_range` / `after_range`：前后完整文稿的 UTF-8 字节半开区间
- `before_context` / `after_context`：源自真实前后候选的有界上下文

同一行多处更改时，修改后区间包括先前选中替换造成的真实偏移。替换文本即使再次包含查询词，也不会重复处理新文字。清空替换文字时，修改后强调区间可为零宽。

当前稿用 `replace_search_draft` 返回新的 WritingBuffer；调用方接管旧/新快照以实现撤销。跨文件用 `apply_search_replace` 作为一个 Project 事务，并沿用 Project 快照恢复。调用方不得将这两种路径重复应用或自行解析正文保护规则。

## 上下文与规模边界

每处最多 160 个 Unicode 标量、640 个 UTF-8 字节，通常保留命中前最多 48 个标量，优先完整展示可容纳的命中。长行第 253 列以命中为中心；同一行多处命中各有真实强调。上下文不添加省略号、不归一化 CRLF，也不改写 emoji 或组合字符。

`SearchContext.source_range` 指向整份源，`highlight` 指向 `text` 内的真实命中交集。`omitted_before` / `omitted_after` 对单行命中描述该行省略，对跨行命中描述文档省略；`match_omitted_before` / `match_omitted_after` 单独说明超长命中是否被截取。`grapheme_clipped` 说明窗口边缘截断了字素簇，不应把它显示为完整字素。提示与省略号绘制在源文之外。

最多 10000 处命中，超限明确报错，应缩小查询范围。限制以内所有结果都应可导航和选择；分页或滚动不改变选择身份。查找、预览和应用均不修改磁盘；保存仍沿用工作区的冲突检查与可恢复事务。

完整契约见 [search-replace](../spec/search-replace.md) 与 [workspace](../spec/workspace.md)。

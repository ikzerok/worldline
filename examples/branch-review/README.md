# 第十三次退潮：静态全分支审稿示例

四份 `.wl` 源码、三位人物和四种结局。`.world/manuscripts/tide_review.json` 只编排章节，不复制正文。书稿把 `dawn` 放在第一章，便于检查互斥结局的阅读边界；实际运行仍从 `arrival` 开始。

```sh
wl check examples/branch-review --json
wl manuscript-review examples/branch-review --target-json '{"kind":"event","id":"dawn"}' --json
wl manuscript-review examples/branch-review --target-json '{"kind":"event","id":"wharf"}' --json
wl play examples/branch-review
```

试玩按 1→1 会公开日志并营救；按 2→2 会隐藏日志并保留照灯燃料。审稿看到的四个条件正文均是静态候选，不表示它们依次发生。人物“罗沛”的稳定ID仍为 `luopo`，台词的姓名与正文显式链接标签分别来自其正式来源。

在worldedit打开此目录，进入书稿工作台，查看“天明以前（四种结局）”和“最后一罐燃料”。可在当前章节或整书审稿中检查条件、选项、人物身份及段落来源；修改正文后预览取当前稿，坏稿或陈旧来源应明确拒绝，不能自动应用/保存。

公开分享应单独选择读者站对象、章节、字段和附件。本目录包括作者完整源码，不是脱敏阅读包。

# WP-16 批注与提案协作验收证据

对应 worldline #12，覆盖 WL-023/024、WE-020/021、Q-009。

## 已实现

- `CommentDocument v1` 由清单注册，可锚定完整 `TargetRef`、地图标记或带原文/hash 的正文行范围。
- 对象/关系和地图检查器可直接打开批注草稿；正文范围可在协作审阅页指定。
- 正文或标记不再精确存在时批注明确进入 `detached`，不会自动绑定邻段或同名对象；作者可重新指定。
- 对象 ID 跨视图重构会同步更新对象批注锚点；直接对象批注进入删除影响并阻止未处理删除。
- `ProposalDocument v1` 分开记录 content / presentation 文件的 base 与 proposed。
- 提案保存不应用修改；采纳前重新进行三方预览。
- JSON 对象不同稳定键可合并；同字段、删改、数组并发修改与正文并发改写显示冲突并整批拒绝。
- 成功采纳后才将状态写成 `accepted`；作者署名与状态只是团队记录，不构成权限控制。

## 自动验证

- core `collaboration` 回归覆盖对象/标记/正文锚定、失锚重绑、删除保护、不同字段合并、同字段/删改/数组冲突、内容/版式分类。
- core `refactor` 回归验证实体稳定 ID 重命名同步批注锚点。
- worldedit egui 回归验证真实“保存批注”“保存修改提案”“采纳提案”按钮路径；冲突提案保持 open 且工程基线不变。
- 全量 fmt/test/clippy/release 与 WASM 检查在 WP-17 配对验收中再次执行并记录。

## 契约

规范位置：`spec/presentation.md §8.1`、`spec/diagnostics.md` 的 COLLAB00x、`spec/schemas/comment.schema.json` 与 `proposal.schema.json`。

协作文档属于 Project 展示文档生命周期，保留未知可选字段；未知必需能力只读。批注和提案均不进入 Story Program 或运行指纹。

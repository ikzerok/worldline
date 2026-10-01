# 镜海孤岛：显式语言1.12完整例子

用worldedit打开本目录，或运行 `wl check examples/lantern_archive_112`。本作品显式启用1.12，旧工程不会因此升级。

- `schema city` 约束镜海港资料；population=0、public=false、motto空串均是实际存在值。把population改成字符串、漏founding_year、让mayor指向place或拼错属性键，可在core统一诊断中看到错误
- 编辑器“资料与状态”→“持续资料约束”可编辑字段及绑定，先看影响计划，再应用；修改字段键不会自动迁移实例值
- 首次试玩仍看见“进入旧档案室”，但它不可点且给出作者说明。领取钥匙后才可进入；once仅在真正选择后消耗，片段内可以暂停、保存与重放
- 书稿工作台可在未应用正文中查找和预览；当前章节与整书范围明确分开，源码/结构/正文共用文件缓冲
- “发布给读者”必须显式选择对象、章节、属性，引用不会扩大公开范围。静态阅读包不执行锁定条件，不自动公开作者资料或禁用说明

CLI新消费者可用 `wl play examples/lantern_archive_112 --json --choice-presentation --seed 42`。JSON的choices仍只含可选项；新增choice_presentation才含禁用项及说明。stdin编号仍遵守旧可选索引，不用presentation下标替代。

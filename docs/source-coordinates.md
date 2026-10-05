# 当前源码坐标与可信定位

工具0.26通过 `source_coordinates` 提供独立于AST的文本坐标。适用于普通源码当前光标、作者输入行列后的预览和确认；语法未完成仍可使用。当前状态与平台覆盖见[0.26版本说明](https://github.com/ikzerok/worldedit/blob/main/docs/releases/v0.26.0.md)。

## 使用顺序

1. 对精确当前正文建立 `SourceCoordinates`，复用时仍提供相同正文
2. `position_at_character` 接受零基整文Unicode标量索引，`position_at_byte` 接受零基UTF-8字节索引；返回的行列为1基
3. 调用 `Project::preview_source_jump`，传入来源路径、精确当前正文及作者请求；显示core返回的位置、允许范围和有界上下文
4. 真正跳转之前调用 `Project::resolve_source_jump`，再次传入仍然准确的当前正文；只消费成功返回的零宽UTF-8范围
5. 任何失败都保持作者稿和现位置，由作者核对新预览后再确认，不能用旧偏移补救

公开预览字段用于呈现，不是可反序列化的跳转授权。core重新检查身份、来源、基线、刷新代次、当前正文及完整预览一致性，篡改或过期不会悄悄执行。

## 重要区别

- 物理行与字符列不等于软折行、屏幕宽度、UTF-8字节或UTF-16单位
- 行列定位只解释文本位置；[本文件结构](https://github.com/ikzerok/worldline/blob/main/docs/source-outline.md)才解释正式声明归属
- 当前文本可以是未提交到Project的精确overlay，但调用者必须确保它确实是作者正在查看的同一份正文，不能混用另一份副本
- 坏语法不阻止纯文本坐标；未知清单/必需能力、来源越界、外部冲突和未完成保存事务继续拒绝
- 已跟踪归档文件可以定位，不因此激活或编译；浏览器仅核验授权导入快照

完整坐标、换行、空文、请求格式和预算以[正式契约](https://github.com/ikzerok/worldline/blob/main/spec/source-coordinates.md)为准。该功能不增加DSL、运行语义、作品格式、CLI或JSON-RPC方法；编辑器操作见[使用指南](https://github.com/ikzerok/worldedit/blob/main/docs/source-coordinates.md)。

# 当前文件源码结构

工具0.25提供 core Rust 只读投影，给编辑器在当前源文件内辨认声明、精确跳转和恢复作者位置。它不新增语言语法、运行状态、持久格式或第二份正文，也不把源码光标解释为实际演练状态。

## 共享接口

- `Project::source_outline(path, source)`：以当前工程的正式语言/能力选项解析传入的精确源码，产生声明列表和可用性状态
- `SourceOutline::current_item(byte_offset)`：只在可确认的正式语句范围内返回最深的声明归属；注释、空白和非UTF-8边界不猜对象
- `Project::source_outline_range(outline, source, occurrence)`：定位前重新验证工程、来源、当前正文与磁盘基线，再返回精确声明头字节范围

调用方不应直接信任旧条目的范围或按显示名回查。完整身份包括文件内来源 occurrence、类型、稳定ID和正式层级；重复名字/重复ID仍保留各自来源。范围是原始UTF-8半开字节区间，字符光标转换必须依据同一原文。

当前源码可为已应用未保存内容，也可由调用方传入精确尚未应用稿。投影不提交该稿；调用方必须显示自己实际传入的正文，并在每次定位前重校验，不能把新稿配上旧坐标。含语法错误、冲突或超过预算时明确不可用，不保留旧成功项冒充新结果。

结构查询单独设置有界资源保护：512 KiB正文、16384物理行/单行字节、4096声明、64层，以及2 MiB精确JSON投影；真实表达式还有256 token与64层的局部保护。普通编译与运行不新增这些限制。超限不会删改原文，也不返回可误认为完整的截断列表。

本轮没有新增 `wl` 子命令或 JSON-RPC 方法。独立工具可直接使用上述Rust API，但不能借此读取运行中编辑器私有缓冲或遥控界面。普通作者使用配对编辑器的“本文件结构”。

## 准确范围与限制

支持的显式声明类型、正式parser来源、层级、预算和失败状态以[规范](https://github.com/ikzerok/worldline/blob/main/spec/source-outline.md)为准。相关操作不会修改源字节、运行指纹、撤销栈或保存基线；语义错误的完整工程检查仍由原有编译流程负责。

- [编辑器操作步骤](https://github.com/ikzerok/worldedit/blob/main/docs/source-outline.md)
- [语言版本与能力](https://github.com/ikzerok/worldline/blob/main/spec/language-versions.md)
- [工作区与保存保护](https://github.com/ikzerok/worldline/blob/main/spec/workspace.md)
- [0.25范围与验收](https://github.com/ikzerok/worldedit/blob/main/docs/releases/v0.25.0.md)

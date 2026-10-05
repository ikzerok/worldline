# 从变量终值找到本次实际写入

0.27的路线对照扩展普通全局变量：两个终值不同后，可继续看本次验证中真正执行的
let/const/set从什么值写成什么值，以及core验证的语句来源。它不推断完整因果关系。

## 录制并比较

沿用[路线对照指南](https://github.com/ikzerok/worldline/blob/main/docs/route-comparison.md)
录制两条当前版本路径，再使用`wl route-compare`或`project.compare_routes`。两侧
`variable_writes`与`vars`、`variable_differences`一起阅读：

- `captured=true`表示本侧验证实例已开启本段写入捕获；旧结果缺字段为false，不能认作零写入
- `records`按本次实际发生顺序保留变量、操作、前后值、事件/节点、回合和可确认来源
- `before=null`是本次写入前未初始化，和空字符串、false、0分开
- `total_writes`是真实总数，`omitted`表示部分证据未保留；同值赋值也计数
- 结合该侧status/complete阅读，失败、取消、分歧和超预算结果不冒充完整结局

顶层初始值和检查点恢复值是起点。记录不会补造它们之前的历史；片段local和参数
不与全局同名变量混用。const重访而没有真正写入时不生成记录。求值失败不会生成
一条成功动作，也不会为取证重新执行表达式。

## 来源与边界

来源来自当前正式AST/parser与词法；缺少来源或身份无法唯一确认时保留实际写入，
但来源为null。不按同名变量、静态赋值列表或旧行号猜位置。编辑器跳转还要验证
当前完整源码与工作区；改稿之后重新比较。

状态动作与变量写入共享原每侧256条/64KiB额度，整体比较结果仍受1MiB守卫。
巨大值先计额后复制；记录截断不能解释为后续未发生。机器结果不遥控正在运行的
编辑器，也不修改其当前试玩会话、作者草稿或任何存档。

精确DTO、初始化范围、共享预算、旧结果兼容和来源守卫见
[变量写入证据契约](https://github.com/ikzerok/worldline/blob/main/spec/variable-write-evidence.md)。

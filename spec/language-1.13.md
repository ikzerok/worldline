# 语言 1.13：显式跨时段偏序与静态人物引用

产品0.10一次交付两项独立静态能力。清单 `language_version: "1.13"` 或显式
`CompileOptions::v1_13()` 启用本版；默认1.9及显式1.10/1.11/1.12不自动升级。
未知版本客户端按既有工作区只读保护保留源码原字节。

## 1. 跨时段偏序

```worldline
period year23 as "第二十三年"
period summer as "夏" within year23
period autumn as "秋" within year23
period early_autumn as "初秋" within autumn
event opening during summer
  节庆开幕，具体日期未定。
event closing during early_autumn follows opening
  节庆闭幕。
```

直接归属仍是summer与early_autumn，root均为year23；只有显式follows生成opening
先于closing。root是沿within到达的已声明顶层时段，不取最近共同祖先，也不按名称
推断共同历法。不同顶层root不可加边；根内任意深度后代及根本身的事件可加边。
不要求精确日期，不推导持续天数，不从父子或声明顺序生成边。

默认及旧显式版本中上述边继续A213。1.13独立启用此功能，不要求人物引用能力。
缺失前驱、无时段、跨独立root、自环、间接环与依赖环路为A213；无效父链为A219。
编辑父关系及添加边通过同一core事务校验，失败保留原源码；诊断可定位后继头及已知
前驱。重复follows去重，不自动制造传递边。

投影保留direct period和原rank（仅直接时段内部边）；新增root、order_scope和
root_rank，详见 [relations.md §7](relations.md)。
同根root_rank可用于布局但不是日期或全序；相同rank不保证同时，不同rank本身不证明
两点先后。解析/语义错误标partial并取消可信root_rank，空图也不得伪装完整。
CLI/RPC与UI原生分组共享该契约，源导航仍定位真实事件/时段声明。

## 2. 静态人物引用

本版另外提供直接指向唯一character ID的静态属性值及schema ref character，
必须同时显式启用语言1.13、`content.object_refs.v1`和`content.character_refs.v1`。
生命周期、类型约束、结构重命名及旧客户端保护见 [character-refs.md](character-refs.md)。
该能力不扩展其他TargetRef种类，不创建运行时人物参数或动态speaker。

## 3. 运行与存档边界

时段、父链、during、follows和静态人物ref属性均为作者静态资料，不因本版启用改变
entry、visits、选择、随机数、状态效果、call frame或运行fingerprint。仅改时间元数据
前后的存档可继续加载；人物ID本身改名仍遵从既有运行身份与存档规则。没有日历数学、
自动日期/时长、事实信念裁定、事件自动重放或发布白名单扩大。

# 0.18 → 0.19 合成兼容回归数据

本目录只有人工编写的虚构故事与其机器生成输出；人物、地点、值均为测试样例，不含用户文件、账户资料或真实创作稿件。

- `last-bell.wl` 是两分支虚构叙事；`consumer.wl` 增加带转义和Unicode的引用、规则、状态、once与片段调用
- save、entry trace和checkpoint trace由冻结0.18.0 CLI真实执行生成；checkpoint JSON原样摘取真实checkpoint trace的checkpoint对象
- manifest记录生成CLI SHA-256与固定fixture SHA-256。旧trace/checkpoint的`runtime_version`保持0.18.0，不得改成当前版本绕过兼容守卫
- trace中的绝对文件名只是生成合成fixture时的名义来源身份。测试以`include_str!`编译本目录源码；它不打开、读取或要求该绝对路径存在，因此CI与其他机器不依赖生成机器目录
- `file`和`line`也被用作真正的全局变量、参数、local名称；测试不得按键名递归剔除它们
- 只容许JSON对象键顺序差异；完整Story save、choice身份、once、状态和覆盖均受断言保护。checkpoint里的state是JSON字符串，语义比较要在该明确协议槽位解析，而不是忽略它

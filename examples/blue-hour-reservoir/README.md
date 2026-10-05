# 蓝时水库：依赖、路线审阅与地图往返

本示例为原创虚构演练内容，按本仓库 MIT 许可证随工具提供；不含第三方图片、地图或其他外部素材。人物与地点均为虚构。语言源码是唯一真源，两张地图仅保存展示与对象引用，不执行通行条件或推断地理归属。

需要 worldline/worldedit 0.28，工程已明确使用语言1.13。旧版本读取未知能力或不同工具轨迹时须遵守原有保护，不能为打开样例绕过版本检查。

## 场景与真实预期

潜水员余青在东岸镇找到了供水宪章。暴风封闭盐路，但最后一班渡船仍可凭通行证或水费搭乘。

- 世界资料：3名人物、6份实体资料、正式路线关系、别名、人物强引用及两张地图
- 可复用规则：toll(人数)计算水费，can_cross()读取当前水量和人数
- 路线A：公开宪章领取许可→乘渡船→结束，water保留为5
- 路线B：保留宪章凭记忆绕行→乘渡船→结束，支付4罐水，water变为1
- 盐路菜单有明确锁定说明，不应通过禁用项推进故事
- 未选分支仍是合法、未探索内容；一次路线验证不证明所有分支已经运行

## 编辑器演练

打开本目录的world.wl所在文件夹。

1. 在正文或试玩中打开“水费规则”/“渡河条件”资料，展开“使用处与相关上下文”，区分规则调用、全局读取与静态写入；跳到真实源码，再返回作者位置。静态使用处不等于本次实际执行动作。
2. 分别完成上面的两条路线，保存当前路径。在“试玩路径报告…”中选择路线，核对已应用稿与私密内容范围，重新验证并预览；明确复制或保存到工程外的新Markdown文件。报告只描述成功验证区段，不公开变量值、任意对象资料或附件。
3. 打开第三观察塔资料，从“地图中的位置”进入两张不同地图，观察人类名称与稳定ID；经源码/资料往返后使用作者返回。地图、选中项或稿件变化时应有安全回退，而不是套用旧位置修改对象。
4. 查看时间线或事件图中的汇聚连接，展开数量入口，逐条查看完整选择/条件和来源。同一choice的选择边与其正文显式跃迁边是独立core记录，数量应按实际记录计算，不应为了视觉整洁删除其中一条。

## CLI检查与审阅

在worldline仓库目录运行（发行包可使用同等相对路径；Windows使用wl.exe）：

```text
wl check examples/blue-hour-reservoir --json
wl world-context examples/blue-hour-reservoir --target rule:can_cross --options-json '{"include_executable":true,"depth":2}' --json
wl graph examples/blue-hour-reservoir --json
wl maps list examples/blue-hour-reservoir --json
```

默认world-context不自动增加新类型；上述显式opt-in才启用静态可执行使用处。所有语义和来源均来自core。

要记录路线，先选一个工作区外的新输出路径，再运行：

```text
wl play examples/blue-hour-reservoir --json --choice-presentation --seed 2800 --trace-output=路线A.json
```

JSON模式输入选择索引从0开始。路线A依次输入0、0；路线B依次输入1、0。锁定盐路的presentation行没有旧choices索引，不能把可见行号当可选索引。人类模式的编号则从1开始。

以PowerShell读取新轨迹并生成可读报告：

```powershell
$trace = Get-Content -Raw -Encoding UTF8 ./路线A.json
wl playthrough-report examples/blue-hour-reservoir --trace-json $trace
wl playthrough-report examples/blue-hour-reservoir --trace-json $trace --json
```

保存报告时选择工程外的新.md文件并使用终端的UTF-8重定向。CLI报告命令自身只读、不落盘；编辑器提供明确的新文件保存。轨迹和普通存档都不等于工程备份。原始trace/存档可含变量及运行状态，分享前需另行核对授权；可读报告省略这些字段不代表原始记录已经脱敏。跨工具版本应重新录制轨迹；报告的当前源码清单/指纹与验证状态应和同时交接的完整工程副本核对。

## 共享片段性能边界

本例的规则不需要展开调用图。工具另对复用片段建立保守的“可能Node跃迁”摘要；纯return/END片段不因重复复用而指数展开。含真实跃迁的极端调用图仍逐条保留来源和条件，触达静态预算时明确A231 error与不完整状态，不以截断图冒充通过。该保护不会替作者执行故事或移除运行时预算。

使用与范围：[试玩报告](https://github.com/ikzerok/worldline/blob/main/docs/playthrough-report.md)、[静态依赖规范](https://github.com/ikzerok/worldline/blob/main/spec/executable-context.md)、[语义与预算](https://github.com/ikzerok/worldline/blob/main/spec/semantics.md)。

源码检出的开发者可直接运行本轮配套性能/语义回归：

```text
cargo test -p worldline-core --test fragment_projection --locked
```

该回归包含32层重复return调用、真实转场的两种资源上限以及旧版图oracle逐边对照；它不以机器特定毫秒阈值替代正确性断言，也不把纯CLI测量当作界面帧率承诺。

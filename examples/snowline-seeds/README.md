# 栖雪山站 最小作者路线

一袋种子有两份来源记录，送种人选择采信哪份材料，并在同一检查片段中决定是否使用标签。本例显式使用语言 1.13；产品 0.12 不增加新 DSL，其他作品的默认语言仍为 1.9。先把本目录复制到自己的作品目录，再做改稿练习。

- `lore.wl`：人物、地点、种袋、原簿及两条带来源的记述。它们不自动裁定哪一方正确
- `world.wl`：世界时间偏序、实际叙事、两条路线和结局
- `shared/inspection.wl`：两处调用共用的检查片段，参数与 local 随暂停保存

## 检查并走两条路线

以下命令从 worldline 仓库根执行；`wl` 使用同版发行工具。交互模式选择从 1 起；下面 `--json` 的输入是从 0 起的 `index`。

```sh
wl check examples/snowline-seeds --json
wl catalog examples/snowline-seeds --json
wl timeline examples/snowline-seeds --json
wl play examples/snowline-seeds --seed 17
```

路线 A：选“采用苗圃的来源记录”→“用一枚标签登记来源”。结局为“来源公开”，route=苗圃、label_count=0、disclosed=true，knowledge 含 origin_known、paperwork 含 stamped。

路线 B：选“采用站务的封存建议”→“保留标签，原封交付”。结局为“原封交付”，route=站务、label_count=1、disclosed=false，两个状态集合都为空。两条路线都先返回各自 call 后文，再到 delivery；总计各两次选择。片段参数 document 分别为“苗圃原簿”和“运输清单”，local labels_before 均为 1。

## 记录重放与片段续档

请用同版工具生成和重放：ReplayTrace 与调试检查点严格匹配 runtime 版本，并检查 schema；0.11 轨迹不能直接在 0.12 重放。位置归一化不迁移旧版本轨迹；旧可选 DTO 形状也只在 runtime/schema 已兼容时可读。普通 Story 演练存档按独立的读取格式、必需能力和指纹规则处理，不套用这条轨迹版本规则。

在工程外新建输出目录。下面使用 POSIX shell；Windows PowerShell 对照命令列在后面。

```sh
OUT=$(mktemp -d)
printf '0\n0\n' | wl play examples/snowline-seeds --seed 17 --json --trace-output="$OUT/nursery.json"
printf '1\n1\n' | wl play examples/snowline-seeds --seed 17 --json --trace-output="$OUT/station.json"
wl replay examples/snowline-seeds --trace-json "$(cat "$OUT/nursery.json")" --json
wl replay examples/snowline-seeds --trace-json "$(cat "$OUT/station.json")" --json

# 只选第一条路线，在片段内部的选择处输入结束并保存。
printf '0\n' | wl play examples/snowline-seeds --seed 17 --json --save="$OUT/paused.json"
printf '0\n' | wl play examples/snowline-seeds --json --load="$OUT/paused.json"
```

PowerShell 的换行是反引号加 n。下面记录并重放路线 A；路线 B 将输入改成两行 1，并使用另一个轨迹文件名：

```powershell
$OUT = Join-Path $env:TEMP ("snowline-" + [guid]::NewGuid())
New-Item -ItemType Directory $OUT | Out-Null
"0`n0" | wl play examples/snowline-seeds --seed 17 --json "--trace-output=$OUT/nursery.json"
wl replay examples/snowline-seeds --trace-json (Get-Content "$OUT/nursery.json" -Raw) --json
```

两份完整轨迹应 `status=replayed`、`complete=true`、`ended=true`，各 `completed_choices=2`。续档仍在检查片段的菜单，不重复之前的台词；参数、local 和返回点保留，选择后到路线 A 结局。`--load` 不与 `--seed` 混用。演练存档不是作者工程备份。

## 对照三种顺序

- 世界偏序：collection → handover → delivery，三个事件属于同一 journey 根下的不同直接时段
- 实际阅读：handover → collection 的回忆 → delivery，由 `->` 和选择控制
- `at 1/2/3`：作者概览的展示顺序，不执行事件；书稿章节还可另行编排

世界时间不调度运行效果。没有明确先后边的事件不自动排成全序，同层也不表示同时发生。

## 试一次安全改稿

在自己的副本里先记录路线，再给 `shared/inspection.wl` 加空行或注释。0.12 重放应保持通过，同时仍可定位当前源码；它只排除调用帧顶层的 file/line 定位差异，没有忽略参数、local、调用层级或真实状态。把 `label_count` 初值改成 2 后，原入口轨迹应在观察到状态变化时报告分歧；不能通过删除指纹来载入不兼容存档。

编辑器只跳到本次重放结果在当前程序中的真实位置，旧轨迹行号仅供对比。改稿使结果过期后应重新重放；执行结束或对象缺失且没有实际位置时不可跳转，不会猜活动文件。

编辑器将 handover 的时段从 night 改为 afternoon 时，collection 仍是合法前驱，必须继续选中；它作为 delivery 前驱的关系也要保留。应用只更新工程缓冲，随后保存才写磁盘。跨到独立时间根时，非法关系保留供检查并阻止应用；需要作者明确取消具体前驱，后继约束仍须全工程校验。

完整但精简的作者步骤和改名边界见[从资料到可信重放](../../docs/author-route.md)。

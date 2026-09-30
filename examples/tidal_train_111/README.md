# 最后一班潮汐列车

使用产品0.8.0或更高版本。语言版本显式写在 `.world/project.json`，不会升级其他作品。

`wl check examples/tidal_train_111` 校验作品；`wl play examples/tidal_train_111 --seed 31` 演练。台词结构化输出使用 `--json`。在 hand_over 的选择处保存，会保留两层片段参数、局部证物快照、随机状态、一次性原则说明和返回点。

rule 每次读取当前人数和油料。when 只求命中分支；旧 and/or 仍急切求值。两处 explain 共用定义和本地化身份；once 原则说明在所有调用之间共享一次性状态。direction 是作者演出备注，普通运行文本和默认翻译交换不含它。

片段参数和local不污染全局。标签必须通过tag构造器选择已声明身份；集合运算不修改状态，只有明确的 become ... from 动作改变当前证物并记录历史。

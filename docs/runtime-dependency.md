# 试玩会话的自引用依赖

工具0.29使用官方crates.io发布的 `self_cell = "=1.2.2"`，Cargo.lock固定版本与SHA-256校验值 `b12e76d157a900eb52e81bc6e9f3069344290341720e9178cde2407113ac8d89`。项目按其Apache-2.0许可选项使用；原许可副本见[LICENSE](licenses/self-cell-APACHE.txt)。此文件不改变本项目MIT许可证。

该版本源码README说明默认最低Rust1.51，采用no_std+alloc且默认无额外运行依赖；本项目不启用old_rust或async_builder功能。使用当前固定工具链，并分别验证native与WASM，不从no_std声明推断浏览器GUI已经通过。

`OwnedStory`通过安全API构造私有Program/Analysis拥有者与借用它的Story。项目不手写unsafe或延长生命周期；self_cell依赖内部封装unsafe自引用实现，因此不宣称整个依赖图没有unsafe。原borrowed Story API保持，相关契约见[拥有型会话](../spec/owned-story.md)。

- [固定版本官方API](https://docs.rs/self_cell/1.2.2/self_cell/macro.self_cell.html)
- [上游源码](https://github.com/Voultapher/self_cell)

Windows和Web发行包包含SELF-CELL-LICENSE.txt，worldline源码与语言文档同时保留许可副本。

# 潮岸救援：语言1.11组合示例

从本目录或world.wl打开，清单显式选择1.11，不需要升级其他作品。

```sh
wl check examples/tide-rescue --json
wl play examples/tide-rescue --seed 31
```

先在医院选择“查看详细凭据”，再在收据片段中选择证物处理，返回后继续医院选择并进入码头的完整scene。相同片段在另一处调用，参数和返回位置独立；receipt中的一次性选择按同一源定义在全故事共享。

fare/enough/proof每次读取当前状态，people更新后没有缓存过期问题。state仍是唯一的证物集合；tag/state构造器使用静态身份，普通字符串不会转换成标签。say中的角色与正文分开，direction只供作者。#wl-localization标记沿用现有交换能力。

阅读包需要单独选择对象及字段。只选doctor对象不会公开secret或appearance；用schema2字段白名单明确选择appearance才发布该字段，私有动机与演出备注默认不公开。完整工程备份保留所有源码，不能用作无剧透读者包。

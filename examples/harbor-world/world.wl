// 世界总入口。协作者分别维护 events 下的文件,共享全局 ID。
world fog_harbor as "雾港纪事"
  description "潮汐纪元,一场暴雨让雾港和孤岛灯塔重新建立联系。各地的事件属于同一个世界,只记录已经确定的先后关系。"
  property era = "潮汐纪元"
  property calendar = "港历 317 年"

period storm_night as "暴风雨夜 · 21:00—23:00"

include "characters.wl"
include "events/harbor.wl"
include "events/lighthouse.wl"

// 标签类似引用集合。坐标指向地点标签,地点标签再指向事件。
tag coordinate as "坐标"
  description "查询已定义的位置,或继续沿标签寻找关联事件。"
tag harbor_place as "雾港码头"
  property x = 120
  property y = 36
tag lighthouse_place as "孤岛灯塔"
  property x = 128
  property y = 41
mark tag harbor_place with coordinate
mark tag lighthouse_place with coordinate
mark event arrival with harbor_place
mark event farewell with harbor_place
mark event beacon with lighthouse_place

// 同一人物可以同时维护多个状态；标签本体也可以作为状态目标。
tag calm as "平静"
tag alert as "警觉"
tag waiting as "等待"
tag ready as "就绪"
state lin_mood on character lin with calm as "林舟的心境"
state lin_mission on character lin with waiting as "林舟的任务"
state harbor_signal on tag harbor_place with waiting as "港口信号"

// 独立锚点只记录意义与稳定对象引用，不触发动作，也不改变演练路径。
anchor_def letter_resolve as "来信后的决定"
  description "林舟从旁观港口的风雨，转向主动回应这封来信。关联 arrival 与两个状态，可索引读信和作出决定的变化出处。"
anchor_link letter_resolve character lin
anchor_link letter_resolve state lin_mood
anchor_link letter_resolve state lin_mission
mark anchor letter_resolve with harbor_place

anchor_def beacon_reunion as "远处的回应"
  description "灯塔的回应让两人的旧识有了新的意义。独立对象与正文的手动锚点记录分别保留。"
anchor_link beacon_reunion character lin
anchor_link beacon_reunion character mei
anchor_link beacon_reunion anchor letter_resolve
mark anchor beacon_reunion with lighthouse_place

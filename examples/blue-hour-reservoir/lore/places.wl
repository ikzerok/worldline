entity reservoir_basin kind region as "蓝时水库干涸盆地"
  description "五塔围成的旧海床；地图区域只是作者标记，不自动决定归属。"
entity east_town kind place as "东岸镇与漫长的盐水阶梯"
  description "潮汐退去后建起的集市。渡口告示和水账并不同步。"
  property reservoir = ref("entity", "reservoir_basin")
entity west_town kind place as "西岸镇"
  description "另一端的水账保管地。"
entity watchtower kind place as "第三观察塔"
  description "暴风时仍有人看见灯亮。"
entity ferry kind vessel as "迟到者号"
  description "每日蓝时才起航的渡船。"
entity charter kind document as "共同供水宪章"
  description "公开水账声称渡船不收费；边角的签名似乎被重新描过。"
  property keeper = ref("character", "keeper")
relation_type connects as "连接"
  inverse "通往"
  direction directed
relation_def salt_road type connects from entity east_town to entity west_town
  description "暴风时关闭的盐路；运行条件写在事件选择中。"
  source_note "东岸镇告示，第七行"
relation_def ferry_route type connects from entity east_town to entity watchtower
  description "有水或有通行证可乘渡船。"
  source_note "守账人核验记录"

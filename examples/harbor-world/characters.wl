// 人物资料只定义一次,各事件通过角色 ID 引用。
character lin as "林舟"
  property occupation = "调查员"
  property age = 28
  relation mei as "同行者"

character mei as "梅雨"
  property occupation = "灯塔守望者"
  property resident = true
  relation lin as "旧识"

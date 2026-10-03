world last_bell as "雾港的最后一次钟响"
  description "退潮显露旧城，两个社群各自记着不同的历史。"

let bell_rung = false
character keeper as "守钟人青禾"
entity council kind organization as "港务议会"
  description "宣称旧钟必须永远沉默。"
entity old_city kind place as "退潮旧城"
  property guardian = ref("character", "keeper")
relation_type protects as "守护"
  from character
  to entity
relation_def keeper_city type protects from character keeper to entity old_city

period low_tide as "最后一次退潮"
event arrival as "钟楼前" with keeper
  青禾问：你相信谁的记忆？
  choice "敲响旧钟"
    set bell_rung = true
    -> remember
  choice "交出钟槌"
    -> silence

event remember as "共同记忆"
  钟声将两份记忆交还给海。钟响：{bell_rung}
  -> END

event silence as "沉默的证词"
  港务议会封存了最后一只钟槌。钟响：{bell_rung}
  -> END

event historic_bell during low_tide
  旧钟的第一次钟响，已经发生。


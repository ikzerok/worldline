// 《最后一班潮汐列车》：显式1.11的规则、共享片段、集合与角色台词。
world coast as "潮汐海岸"
character doctor as "岚医生"
character keeper as "站务员舟"
tag map as "疏散地图"
tag log as "潮汐日志"
tag key as "仓库钥匙"
state evidence on world coast with map, log, key as "随身证物"
let fuel = 8
let people = 2

rule fee(passengers: num) -> num = passengers * 2
rule enough() -> bool = when(people > 0, fuel / people >= 2, true)
rule proof_count() -> num = count(members(state(evidence)))

fragment hand_over(selected: tag)
  local before: tagset = members(state(evidence))
  local tide: num = rnd(-2, 2)
  潮位观测：{tide}。交出前有{count(before)}件证物。 #wl-localization:tide_reading
  choice "交出所选证物" if contains(members(state(evidence)), selected)
    become state(evidence) remove from tags(selected)
    已交出，余下{proof_count()}件。 #wl-localization:proof_handed
    return
  choice "暂时保留"
    return

fragment explain(place: str, selected: tag)
  local charge: num = fee(people)
  say doctor "{place}的{people}人需要{charge}罐油。" direction "压低声音，避免惊动候车者" #wl-localization:evacuation_cost
  choice once "请医生解释疏散原则"
    say doctor "先保护需要帮助的人，然后核对证物。" #wl-localization:evacuation_principle
  choice "已了解原则，继续"
    你点了点头。
  call hand_over(selected)
  say keeper "{place}的讨论结束，返回原来的行程。" #wl-localization:return_to_route

event hospital as "医院的决定"
  say doctor "最后一班列车快到了。" #wl-localization:last_train
  call explain("医院", tag(map))
  choice "接走医院的候车者" if enough()
    set fuel = fuel - fee(people)
    -> harbor
  choice "先去码头"
    -> harbor

event harbor as "码头的决定"
  set people = 1
  call explain("码头", tag(log))
  choice "接走码头的人" if enough()
    set fuel = fuel - fee(people)
    -> station
  choice "保留油料，返回车站"
    -> station

event station as "潮水来临之前"
  say keeper "还有{fuel}罐油，证物共{proof_count()}件。" #wl-localization:final_account
  if proof_count() >= 2
    你把两件以上的证物交给记录员，事故调查得以继续。
  else
    你记下证物去向，等风浪过去后补齐记录。
  最后一班潮汐列车驶过亮着灯的桥。
  -> END

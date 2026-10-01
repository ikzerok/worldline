include "lore.wl"
include "shared/inspection.wl"

let label_count = 1
let route = "未定"
let disclosed = false
tag origin_known as "知道种子来源"
tag stamped as "来源已登记"
state knowledge on character courier with [] as "送种人掌握的资料"
state paperwork on entity seed_lot with [] as "这一袋种子的手续"
rule may_label() -> bool = has(knowledge, origin_known) and has(paperwork, stamped)

period journey as "封山前后的两天"
period afternoon as "前日下午" within journey
period night as "封山夜" within journey
period dawn as "次日清晨" within journey

event handover as "夜间交接" with courier, botanist at 1 during night follows collection
  [[entity:station|栖雪山站]]只剩最后一班车。种袋已经封好，来源标签却还是空白。
  say botanist "先想想昨天下午，是谁把这袋种子交给了你。"
  -> collection

event collection as "回忆采收" with courier, botanist, inspector at 2 during afternoon
  你的思绪回到前日下午。[[entity:register|苗圃原簿]]和运输清单记着不同的交接理由。
  choice "采用苗圃的来源记录"
    set route = "苗圃"
    become knowledge add origin_known
    call inspection("苗圃原簿")
    苗圃路线返回：你记得容槿指过的那一栏，思绪回到发车之后。
    -> delivery
  choice "采用站务的封存建议"
    set route = "站务"
    call inspection("运输清单")
    站务路线返回：你记得陆遥说先保住种袋，思绪回到发车之后。
    -> delivery

event delivery as "清晨送达" with courier at 3 during dawn follows handover
  if may_label()
    set disclosed = true
    结局：来源公开。接收人知道这一袋来自哪一座苗圃。
  else
    结局：原封交付。接收人保留了来源待查的说明。
  路线：{route}；剩余标签：{label_count}；已公开：{disclosed}。
  -> END

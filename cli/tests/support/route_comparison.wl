world foundry as "晨钟工坊"
entity bell kind device as "铜铃"
tag restored as "修复"
tag traded as "交换"
state bell_fate on entity bell with [] as "铜铃去向"
let credits = 0
event arrival
  工坊窗外传来风声。
  choice "修复铜铃"
    become bell_fate with restored as "焊合铃舌"
    -> workshop
  choice "换取零件"
    become bell_fate with traded as "交给行商"
    set credits = 7
    -> workshop
event workshop
  工作台上的余温尚未散去。
  choice "离开工坊"
    -> END

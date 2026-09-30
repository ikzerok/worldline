// 1.11 完整语言示例：共享规则、两层片段、集合参数与角色台词。
world coast as "潮岸"
character doctor as "林医生"
  property appearance = "蓝色雨衣"
  property secret = "未向读者公开的动机"
tag map as "海图"
tag log as "航海日志"
tag permit as "通行证"
state evidence on world coast with map, log, permit as "随身证物"
let fuel = 12
let people = 2
rule fare(count: num) -> num = count * 2
rule enough(count: num) -> bool = when(count > 0, fuel >= fare(count), false)
rule proof() -> bool = count(members(state(evidence))) >= 2

fragment receipt(place: str, selected: tag)
  local amount: num = fare(people)
  say doctor "{place}需要{amount}罐油。" direction "低声，停顿" #wl-localization:receipt_cost
  choice once "交出这份证物"
    become state(evidence) remove from tags(selected)
    say doctor "已收下证物。" #wl-localization:receipt_accept
  choice "保留证物"
    保留了手中的资料。
  return

fragment consultation(place: str, selected: tag)
  say doctor "先核对{place}的登记。" #wl-localization:consult_open
  choice "查看详细凭据" if proof()
    call receipt(place, selected)
  choice "直接继续"
    没有展开详细凭据。
  返回咨询段。
  return

event hospital as "医院"
  call consultation("医院", tag(map))
  医院登记恢复；现有{count(members(state(evidence)))}份证物。
  choice "送走这批人" if enough(people)
    set fuel = fuel - fare(people)
    set people = 3
    -> dock.boarding
  choice "结束本次演练"
    -> END

event dock as "码头"
  这段只在从码头事件入口进入时显示。
  scene boarding
    call consultation("码头", tag(log))
    码头登记恢复，剩余油料{fuel}。
    choice "完成登记" if enough(people)
      -> END
    choice "保留当前安排"
      -> END

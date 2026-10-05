world reservoir as "蓝时水库"
  description "退去的内海留下五座塔镇。公共水账与潜水员的记忆并不一致。"
let water = 5
let travellers = 2
let permit = false
let storm = true
let remembered = false
rule toll(people: num) -> num = people * 2
rule can_cross() -> bool = water >= toll(travellers)
tag sealed as "未公开"
tag shared as "已公开"
state charter_status on world reservoir with sealed as "供水宪章的公开状态"


event arrival as "蓝时的最后一班渡船" with diver, keeper
  [[character:diver|潜水员]]把湿透的[[entity:charter|供水宪章]]放在桌上。
  水费按[[rule:toll|水费规则]]计算，渡船准入使用[[rule:can_cross|渡河条件]]。
  say keeper "{travellers}人要付{toll(travellers)}罐水。你们现在有{water}罐。" direction "仍然看着窗外的塔"
  choice "公开宪章，领取通行证"
    become charter_status with shared as "港口公开登记"
    set permit = true
    -> crossing
  choice "保留宪章，凭记忆绕行"
    set remembered = true
    -> crossing

include "lore/places.wl"
include "lore/people.wl"
include "story/routes.wl"

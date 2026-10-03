include "设定/人物与地点.wl"
world salt_mist as "盐雾群岛：失灯之夜"
  description "海图员需要在潮汐封港前调查失灯。\n第二行：保留中文，标点与‘引号’。"
period night as "失灯之夜"
period dawn as "黎明前" within night
period morning as "黎明后" within night

tag old_chart as "旧海图"
tag permit as "通行证"
state inventory on character linqi with old_chart as "行囊"
let secret_allowed = false

event harbor as "雾港择路" with linqi, lingzhou during dawn
  [[character:lingzhou|绫舟]]递来一张旧海图。绫舟港与绫舟是不同名字。
  你听说[[entity:north_lighthouse|北灯塔]]已经失灯。
  choice "去北灯塔" enable has(inventory, permit) disabled "需要通行证"
    -> lighthouse
  choice once "请绫舟出示通行证"
    become inventory add permit as "获得通行证"
    -> harbor
  choice "去潮洞"
    -> cave
  choice "去密室" if secret_allowed
    -> impossible

event lighthouse as "北灯塔调查" with linqi during morning follows lights_out
  携带旧海图抵达北灯塔。
  choice "回雾港"
    -> harbor
  choice "记录灯塔结局"
    -> ending_lighthouse

event cave as "潮洞调查" with linqi during morning follows lights_out
  旧海图上的暗礁原来是入口。
  -> ending_cave

event witness as "守灯人离港" with lingzhou during dawn
  守灯人见过渡鸦商会。
  -> END

event council_order as "议会封港令" during dawn
  议会下达封港令。
  -> END

event lights_out as "北灯塔失灯" during dawn follows witness, council_order
  北灯塔熄灭。
  -> END

event ending_lighthouse as "灯塔结局"
  你找到失踪的守灯人。
  -> END

event ending_cave as "潮洞结局"
  你发现走私航线。
  -> END

event impossible as "不可达密室结局"
  这个结局有静态入边，但开关从不设为true。
  -> END

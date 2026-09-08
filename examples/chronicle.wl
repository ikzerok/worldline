// chronicle.wl —— v1.5 全特性示例:双故事线漂流(2026-04 草案模型)
// 清醒世界(awake)与梦境(dream)两条故事线;->> 负责跨线漂流。

character servant as "女仆"
character keeper as "守梦人"

let lucidity = 0

storyline awake as "清醒世界"
  event start as "雨夜的宅邸"
    你站在旧宅门前,雨越下越大。
    choice "敲门"
      set lucidity = lucidity + 1
      -> hall
    choice "在门廊睡下"
      -> sleep

  event hall as "门厅" with servant
    effect on enter
      meet servant as "初见女仆"
    女仆举着烛台,在黑暗里看着你。
    choice "询问宅子的历史"
      她讲起三楼永远锁着的房间。
      anchor "听闻密室" as "主角得知三楼密室"
      grant brave as "你下定决心"
      -> stair
    choice "告辞"
      -> sleep

  event stair as "楼梯" perm brave
    你鼓起勇气,踏上了那段楼梯。~
    楼梯尽头只有一扇小小的窗,窗外还是雨。
    -> sleep

  event sleep as "入梦"
    effect on enter
      part servant as "现实远去"
    你在椅子上睡去。~
    ->> dream.entry

  event wake as "梦醒"
    你在门廊醒来,手心里多了一枚雾凝成的钥匙。 #道具:雾钥匙
    -> END

storyline dream as "梦境"
  event dream.entry as "梦境入口" with keeper
    effect on enter
      meet keeper as "初见守梦人"
    守梦人等在雾里,仿佛已经等了很久。
    choice "追问密室" if lucidity >= 1
      "清醒的人才能推开门。"他说。
      -> dream.door
    choice "随雾漂流"
      -> dream.deep

  event dream.door as "梦境之门" after seen(hall)
    门后传来现实的声音。~
    雾开始退散。
    choice "推门而归"
      ->> wake
    choice "留在梦里"
      -> dream.deep

  event dream.deep as "雾之深处"
    effect on enter if lucidity >= 3
      to awake as "极度清醒:提前归线"
    雾越来越浓。~
    你分不清方向,索性跟着雾走。
    -> dream.entry

// 灯塔作者维护。此事件与港口的来信并列,不固定先后。
storyline lighthouse as "灯塔"
  event beacon as "灯塔守望者" with lin, mei at 10 during storm_night
    梅雨重新点亮灯塔,将信号传向远处的港口。
    anchor "灯塔重逢"
    -> END

// 关联独立锚点；上面的 anchor "灯塔重逢" 仍只在演练时记录。
anchor_link beacon_reunion event beacon

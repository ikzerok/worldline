// 港口作者维护。arrival 与灯塔 beacon 的时间先后尚未确定。
storyline harbor as "雾港"
  event arrival as "港口的来信" with lin at 10 during storm_night
    effect on enter
      become lin_mood with alert as "读到来信"
    effect on exit
      become lin_mission with ready as "决定下一步行动"
    雾很大,港口的灯一盏盏亮起。
    choice "前往灯塔"
      ->> beacon
    choice "留在港口"
      -> farewell

  // farewell 明确晚于 arrival 和 beacon,不要求它们之间有先后。
  event farewell as "潮汐之后" with lin at 20 during storm_night follows arrival, beacon
    effect on exit
      become lin_mood with calm as "信已收好"
      become harbor_signal with ready as "码头准备完毕"
    林舟将信收好,等待下一次潮汐。
    -> END

// 事件作者在本文件补充关联，锚点与状态仍在总入口定义。
anchor_link letter_resolve event arrival

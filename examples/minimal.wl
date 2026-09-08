// minimal.wl —— 最小 worldline 故事

event start
  世界线在此分岔。
  choice "向左"
    左边是一条河。~
    河水很凉。
    -> END
  choice "向右"
    右边是一座山。
    -> END

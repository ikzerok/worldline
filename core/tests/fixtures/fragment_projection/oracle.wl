let open = true
let count = 2
fragment quiet()
  choice "继续停留"
    return
  if open
    call silence()
  return
fragment silence()
  return
fragment jumping()
  if open
    choice once "带上长名字的供水宪章" enable count > 0 disabled "没有名额"
      -> target
  else
    ->> other
fragment wrapper()
  call quiet()
  call jumping()
  call jumping()
event start
  scene room
    if count > 1
      call wrapper()
    -> END
event target
  -> END
storyline far
  event other
    -> END

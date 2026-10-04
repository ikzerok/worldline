event wharf as "最后一罐燃料" with luopo,jixian
  [[character:luopo|罗沛]]抬头看着[[entity:lighthouse|沉灯塔]]。[[character:jixian|季弦]]举起唯一的燃料罐。
  say luopo "那艘船上有人。你也听见了第四声钟，对吗？" direction "不看档案员"
  choice "把燃料交给救生艇" if fuel > 0
    set fuel = fuel - 1
    become rescue_status add rescued as "救生艇出发"
    -> dawn
  choice "留下燃料照亮档案"
    -> dawn

event dawn as "天明以前" with linwu
  if has(archive_status, public) and has(rescue_status, rescued)
    潮水吞下船骨，却带回三个活着的证人。公开的名字终于有了声音。
  if has(archive_status, public) and not has(rescue_status, rescued)
    档案在灯下清晰，海上的声音却永远停了。
  if has(archive_status, hidden) and has(rescue_status, rescued)
    三个人获救。林芜把日志锁进抽屉，第四声钟继续没有名字。
  if has(archive_status, hidden) and not has(rescue_status, rescued)
    灯亮到天明，城里仍没人知道那一夜发生了什么。
  -> END

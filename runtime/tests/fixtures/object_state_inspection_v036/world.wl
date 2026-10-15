world shared as "同名"
character shared as "同名"
character other as "同名"
entity empty kind place as "无状态地点"
tag calm as "平静"
tag alert as "警觉"
tag proved as "已验证"
state person_mood on character shared with calm as "状态"
state person_empty on character shared with [] as "状态"
state other_mood on character other with calm as "状态"
state world_mood on world shared with [] as "状态"
let shared_score = 3
let shared_mood = false
fragment change(target: state)
  become target add from tags(tag(proved))
  return
event start with shared
  effect on enter
    become person_mood with alert
  choice "继续 {rnd(1, 9)}" if has(person_mood, alert)
    become person_mood with alert
    call change(state(person_empty))
    choice "结束"
      -> END

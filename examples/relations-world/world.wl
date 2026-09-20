entity keepers kind organization as "守灯会"
  description "共同维护港口灯塔的组织。"
entity lighthouse kind place as "雾港灯塔"
period modern as "现代"
relation_type maintains as "维护"
  inverse "由其维护"
  direction directed
  from entity
  to entity
relation_def daily_care type maintains from entity keepers to entity lighthouse
  description "守灯会负责日常维护。"
  source_note "共同设定记录第3项"
  scope period modern
  property frequency = "每日"
relation_def annual_repair type maintains from entity keepers to entity lighthouse
  description "每年另行组织塔身修缮，与日常维护分别记载。"
  property frequency = "每年"
event arrival as "抵达灯塔"
  查阅 [[relation:daily_care|日常维护]] 与 [[relation:annual_repair|年度修缮]]。
  -> END

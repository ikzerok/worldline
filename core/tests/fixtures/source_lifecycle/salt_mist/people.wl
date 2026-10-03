character linqi as "林栖"
  property occupation = "海图员"
  property biography = "少年时居于雾港。\n如今绘制潮汐地图。"
  property mentor = ref("character", "lingzhou")
character lingzhou as "绫舟"
  property title = "守灯人"
  property note = "绫舟只是普通文字；lingzhou_suffix不可误改。"
alias character lingzhou as "舟姐"
entity harbor_place kind place as "雾港"
  property belongs_to = ref("entity", "tide_council")
entity north_lighthouse kind place as "北灯塔"
entity tidal_cave kind place as "潮洞"
entity tide_council kind organization as "潮汐议会"
entity raven_guild kind organization as "渡鸦商会"
relation_type directs as "指挥"
  inverse "受指挥于"
  direction directed
relation_def rel_council_harbor type directs from entity tide_council to entity harbor_place
  description "议会正式管辖雾港。"
relation_def rel_lingzhou_lighthouse type directs from character lingzhou to entity north_lighthouse
  description "绫舟看守灯塔。"

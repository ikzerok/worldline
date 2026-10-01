world island as "镜海孤岛 · 银钥匙档案"
  description "资料约束、可见锁定分支与独立书稿的完整例子。"

schema city for entity entity_type place closed
  field population_id population number required
  field founding_id founding_year number required
  field mayor_id mayor ref entity entity_type organization required
  field public_id public boolean
  field motto_id motto text

entity council kind organization as "潮汐议会"
  description "负责守护灯塔和旧档案的组织。"
entity harbor kind place as "镜海港"
  description "灯塔照亮潮湿的旧石路。"
  property population = 0
  property founding_year = 1200
  property mayor = ref("entity", "council")
  property public = false
  property motto = ""
bind entity harbor to city

character keeper as "守灯人岑溪"
let silver_key = false

fragment archive(place: str)
  local caption: str = "抵达" + place
  say keeper "{caption}。请记住，我们只公开明示选择的资料。"
  choice "翻看潮汐记录"
    你记下了昨日的潮位。
    return
  choice "合上卷宗"
    return

event arrival as "第一章：镜海港的银钥匙与漫长夜色"
  夜潮拍打镜海港的石阶，守灯人点亮窗边的小灯。
  你看见 [[entity:harbor|镜海港]] 的地图，但旧档案室的门仍然锁着。
  对岸的船只逐渐隐入薄雾，每一扇窗都像一页未完成的稿纸。
  潮声提醒你，世界设定和读者当前能做的事情是不同的。
  choice once "进入旧档案室" enable silver_key disabled "还缺银钥匙；先向守灯人领取"
    call archive("旧档案室")
    -> ending
  choice once "领取银钥匙"
    set silver_key = true
    -> arrival
  choice "离开港口"
    -> ending

event ending as "第二章：灯塔之后"
  天色渐亮，镜海港留在你的身后。
  -> END

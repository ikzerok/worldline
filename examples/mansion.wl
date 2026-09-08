// mansion.wl —— 旧宅惊魂:展示事件/场景/选择/once/条件/计数/标签/粘接

let courage = 0
let has_key = false
const MAX_VISITS = 3

event start
  你站在旧宅门前,雨越下越大。 #氛围:雨夜
  门牌上的字迹已经模糊。~
  只认得出一个"宅"字。

  choice "敲门"
    set courage = courage + 1
    门后传来缓慢的脚步声。
    -> hall
  choice "绕到后院"
    你翻过湿滑的矮墙,落在荒草里。
    -> garden
  choice "多等一会儿" if visits(start) < MAX_VISITS
    雨没有变小,你的勇气正在流失。
    -> start

event hall
  女仆举着烛台,在黑暗里看着你。
  if courage >= 1
    "真有胆量,"她说,"
  else
    "从后门进来的客人,"她皱起眉,"
  在这样的夜里,大家都是不速之客。"
  -> hall.choice

event hall.choice
  你面前的走廊通向两个方向。

  choice "去书房"
    -> study
  choice "去地窖" if has_key
    -> cellar
  choice "回门口"
    你沿着原路返回。
    -> start

event study
  书房里满是灰,壁炉上摆着一张照片。 #线索:照片
  choice once "翻开日记"
    set has_key = true
    日记的最后一页夹着一把小钥匙。 #关键道具:钥匙
    你收起了它。
  choice "离开书房"
    -> hall
  你合上了门。
  -> hall

event garden
  后院荒草齐腰,只有一间亮着灯的小屋。
  scene shed
    小屋里空无一人,桌上摆着一盏还温着的茶。
    set courage = courage + 2
    茶的主人一定刚离开不久。~
    你的心跳加快了。
    -> start

event cellar
  地窖的石阶又冷又深。
  if courage >= 3
    你一口气走到底,推开了尽头的大门——
    门后是你从未见过的星空。 #结局:星空
    -> END
  else
    黑暗浓得化不开,你退了回去。
    -> hall

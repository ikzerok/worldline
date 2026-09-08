world harbor as "雾港人物资料"
  description "通过别名和正文链接阅读同一个世界。"

character lin as "林舟"
  property appearance = "深色外套，衣袋里总放着一枚旧罗盘。"
  property background = "曾经负责灯塔的夜间巡查。\n现在沿海岸寻找失踪的妹妹。"
  property motivation = "确认妹妹的下落，而不只是得到一个令人安心的解释。"
  property boundaries = "不会用无辜者的安全换取线索。"
  property voice = "陌生人面前只回答必要信息；谈及妹妹时会主动解释。"
  property dialogue_examples = "拒绝：这事到此为止。\n关心：风要变了，你最好留在灯下。"
  relation mei as "妹妹"
character mei as "林梅"
  property personality = "遇到未知的事物，会先观察，再提问。"
alias character lin as "阿舟"
alias character lin as "灯塔守望者"

tag pier as "旧码头"
  description "港口东侧的木制码头，夜间可以看见灯塔。"
tag calm as "平静"
tag alert as "警觉"
state mood on character lin with calm as "心境"
anchor_def decision as "重新承担责任"
  description "林舟开始认真对待码头工人的担忧。"
anchor_link decision character lin
anchor_link decision event arrival
anchor_link decision state mood
mark character lin with pier
mark event arrival with pier

event arrival as "回到码头" with lin
  [[character:lin|阿舟]]停在[[tag:pier|旧码头]]边，看向远处的灯。
  choice "问[[character:lin|守望者]]发生了什么"
    become mood with alert as "听见关于灯塔的消息"
    他把罗盘收进衣袋。“风要变了。”
    -> END
  choice "陪他安静地等一会儿"
    become mood with calm as "选择暂时等待"
    他点了点头，仍然看着海面。
    -> END

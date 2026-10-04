entity harbor kind place as "港城"
  description "石阶每日两次被潮水覆盖。"
entity guild kind organization as "渔人会"
  description "保管各船家的潮钟。"
entity bureau kind organization as "潮务局"
  description "命令在下一次逆潮前焚毁旧灯塔。"
entity lighthouse kind place as "旧灯塔"
  description "顶层存放未寄出的信。"
  property owner = ref("entity", "guild")
  property disputed = true
  property windows = 9
entity seal kind item as "潮务印信"
  description "一侧缺角，与信封上的印痕相符。"
  property custodian = ref("character", "scribe")

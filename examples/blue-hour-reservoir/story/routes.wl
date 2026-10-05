event crossing as "穿过空海床" with keeper, diver
  choice "走[[relation:salt_road|盐路]]" enable not storm disabled "暴风封路，暂时不能通行"
    你踩着白色盐线前进。
    -> reunion
  choice "乘[[entity:ferry|迟到者号]]" enable permit or can_cross() disabled "还缺通行证或足够水费"
    if not permit
      set water = water - toll(travellers)
    -> reunion
  choice "在第三观察塔等待"
    -> reunion

event reunion as "同一个清晨的三封信" with courier
  if remembered
    潜水员认出了宪章上的旧字迹。
  else
    守账人把登记副本封进信里。
  say courier "现在还有{water}罐水。我会把你们的消息带到西岸。"
  -> END

fragment inspection(document: str)
  local labels_before: num = label_count
  say inspector "我核对的是{document}。你进站时有{labels_before}枚标签。"
  choice "用一枚标签登记来源" enable label_count > 0 disabled "没有可用标签"
    set label_count = label_count - 1
    become paperwork add stamped
    标签盖好了。
  choice "保留标签，原封交付"
    你把标签留在口袋里。
  return

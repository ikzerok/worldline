world coast as "雾港兼容样稿"
tag key as "钥匙"
tag log as "证词"
state inventory on world coast with key, log
let file = "真正语义文件值"
let line = 7
character keeper as "守钟人"
entity ledger kind item as "key inventory ready 原样"
  property owner = ref("character", "keeper")
  property mirror = ref("entity", "ledger")
rule ready(n: num) -> bool = n > 0 and has(inventory, key)
rule count_keys() -> num = count(members(state(inventory)))
fragment gate(file: str)
  local line: num = 9
  say keeper "中文\t👩‍👩‍👧‍👦é\"{file}\" {ready(line)} {count_keys()} {contains(members(state(\"inventory\")), tag(\"key\"))}" direction "key inventory ready 原样"
  choice once "交出钥匙：{ready(line)}" if ready(line)
    become state(inventory) remove from tags(tag(key))
    return
  choice "保留钥匙"
    return
period tide as "最后一次退潮"
event start during tide
  call gate("海图")
  call gate("旧地图")
  -> finish
event finish as "修订后的结尾" during tide follows start
  {file} / {line} / {count_keys()} [[entity:ledger|ledger 原样]]
  -> END


world countertide as "逆潮档案"
  description "港城每逢逆潮，居民收到七日前自己尚未写出的信。"
include "人物/档案员.wl"
include "设定/港城.wl"
include "章节/灯塔.wl"
let seal_found = false

event arrival as "一 · 逆潮来信" with patrol
  你抵达 [[entity:harbor|港城]] 时，[[character:patrol|巡潮员沈砚]] 正在收拢湿透的信。
  信封上有你自己的字迹，落款却在七日之后。
  say patrol "旧灯塔属于渔人会，这是我从父亲那里听来的。"
  choice once "检查信封的印信"
    你看见 [[entity:seal|潮务印信]]，记下缺口的位置。
    set seal_found = true
    -> arrival
  choice "出示印信，进入灯塔" enable seal_found disabled "先检查信封的印信"
    -> tower
  choice "留下信件，返回港城"
    -> retreat

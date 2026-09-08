// include-main.wl —— 多文件故事:章节从其他文件合并

let affinity = 0

include "include-chapter2.wl"

event start
  序章:你在车站长椅上醒来,手里攥着半张车票。
  choice "去站台" 
    -> platform
  choice "再睡一会儿"
    车站的广播声把你再次吵醒。
    -> start

event platform
  你把半张车票递给检票员。
  if affinity >= 1
    "又是你。"他叹了口气,放你进去了。
    -> END
  else
    "票不完整,不能进。"他摇摇头。
    -> END

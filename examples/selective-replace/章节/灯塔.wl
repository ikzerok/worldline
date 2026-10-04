event tower as "二 · 未写出的证词" with scribe
  [[entity:lighthouse|旧灯塔]] 的门闩上刻着 [[entity:guild|渔人会]] 的纹样。
  你在窗下找到 [[character:scribe|闻笙]] 的记录：“灯塔归潮务局，九扇窗是伪造的。”
  这是一条证词，还不能当作已确认的事实。
  choice "保存证词，等待下一次逆潮"
    -> preserve
  choice "把证词交给潮务局"
    -> submit

event preserve as "三 · 留下证词"
  你把信压在九扇窗下的窗台上。灯塔的主人仍有疑问。
  回廊里的烛火逐渐熄灭，信封边缘仍有盐结成的白线。风从石阶上吹来，带着远处码头的铁锈气味。你把每一份证词按收到的顺序放好，再回头检查窗边的字迹。纸页说的是九扇窗，作者还需要决定哪些叙述应该修订。
  -> END

event submit as "四 · 烧毁之前"
  潮务局收走证词，焚塔命令被暂缓。
  -> END

event retreat as "尾声 · 回到港城"
  你没有打开信件，逆潮却记住了你的脚步。
  -> END

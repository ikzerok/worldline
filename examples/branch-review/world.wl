world tide_city as "潮汐城的第十三次退潮"
  description "潮退一夜，天明封海；留下的不是同一个真相。"
let fuel = 1
tag hidden as "尚未公开"
tag public as "已经公开"
tag rescued as "成功营救"
state archive_status on world tide_city with hidden as "档案公开状态"
state rescue_status on world tide_city with [] as "营救记录"
event arrival as "退潮钟声" with linwu
  钟敲十三下。[[character:linwu|林芜]]在[[entity:archive|档案馆]]收到一页烧焦的航海日志。
  choice "把旧日志交给全城"
    become archive_status with public as "选择公开"
    -> wharf
  choice "把旧日志藏入袖中"
    -> wharf

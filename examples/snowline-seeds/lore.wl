world snowline as "栖雪山站"
  description "封山前最后一班车，会把一袋种子送往山下。"
character courier as "阿棠"
character botanist as "容槿"
character inspector as "陆遥"
alias character botanist as "苗圃记录员"
entity station kind place as "栖雪山站"
entity seed_lot kind object as "待运种袋"
  property custodian = ref("character", "botanist")
entity register kind source as "苗圃原簿"
  description "记载种子来源；空白处不等于从未发生。"
relation_type recounts as "记述"
  inverse "记述来源"
relation_def nursery_account type recounts from entity register to event collection
  source_note "苗圃原簿：容槿将种子交给送种人，以备山下补种。"
relation_def station_account type recounts from character inspector to event collection
  source_note "站务口述：先封存运走，来源由接收人复核。"

use super::*;

#[test]
fn playthrough_minimal_left() {
    let (log, labels) = play_all(
        r#"
event start
  世界线在此分岔。
  choice "向左"
    左边是一条河。~
    河水很凉。
    -> END
  choice "向右"
    右边是一座山。
    -> END
"#,
        &[0],
    );
    assert_eq!(labels, vec!["向左|向右"]);
    assert_eq!(
        log,
        "世界线在此分岔。 <<向左>> 左边是一条河。河水很凉。[END]"
    );
}

#[test]
fn playthrough_conditions_and_vars() {
    let src = r#"
let coins = 5

event start
  你有 {coins} 枚硬币。
  choice "买地图" if coins >= 10
    -> END
  choice "买半张" if coins >= 4
    set coins = coins - 4
    还剩 {coins} 枚。
    -> END
"#;
    let (log, labels) = play_all(src, &[0]);
    // 条件不满足:买地图被过滤,首选项是"买半张"
    assert_eq!(labels, vec!["买半张"]);
    assert_eq!(log, "你有 5 枚硬币。 <<买半张>> 还剩 1 枚。[END]");
}

#[test]
fn once_choice_disappears_after_taken() {
    let src = r#"
event room
  房间一览无余。
  choice once "开宝箱"
    宝箱空了。
  choice "离开"
    -> END
  -> room
"#;
    let (_, labels) = play_all(src, &[0, 0]);
    assert_eq!(
        labels,
        vec!["开宝箱|离开", "离开"],
        "once 选择被选后应从列表消失"
    );
}

#[test]
fn fallback_falls_through_when_all_cond_fail() {
    let src = r#"
event start
  choice "敲门" if false
    -> END
  没有可用的选择,你转身离开。
  -> END
"#;
    let (log, labels) = play_all(src, &[]);
    assert!(labels.is_empty(), "无可用选择时不暂停:{labels:?}");
    assert_eq!(log, "没有可用的选择,你转身离开。[END]");
}

#[test]
fn gather_semantics_shared_continuation() {
    let src = r#"
event start
  choice "A"
    你选了 A。
  choice "B"
    你选了 B。
  两条路在此汇聚。~
  然后。
  -> END
"#;
    let (log_a, _) = play_all(src, &[0]);
    let (log_b, _) = play_all(src, &[1]);
    // ~ 粘接方向:作用于其后一行(汇聚行粘接"然后"行)
    assert!(
        log_a.contains("你选了 A。\n两条路在此汇聚。然后。"),
        "{log_a}"
    );
    assert!(
        log_b.contains("你选了 B。\n两条路在此汇聚。然后。"),
        "{log_b}"
    );
}

#[test]
fn nested_choice_inner_gather() {
    let src = r#"
event start
  choice "外层一"
    choice "内层甲"
      甲。
    内层汇聚点。
  外层汇聚点。
  -> END
"#;
    let (log, _) = play_all(src, &[0, 0]);
    assert!(log.contains("甲。\n内层汇聚点。\n外层汇聚点。"), "{log}");
}

#[test]
fn visits_and_turns() {
    let src = r#"
event start
  第 {visits(start)} 次来到这里,共 {turns()} 回合。
  choice "再来一次" if visits(start) < 3
    -> start
  choice "结束"
    -> END
"#;
    let (log, labels) = play_all(src, &[0, 0, 0]);
    assert_eq!(labels.len(), 3);
    assert!(log.contains("第 1 次来到这里,共 0 回合。"), "{log}");
    assert!(log.contains("第 2 次来到这里,共 1 回合。"), "{log}");
    assert!(log.contains("第 3 次来到这里,共 2 回合。"), "{log}");
}

#[test]
fn scene_nesting_and_divert() {
    let src = r#"
event market
  你走进集市。
  scene stall
    摊主向你招手。~
    "来看看。"
    -> market.exit

event market.exit
  你离开了集市。
  -> END
"#;
    let (log, _) = play_all(src, &[]);
    assert!(
        log.contains("你走进集市。\n摊主向你招手。\"来看看。\""),
        "{log}"
    );
    assert!(log.contains("你离开了集市。"), "{log}");
}

#[test]
fn variable_gate_once_choice_and_gather_walkthrough() {
    let result = compile_source(
        "walkthrough.wl",
        r#"let score = 0
let unlocked = false
event start
  choice "进入"
    set score = score + 1
    -> hub
  choice "跳过"
    -> END
  choice "等待" if visits(start) < 3
    -> start
event hub
  choice "领取标记"
    -> item
  choice "受限分支" if unlocked
    -> gated
  choice "返回"
    -> start
event item
  choice once "取得标记"
    set unlocked = true
    已解锁
  choice "返回"
    -> hub
  汇聚完成
  -> hub
event gated
  if score >= 3
    条件通过
    -> END
  else
    条件不足
    -> hub
"#,
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let mut log = transcript(&mut s);
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["进入", "跳过", "等待"]
    );
    s.choose(0).unwrap(); // score=1
    log.push_str(&transcript(&mut s));
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["领取标记", "返回"],
        "变量解锁前不应出现受限选择"
    );
    s.choose(0).unwrap(); // item
    log.push_str(&transcript(&mut s));
    s.choose(0).unwrap(); // once 选择后执行汇聚
    log.push_str(&transcript(&mut s));
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["领取标记", "受限分支", "返回"],
        "变量解锁后应出现受限选择"
    );
    s.choose(1).unwrap(); // score=1，执行 else 后回到 hub
    log.push_str(&transcript(&mut s));
    assert!(log.contains("已解锁"), "{log}");
    assert!(log.contains("汇聚完成"), "{log}");
    assert!(log.contains("条件不足"), "{log}");
    assert!(!log.contains("条件通过"), "{log}");
    s.choose(0).unwrap(); // 再次进入 item，once 选择已消费
    transcript(&mut s);
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["返回"]
    );
}

#[test]
fn rnd_within_bounds_and_save() {
    let src = r#"
event start
  骰子:{rnd(1, 6)}。
  -> END
"#;
    for _ in 0..20 {
        let result = compile_source("r.wl", src);
        let mut s = Story::new(&result.program, &result.analysis).unwrap();
        let log = transcript(&mut s);
        let n: f64 = log
            .trim_start_matches("骰子:")
            .trim_end_matches("。[END]")
            .parse()
            .unwrap();
        assert!((1.0..=6.0).contains(&n), "{log}");
    }
}

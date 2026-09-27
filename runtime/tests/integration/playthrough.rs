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
fn mansion_full_walkthrough() {
    // 敲门 → 去书房 → 翻开日记(得钥匙) → 去地窖 → 勇气不足退回
    let result = compile_path(&example("mansion.wl")).unwrap();
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let mut log = String::new();
    log.push_str(&transcript(&mut s)); // start 暂停
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["敲门", "绕到后院", "多等一会儿"]
    );
    s.choose(0).unwrap(); // 敲门 courage=1
    log.push_str(&transcript(&mut s)); // hall → hall.choice 暂停
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["去书房", "回门口"],
        "无钥匙时地窖不应出现"
    );
    s.choose(0).unwrap(); // 去书房
    log.push_str(&transcript(&mut s)); // study 暂停
    s.choose(0).unwrap(); // 翻开日记(once,得钥匙)
    log.push_str(&transcript(&mut s)); // 汇聚 → hall.choice 暂停
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["去书房", "去地窖", "回门口"],
        "有钥匙后地窖应出现"
    );
    s.choose(1).unwrap(); // 去地窖(courage=1 < 3)
    log.push_str(&transcript(&mut s)); // cellar else 分支 → hall.choice
    assert!(log.contains("日记的最后一页夹着一把小钥匙。"), "{log}");
    assert!(log.contains("你合上了门。"), "{log}");
    assert!(log.contains("黑暗浓得化不开"), "{log}");
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

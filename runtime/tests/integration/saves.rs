use super::*;
// -- 存读档 -------------------------------------------------------------------

#[test]
fn save_load_roundtrip() {
    let src = r#"
let courage = 0

event start
  你有 {courage} 点勇气。
  choice "加勇"
    set courage = courage + 1
    -> mid
  choice "直接走"
    -> mid

event mid
  中场:勇气 {courage}。
  choice "结局" if courage >= 1
    好结局。
    -> END
  choice "坏结局"
    坏结局。
    -> END
"#;
    let result = compile_source("save.wl", src);
    let mut a = Story::new(&result.program, &result.analysis).unwrap();
    transcript(&mut a);
    a.choose(0).unwrap(); // 加勇 → courage=1,进入 mid 并暂停
    transcript(&mut a);
    let json = a.save().unwrap();

    // 恢复:存档不含暂停态,continue 会重新遇到选择组并再次暂停
    let mut b = Story::load(&result.program, &result.analysis, &json).unwrap();
    let silence = transcript(&mut b);
    assert!(silence.is_empty(), "重建暂停不应产生新输出:{silence}");
    let labels: Vec<String> = b.choices().iter().map(|c| c.label.clone()).collect();
    assert_eq!(labels, vec!["结局", "坏结局"]);
    b.choose(0).unwrap();
    let out = transcript(&mut b);
    assert!(out.contains("好结局。"), "{out}");
}

#[test]
fn save_load_mid_text_execution() {
    // 在选择体内执行到一半(嵌套帧)时存档,恢复后继续执行剩余语句
    let src = r#"
event start
  choice "走"
    第一句。~
    -> keep
  -> END

event keep
  choice "继续"
    后半句 A。
    -> END
  choice "停"
    后半句 B。
    -> END
"#;
    let result = compile_source("s2.wl", src);
    let mut a = Story::new(&result.program, &result.analysis).unwrap();
    transcript(&mut a);
    a.choose(0).unwrap(); // 进入选择体:文本 + divert → keep 暂停
    let log1 = transcript(&mut a);
    assert!(log1.contains("第一句。"), "{log1}");
    let json = a.save().unwrap(); // 暂停于 keep 的选择组

    let mut b = Story::load(&result.program, &result.analysis, &json).unwrap();
    let silence = transcript(&mut b);
    assert!(silence.is_empty(), "重建暂停不应产生新输出:{silence}");
    assert_eq!(b.choices().len(), 2);
    b.choose(1).unwrap();
    let log2 = transcript(&mut b);
    assert!(log2.contains("后半句 B。"), "{log2}");
}

#[test]
fn fingerprint_rejects_changed_program() {
    let r1 = compile_source("a.wl", "event start\n  甲。\n  -> END\n");
    let s = Story::new(&r1.program, &r1.analysis).unwrap();
    let json = s.save().unwrap();
    let r2 = compile_source("a.wl", "event start\n  乙。\n  -> END\n");
    assert!(
        Story::load(&r2.program, &r2.analysis, &json).is_err(),
        "内容变化后旧档应被拒绝"
    );
}

#[test]
fn comment_does_not_break_saves() {
    let r1 = compile_source("a.wl", "event start\n  甲。\n  -> END\n");
    let s = Story::new(&r1.program, &r1.analysis).unwrap();
    let json = s.save().unwrap();
    let r2 = compile_source(
        "a.wl",
        "// 只是加了一条注释\nevent start\n  甲。\n  -> END\n",
    );
    assert!(
        Story::load(&r2.program, &r2.analysis, &json).is_ok(),
        "加注释不应破坏存档"
    );
}

#[test]
fn save_roundtrip_v15_state() {
    let src = "storyline a\n  event start\n    grant key\n    anchor \"记录点\" as \"存档前\"\n    ->> b.entry\n\nstoryline b\n  event b.entry\n    梦境。\n    choice \"继续\"\n      -> END\n";
    let result = compile_source("sv.wl", src);
    let mut a = Story::new(&result.program, &result.analysis).unwrap();
    transcript(&mut a); // b.entry 暂停
    assert_eq!(a.storyline(), "b");
    let json = a.save().unwrap();
    let mut b = Story::load(&result.program, &result.analysis, &json).unwrap();
    transcript(&mut b);
    assert_eq!(b.storyline(), "b");
    assert!(b.perm_list().contains(&"key".to_string()));
    let manuals: Vec<_> = b
        .anchors()
        .iter()
        .filter(|x| x.kind == AnchorKind::Manual)
        .collect();
    assert_eq!(manuals.len(), 1);
    assert_eq!(manuals[0].name, "记录点");
}

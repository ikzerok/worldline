use super::*;

#[test]
fn diagnostics_unknown_divert_and_var() {
    let c = codes("event start\n  -> nowhere\n  set x = y + 1\n  -> END\n");
    assert!(c.contains(&("A101".into(), Severity::Error)), "{c:?}");
    assert!(c.iter().filter(|(k, _)| k == "A102").count() >= 2, "{c:?}"); // x 与 y 未声明
}

#[test]
fn diagnostics_duplicate_symbols() {
    let c = codes("event start\n  -> END\nevent start\n  -> END\n");
    assert!(c.contains(&("A104".into(), Severity::Error)), "{c:?}");
}

#[test]
fn diagnostics_const_assignment() {
    let c = codes("const K = 1\nevent start\n  set K = 2\n  -> END\n");
    assert!(c.contains(&("A106".into(), Severity::Error)), "{c:?}");
}

#[test]
fn diagnostics_type_mismatch() {
    let c = codes("let a = 1\nevent start\n  if a + \"x\"\n    -> END\n  -> END\n");
    assert!(c.contains(&("A103".into(), Severity::Error)), "{c:?}");
}

#[test]
fn diagnostics_unknown_function() {
    let c = codes("event start\n  你 {foo(1)}\n  -> END\n");
    assert!(c.contains(&("A103".into(), Severity::Error)), "{c:?}");
}

#[test]
fn diagnostics_unreachable_event() {
    let c = codes("event start\n  -> END\n\nevent island\n  -> END\n");
    assert!(c.contains(&("A201".into(), Severity::Warning)), "{c:?}");
}

#[test]
fn diagnostics_missing_end_divert() {
    let c = codes("event start\n  就这样结束。\n");
    assert!(c.contains(&("A202".into(), Severity::Warning)), "{c:?}");
}

#[test]
fn diagnostics_all_cond_group() {
    let c = codes("let flag = false\nevent start\n  choice \"a\" if flag\n    -> END\n  -> END\n");
    assert!(c.contains(&("A203".into(), Severity::Warning)), "{c:?}");
}

#[test]
fn diagnostics_self_loop() {
    let c = codes("event start\n  -> start\n");
    assert!(c.contains(&("A206".into(), Severity::Warning)), "{c:?}");
}

#[test]
fn diagnostics_unused_var() {
    let c = codes("let u = 1\nevent start\n  -> END\n");
    assert!(c.contains(&("A107".into(), Severity::Warning)), "{c:?}");
}

#[test]
fn diagnostics_tab_indent() {
    let c = codes("event start\n\tx\n  -> END\n");
    assert!(c.contains(&("P002".into(), Severity::Error)), "{c:?}");
}

#[test]
fn v15_diagnostics() {
    // A208:with 引用未定义角色
    let c = codes("character a\nevent start with ghost\n  -> END\n");
    assert!(c.contains(&("A208".into(), Severity::Error)), "{c:?}");
    // A210:to 目标故事线不存在
    let c2 = codes("event start\n  effect on enter\n    to nothere\n  -> END\n");
    assert!(c2.contains(&("A210".into(), Severity::Error)), "{c2:?}");
    // A209:同线漂流
    let c3 = codes("storyline a\n  event start\n    ->> start\n");
    assert!(c3.contains(&("A209".into(), Severity::Warning)), "{c3:?}");
}

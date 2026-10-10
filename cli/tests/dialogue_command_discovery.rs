use std::process::Command;

#[test]
fn missing_and_unknown_commands_show_both_dialogue_entrypoints() {
    for args in [vec![], vec!["unknown-command"], vec!["--help"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_wl"))
            .args(args)
            .output()
            .unwrap();
        // 顶层沿用现有用法失败退出码；真正子命令帮助另行成功返回。
        assert_eq!(output.status.code(), Some(2));
        let help = String::from_utf8(output.stderr).unwrap();
        assert!(help.contains("wl dialogue query|preview|apply"), "{help}");
        assert!(help.contains("wl production-script query|export"), "{help}");
        assert!(help.contains("--save / --output 均须显式选择"), "{help}");
    }
    let error = wl::run(
        &["unknown-command".into()],
        &mut Vec::new(),
        &mut std::io::Cursor::new(Vec::new()),
    )
    .unwrap_err();
    assert!(error.contains(" / dialogue / production-script / "));
}

#[test]
fn subcommand_help_succeeds_and_explains_explicit_delivery() {
    for (command, option) in [("dialogue", "--save"), ("production-script", "--output")] {
        let output = Command::new(env!("CARGO_BIN_EXE_wl"))
            .args([command, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains(&format!("wl {command}")), "{help}");
        assert!(help.contains(option), "{help}");
    }
}

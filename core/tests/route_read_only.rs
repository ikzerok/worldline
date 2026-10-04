use worldline_core::project::Project;
#[test]
fn readonly_open_rejects_transactions_without_recovery_and_keeps_raw_source() {
    let root = std::env::temp_dir().join(format!("worldline-v020-readonly-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let source = b"event start\n  hello\n  -> END\n";
    std::fs::write(root.join("world.wl"), source).unwrap();
    let project = Project::open_read_only(&root).unwrap();
    assert!(!project.compile_read_only().unwrap().has_errors());
    assert!(!project.is_dirty());
    std::fs::create_dir_all(root.join(".world/.transactions/pending")).unwrap();
    std::fs::write(
        root.join(".world/.transactions/pending/journal.json"),
        b"unresolved",
    )
    .unwrap();
    assert!(Project::open_read_only(&root)
        .err()
        .unwrap()
        .contains("未完成"));
    assert!(project.compile_read_only().is_err());
    assert_eq!(std::fs::read(root.join("world.wl")).unwrap(), source);
    assert_eq!(
        std::fs::read(root.join(".world/.transactions/pending/journal.json")).unwrap(),
        b"unresolved"
    );
    std::fs::remove_dir_all(root).unwrap();
}

use std::{fs, path::PathBuf};
use worldline_core::{compile_path, project::Project};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "wl-workspace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn recursive_index_refresh_conflict_and_complete_export() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    fs::create_dir_all(root.join("chapters/deep")).unwrap();
    fs::create_dir_all(root.join(".agent/skills")).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    fs::write(
        root.join("chapters/deep/extra.wl"),
        "character extra as \"目录人物\"\n",
    )
    .unwrap();
    fs::write(root.join("README.md"), "作者的说明").unwrap();
    fs::write(root.join(".agent/skills/note.md"), "关联创作资料").unwrap();
    fs::write(root.join("unused.bin"), [0, 255, 128]).unwrap();
    let mut project = Project::open(&root).unwrap();
    // 文档索引与刷新结果使用工程规范路径。
    let root = project.root.clone();
    assert_eq!(project.documents.len(), 2);
    assert!(project
        .compile()
        .analysis
        .symbols
        .characters
        .contains_key("extra"));
    assert_eq!(project.search("目录人物").len(), 1);
    assert_eq!(
        compile_path(&root).unwrap().analysis.fingerprint,
        project.compile().analysis.fingerprint
    );
    let extra = root.join("chapters/deep/extra.wl");
    fs::write(&extra, "character changed\n").unwrap();
    project.refresh().unwrap();
    assert!(project
        .compile()
        .analysis
        .symbols
        .characters
        .contains_key("changed"));
    project
        .set_text(&extra, "character local\n".into())
        .unwrap();
    fs::write(&extra, "character remote\n").unwrap();
    assert_eq!(project.refresh().unwrap(), vec![extra.clone()]);
    assert!(project.document(&extra).unwrap().contains("local"));
    assert!(project.save().is_err());
    let export = temp.0.join("export");
    project.export(&export).unwrap();
    assert_eq!(fs::read(export.join("unused.bin")).unwrap(), [0, 255, 128]);
    assert_eq!(
        fs::read_to_string(export.join("README.md")).unwrap(),
        "作者的说明"
    );
    assert!(export.join(".agent/skills/note.md").is_file());
    assert!(fs::read_to_string(export.join("chapters/deep/extra.wl"))
        .unwrap()
        .contains("local"));
    assert!(project.export(&root.join("nested-export")).is_err());
    let new_file = root.join("added.wl");
    fs::write(&new_file, "tag new_tag\n").unwrap();
    project.refresh().unwrap();
    assert!(project.documents.contains_key(&new_file));
    fs::remove_file(&new_file).unwrap();
    project.refresh().unwrap();
    assert!(!project.documents.contains_key(&new_file));
}

#[test]
fn external_includes_assets_and_symlink_ancestors_are_rejected() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    fs::create_dir(&root).unwrap();
    fs::write(temp.0.join("outside.wl"), "character hidden\n").unwrap();
    fs::write(
        root.join("world.wl"),
        "include \"../outside.wl\"\nevent start\n  -> END\n",
    )
    .unwrap();
    let result = compile_path(&root).unwrap();
    assert!(result.diagnostics.iter().any(|d| d.code == "A109"));
    assert!(!result.analysis.symbols.characters.contains_key("hidden"));
    fs::write(
        root.join("world.wl"),
        "asset leak file \"../outside.wl\"\nevent start\n  -> END\n",
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    assert!(project
        .compile()
        .diagnostics
        .iter()
        .any(|d| d.code == "A109"));
    assert!(project
        .add_asset_reference(
            &worldline_core::catalog::TargetRef::new("event", "start"),
            &temp.0.join("outside.wl")
        )
        .is_err());
    assert!(project
        .add_file(std::path::Path::new("../escape.wl"))
        .is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&temp.0, root.join("link")).unwrap();
        assert!(project.refresh().is_err());
        assert!(project
            .add_file(std::path::Path::new("link/new.wl"))
            .is_err());
    }
}

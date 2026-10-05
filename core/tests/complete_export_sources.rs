//! 完整工程导出保留全部创作源码，但不改变活动编译或读者公开边界。
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::catalog::TargetRef;
use worldline_core::project::{CheckpointLimits, Project};
use worldline_core::reader_export::ReaderExportSelection;

const WORLD: &str = "include \"active.wl\"\nevent start\n  公开正文\n  -> END\n";
const MANIFEST: &str = concat!(
    "{\r\n  \"schema_version\": 1, \"language_version\": \"1.10\",\r\n",
    "  \"entry\":\"world.wl\", \"required_features\":[\"workspace.source_sets.v1\"],\r\n",
    "  \"source_config\": {\"mode\":\"explicit\",\"active\":[\"world.wl\",\"active.wl\"],",
    "\"archived\":[\"归档/旧稿.wl\"]},\r\n  \"extension\": {\"keep\": \"原始字节🙂\"}\r\n}\r\n",
);

struct Workspace {
    base: PathBuf,
    root: PathBuf,
}
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!(
            "wl-complete-export-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = base.join("workspace");
        fs::create_dir_all(&root).unwrap();
        let workspace = Self {
            base,
            root: Project::new(&root).root,
        };
        workspace.write("world.wl", WORLD.as_bytes());
        workspace.write(
            "active.wl",
            concat!(
                "entity visible kind place as \"公开地点\"\n",
                "entity active_secret kind item as \"活动秘密_SENTINEL\"\n",
            )
            .as_bytes(),
        );
        workspace.write(
            "归档/旧稿.wl",
            "entity archived kind item as \"归档秘密_SENTINEL\"\r\n".as_bytes(),
        );
        workspace.write(
            "草稿/未活动.wl",
            "entity inactive kind item as \"未活动秘密_SENTINEL\"\n".as_bytes(),
        );
        workspace.write(".world/project.json", MANIFEST.as_bytes());
        workspace.write(".hidden/unreferenced.bin", &[0, 255, 12, 13, 10]);
        workspace.write(".agent/private.md", "创作私记_SENTINEL\n".as_bytes());
        workspace.write("notes.md", "未引用说明🙂\r\n".as_bytes());
        workspace
    }
    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn open(&self) -> Project {
        Project::open(&self.root).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}
fn bytes<'a>(files: &'a BTreeMap<PathBuf, Vec<u8>>, path: &str) -> &'a [u8] {
    files
        .get(Path::new(path))
        .unwrap_or_else(|| panic!("完整导出遗漏：{path}"))
        .as_slice()
}

#[test]
fn complete_export_keeps_archived_inactive_current_buffers_and_exact_manifest() {
    let workspace = Workspace::new();
    let mut project = workspace.open();
    let compiled = project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let archived =
        "// 归档未保存的编辑🙂\r\nentity archived kind item\r\n  property unfinished = (\r\n";
    let inactive = "尚未完成的未活动草稿🙂\r\n含任意语言文本";
    project
        .set_text(&workspace.root.join("归档/旧稿.wl"), archived.into())
        .unwrap();
    project
        .set_text(&workspace.root.join("草稿/未活动.wl"), inactive.into())
        .unwrap();
    let baseline = project.content_baseline();
    let exported = project.export_files().unwrap();
    assert_eq!(bytes(&exported, "归档/旧稿.wl"), archived.as_bytes());
    assert_eq!(bytes(&exported, "草稿/未活动.wl"), inactive.as_bytes());
    assert_eq!(bytes(&exported, ".world/project.json"), MANIFEST.as_bytes());
    for relative in [
        "world.wl",
        "active.wl",
        ".hidden/unreferenced.bin",
        ".agent/private.md",
        "notes.md",
    ] {
        assert_eq!(
            bytes(&exported, relative),
            fs::read(workspace.root.join(relative)).unwrap()
        );
    }
    assert_eq!(project.content_baseline(), baseline);
    assert!(project.is_dirty());
    assert_ne!(
        fs::read(workspace.root.join("归档/旧稿.wl")).unwrap(),
        archived.as_bytes()
    );
    let root = workspace.base.join("not-created");
    let mut rebuilt = Project::from_snapshot(&root, Path::new("world.wl"), &exported).unwrap();
    assert!(!root.exists());
    let compiled_again = rebuilt.compile();
    assert!(
        !compiled_again.has_errors(),
        "非活动坏稿不得进入编译：{:?}",
        compiled_again.diagnostics
    );
    assert_eq!(
        compiled_again.analysis.fingerprint,
        compiled.analysis.fingerprint
    );
    assert_eq!(compiled_again.program.entry, compiled.program.entry);
    assert!(!compiled_again
        .analysis
        .catalog
        .entities
        .contains_key("archived"));
    assert!(!compiled_again
        .analysis
        .catalog
        .entities
        .contains_key("inactive"));
    let selection = rebuilt.source_selection().unwrap();
    assert!(selection.is_active(&root.join("active.wl")));
    assert!(selection.is_archived(&root.join("归档/旧稿.wl")));
    assert!(!selection.is_active(&root.join("草稿/未活动.wl")));
    assert_eq!(
        rebuilt.document(&root.join("归档/旧稿.wl")).unwrap(),
        archived
    );
    assert_eq!(
        rebuilt.document(&root.join("草稿/未活动.wl")).unwrap(),
        inactive
    );
}

#[test]
fn complete_export_keeps_unsaved_new_archived_and_inactive_buffers() {
    let workspace = Workspace::new();
    let mut project = workspace.open();
    let fingerprint = project.compile().analysis.fingerprint;
    let new_inactive = project.add_file(Path::new("草稿/新未活动.wl")).unwrap();
    let new_archived = project.add_file(Path::new("归档/新归档.wl")).unwrap();
    project
        .set_text(&new_inactive, "新未活动原文🙂\r\n".into())
        .unwrap();
    project
        .set_text(&new_archived, "新归档原文🙂\r\n".into())
        .unwrap();
    project
        .set_text(&workspace.root.join("world.wl"), WORLD.into())
        .unwrap();
    let manifest = MANIFEST.replace(
        "\"archived\":[\"归档/旧稿.wl\"]",
        "\"archived\":[\"归档/旧稿.wl\",\"归档/新归档.wl\"]",
    );
    project
        .set_authoring_document(
            &workspace.root.join(".world/project.json"),
            manifest.as_bytes().to_vec(),
        )
        .unwrap();
    assert!(!project.compile().has_errors());
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    let exported = project.export_files().unwrap();
    assert_eq!(
        bytes(&exported, "草稿/新未活动.wl"),
        "新未活动原文🙂\r\n".as_bytes()
    );
    assert_eq!(
        bytes(&exported, "归档/新归档.wl"),
        "新归档原文🙂\r\n".as_bytes()
    );
    assert_eq!(bytes(&exported, ".world/project.json"), manifest.as_bytes());
    assert!(!new_inactive.exists());
    assert!(!new_archived.exists());
    assert_eq!(
        fs::read(workspace.root.join(".world/project.json")).unwrap(),
        MANIFEST.as_bytes()
    );
    let copy = workspace.base.join("exported");
    for (relative, content) in &exported {
        let path = copy.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    let mut reopened = Project::open(&copy).unwrap();
    assert!(!reopened.compile().has_errors());
    assert_eq!(reopened.compile().analysis.fingerprint, fingerprint);
    assert!(reopened
        .source_selection()
        .unwrap()
        .is_archived(&copy.join("归档/新归档.wl")));
    assert!(!reopened
        .source_selection()
        .unwrap()
        .is_active(&copy.join("草稿/新未活动.wl")));
}

#[test]
fn complete_export_excludes_tombstones_and_checkpoints_and_blocks_pending_transactions() {
    let workspace = Workspace::new();
    let mut project = workspace.open();
    project
        .delete_document(&workspace.root.join("草稿/未活动.wl"))
        .unwrap();
    project
        .create_checkpoint(Some("导出前".into()), CheckpointLimits::default())
        .unwrap();
    assert!(workspace.root.join(".world/.checkpoints").exists());
    let exported = project.export_files().unwrap();
    assert!(!exported.contains_key(Path::new("草稿/未活动.wl")));
    assert!(workspace.root.join("草稿/未活动.wl").exists());
    assert!(exported
        .keys()
        .all(|path| !path.starts_with(".world/.checkpoints")
            && !path.starts_with(".world/.transactions")));
    // 外部出现待恢复事务时必须拒绝整个导出，不能把半完成状态打包。
    workspace.write(".world/.transactions/pending/journal.json", b"{}");
    let baseline = project.content_baseline();
    assert!(project.export_files().is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(
        fs::read(
            workspace
                .root
                .join(".world/.transactions/pending/journal.json")
        )
        .unwrap(),
        b"{}"
    );
}

#[test]
fn complete_backup_does_not_broaden_reader_publication_selection() {
    let workspace = Workspace::new();
    let project = workspace.open();
    let complete = project.export_files().unwrap();
    assert!(complete.contains_key(Path::new("归档/旧稿.wl")));
    assert!(complete.contains_key(Path::new("草稿/未活动.wl")));
    assert!(complete.contains_key(Path::new(".agent/private.md")));
    let selection = ReaderExportSelection {
        schema_version: 1,
        site_title: "公开地点".into(),
        objects: vec![TargetRef::new("entity", "visible")],
        fields: Vec::new(),
        required_features: Vec::new(),
        maps: Vec::new(),
        manuscripts: Vec::new(),
        attachments: Vec::new(),
    };
    let plan = project.preview_reader_export(&selection).unwrap();
    let published = project
        .build_reader_export(&selection, &plan.plan_digest)
        .unwrap();
    let combined = published
        .iter()
        .flat_map(|(path, bytes)| [path.to_string_lossy().as_bytes().to_vec(), bytes.clone()])
        .flatten()
        .collect::<Vec<_>>();
    let output = String::from_utf8_lossy(&combined);
    for secret in [
        "活动秘密_SENTINEL",
        "归档秘密_SENTINEL",
        "未活动秘密_SENTINEL",
        "创作私记_SENTINEL",
        "active_secret",
        "archived",
        "inactive",
    ] {
        assert!(!output.contains(secret), "读者公开包不应携带：{secret}");
    }
    assert!(!published.contains_key(Path::new(".world/project.json")));
    assert!(published
        .keys()
        .all(|path| path.extension().is_none_or(|extension| extension != "wl")));
}

#[cfg(unix)]
#[test]
fn complete_export_still_rejects_unreferenced_links_including_hidden_files() {
    use std::os::unix::fs::symlink;
    for relative in ["linked.wl", ".hidden/linked.bin"] {
        let workspace = Workspace::new();
        let project = workspace.open();
        let outside = workspace.base.join("outside.txt");
        fs::write(&outside, "目录外资料🙂").unwrap();
        symlink(&outside, workspace.root.join(relative)).unwrap();
        let baseline = project.content_baseline();
        assert!(project.export_files().is_err());
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(fs::read_to_string(outside).unwrap(), "目录外资料🙂");
    }
}

#[test]
fn complete_export_keeps_include_and_asset_workspace_boundaries() {
    for declaration in [
        "include \"../outside.wl\"\n",
        "asset outside file \"../outside.wl\"\n",
    ] {
        let workspace = Workspace::new();
        let outside = workspace.base.join("outside.wl");
        fs::write(&outside, "event outside\n  -> END\n").unwrap();
        let mut project = workspace.open();
        project
            .set_text(&project.entry.clone(), format!("{declaration}{WORLD}"))
            .unwrap();
        let baseline = project.content_baseline();
        assert!(project.export_files().is_err());
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(
            fs::read_to_string(outside).unwrap(),
            "event outside\n  -> END\n"
        );
    }
}

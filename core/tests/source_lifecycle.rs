//! 安全源码组织：身份/资源/顺序证明与失败零修改。
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::project::Project;
use worldline_core::source_lifecycle::SourceLifecycleRequest as Request;

struct Workspace(PathBuf);
impl Workspace {
    fn new(files: &[(&str, &str)]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-lifecycle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        // 复用 compiler::source_path 的正式身份，消除 Windows 临时目录的 8.3 别名。
        let root = Project::new(&root).root;
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        Self(root)
    }
    fn open(&self) -> Project {
        Project::open(&self.0).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn request(from: &str, to: &str) -> Request {
    Request::Move {
        from: from.into(),
        to: to.into(),
    }
}
const EXPLICIT: &str = r#"{"schema_version":1,"language_version":"1.9","required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":["world.wl","old.wl"],"archived":["archive.wl"]},"extension":{"keep":"old.wl"}}"#;

#[test]
fn explicit_create_is_active_and_include_inactive_rejects_without_partial_changes() {
    let ws = Workspace::new(&[
        ("world.wl", "event start\n  未完成 ${\n"),
        ("old.wl", "// old\n"),
        ("archive.wl", "// archived\n"),
        ("inactive.wl", "// inactive\n"),
        (".world/project.json", EXPLICIT),
    ]);
    let mut project = ws.open();
    let baseline = project.content_baseline();
    assert!(project
        .include_file(&ws.0.join("archive.wl"))
        .unwrap_err()
        .contains("非活动"));
    assert_eq!(project.content_baseline(), baseline);
    assert!(project.include_file(&ws.0.join("inactive.wl")).is_err());
    assert_eq!(project.content_baseline(), baseline);
    let path = project.add_file(Path::new("章节/新 章.wl")).unwrap();
    assert!(project.source_selection().unwrap().is_active(&path));
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("include \"章节/新 章.wl\""));
    assert!(!project
        .compile()
        .diagnostics
        .iter()
        .any(|d| d.code == "A105"));
    let value: serde_json::Value = serde_json::from_slice(
        project
            .authoring_document(&ws.0.join(".world/project.json"))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(value["extension"]["keep"], "old.wl");
    assert_eq!(project.language_version(), "1.9");
}

#[test]
fn deep_move_preserves_formal_inbound_outbound_resources_and_plain_bytes() {
    let ws = Workspace::new(&[
        ("world.wl", "include \"old.wl\" // old.wl stays\ninclude \"sibling.wl\"\ntag t as \"标签\"\nalias file \"old.wl\" as \"old.wl stays\"\nmark file \"old.wl\" with t\nattach file \"old.wl\" with picture\nevent start\n  [[file:old.wl|old.wl stays]] 普通 old.wl\n  choice \"[[file:old.wl|选 old.wl]]\"\n    -> END\n"),
        ("old.wl", "include \"sibling.wl\"\nasset picture file \"assets/picture.txt\" as \"原附件\"\nevent chapter\n  [[file:sibling.wl|兄弟]]\n  -> END\n"),
        ("sibling.wl", "event sibling\n  兄弟正文\n  -> END\n"),
        ("assets/picture.txt", "same asset bytes"),
    ]);
    let mut project = ws.open();
    assert!(
        !project.compile().has_errors(),
        "{:?}",
        project.compile().diagnostics
    );
    let before = project.clone();
    let original = project.compile();
    let plan = project
        .preview_source_lifecycle(&request("old.wl", "章节/第一 卷/新章.wl"))
        .unwrap();
    assert_eq!(project.content_baseline(), before.content_baseline());
    assert_eq!(
        plan.runtime_fingerprint_before,
        plan.runtime_fingerprint_after
    );
    assert_eq!(plan.entry_before, plan.entry_after);
    let expected_asset = project.root.join("assets").join("picture.txt");
    let asset = plan
        .resources
        .iter()
        .find(|resource| resource.field == "asset.path")
        .unwrap_or_else(|| panic!("asset.path missing; resources={:?}", plan.resources));
    assert_eq!(
        asset.resolved_before, expected_asset,
        "asset must resolve to the specific canonical workspace file; resource={asset:?}"
    );
    assert_eq!(
        asset.resolved_after, asset.resolved_before,
        "asset resolution must remain identical across the move; resource={asset:?}"
    );
    assert!(plan
        .changes
        .iter()
        .flat_map(|c| &c.occurrences)
        .any(|o| o.field.as_deref() == Some("outbound.asset.path")));
    project.apply_source_lifecycle_plan(&plan).unwrap();
    let expected_source = project.root.join("章节").join("第一 卷").join("新章.wl");
    assert_eq!(plan.destination_path.as_ref(), Some(&expected_source));
    let new_text = project.document(&expected_source).unwrap();
    assert!(new_text.contains("include \"../../sibling.wl\""));
    assert!(new_text.contains("\"../../assets/picture.txt\""));
    assert!(new_text.contains("[[file:../../sibling.wl|兄弟]]"));
    let entry = project.document(&project.entry).unwrap();
    assert!(entry.contains("// old.wl stays"));
    assert!(entry.contains("as \"old.wl stays\""));
    assert!(entry.contains("普通 old.wl"));
    assert!(entry.contains("[[file:章节/第一 卷/新章.wl|old.wl stays]]"));
    assert_eq!(
        project.compile().analysis.fingerprint,
        original.analysis.fingerprint
    );
    let after = project.clone();
    assert!(project.restore(before));
    assert!(project.document(&ws.0.join("old.wl")).is_ok());
    assert!(project.restore(after));
    project.save().unwrap();
    assert!(!ws.0.join("old.wl").exists());
    let mut reopened = ws.open();
    assert_eq!(
        reopened.compile().analysis.fingerprint,
        original.analysis.fingerprint
    );
    assert_eq!(reopened.compile().program.entry, original.program.entry);
}

#[test]
fn archived_move_keeps_membership_and_manifest_optional_bytes() {
    let ws = Workspace::new(&[
        ("world.wl", "include \"old.wl\"\nevent start\n  -> END\n"),
        ("old.wl", "// active\n"),
        ("archive.wl", "event secret\n  秘密\n  -> END\n"),
        (".world/project.json", EXPLICIT),
    ]);
    let mut project = ws.open();
    let plan = project
        .preview_source_lifecycle(&request("archive.wl", "归档/旧稿.wl"))
        .unwrap();
    assert_eq!(plan.membership, "archived");
    project.apply_source_lifecycle_plan(&plan).unwrap();
    let archived = project.root.join("归档").join("旧稿.wl");
    assert!(project.source_selection().unwrap().is_archived(&archived));
    assert!(!project.sources().contains_key(&archived));
    assert!(!project
        .compile()
        .program
        .events
        .iter()
        .any(|e| e.name == "secret"));
    assert!(std::str::from_utf8(
        project
            .authoring_document(&ws.0.join(".world/project.json"))
            .unwrap()
            .bytes()
    )
    .unwrap()
    .contains("\"extension\":{\"keep\":\"old.wl\"}"));
}

#[test]
fn preview_cancel_stale_tamper_and_destination_race_are_zero_write() {
    let ws = Workspace::new(&[
        ("world.wl", "include \"old.wl\"\nevent start\n  -> END\n"),
        ("old.wl", "// text\n"),
    ]);
    let mut project = ws.open();
    let baseline = project.content_baseline();
    let request = request("old.wl", "new.wl");
    assert!(project
        .preview_source_lifecycle_cancellable(&request, || true)
        .is_err());
    let plan = project.preview_source_lifecycle(&request).unwrap();
    let mut altered = plan.clone();
    altered.entry_after = "forged".into();
    assert!(project.apply_source_lifecycle_plan(&altered).is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert!(project
        .apply_source_lifecycle_cancellable(&request, &plan.plan_digest, || true)
        .is_err());
    std::fs::write(ws.0.join("new.wl"), "external").unwrap();
    assert!(project.apply_source_lifecycle_plan(&plan).is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(
        std::fs::read_to_string(ws.0.join("new.wl")).unwrap(),
        "external"
    );
    std::fs::remove_file(ws.0.join("new.wl")).unwrap();
    let entry = project.entry.clone();
    project
        .set_text(&entry, "include \"old.wl\"\nevent other\n  -> END\n".into())
        .unwrap();
    let modified = project.content_baseline();
    assert!(project.apply_source_lifecycle_plan(&plan).is_err());
    assert_eq!(project.content_baseline(), modified);
}

#[test]
fn external_source_or_resource_changes_invalidate_plan() {
    let ws = Workspace::new(&[
        ("world.wl", "include \"old.wl\"\nevent start\n  -> END\n"),
        ("old.wl", "asset a file \"a.txt\"\n"),
        ("a.txt", "before"),
    ]);
    let mut project = ws.open();
    let plan = project
        .preview_source_lifecycle(&request("old.wl", "深/new.wl"))
        .unwrap();
    let baseline = project.content_baseline();
    std::fs::write(ws.0.join("a.txt"), "after").unwrap();
    assert!(project.apply_source_lifecycle_plan(&plan).is_err());
    assert_eq!(project.content_baseline(), baseline);
    std::fs::write(ws.0.join("a.txt"), "before").unwrap();
    std::fs::write(ws.0.join("old.wl"), "// external").unwrap();
    assert!(project.apply_source_lifecycle_plan(&plan).is_err());
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn implicit_semantic_load_order_change_is_rejected_even_if_fingerprint_same() {
    let ws = Workspace::new(&[
        ("world.wl", "event start\n  -> END\n"),
        ("a.wl", "tag a\n"),
        ("b.wl", "tag b\n"),
    ]);
    let project = ws.open();
    let baseline = project.content_baseline();
    let error = project
        .preview_source_lifecycle(&request("a.wl", "z.wl"))
        .unwrap_err();
    assert!(error.contains("加载顺序"), "{error}");
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn path_safety_entry_case_collision_missing_assets_and_unknown_features_reject() {
    let ws = Workspace::new(&[
        ("world.wl", "include \"old.wl\"\nevent start\n  -> END\n"),
        ("old.wl", "// text\n"),
        ("Existing.wl", "// existing\n"),
    ]);
    let project = ws.open();
    for (from, to) in [
        ("world.wl", "entry.wl"),
        ("old.wl", "OLD.wl"),
        ("old.wl", "existing.wl"),
        ("old.wl", "../escape.wl"),
        ("old.wl", "world.wl/child.wl"),
    ] {
        assert!(
            project
                .preview_source_lifecycle(&request(from, to))
                .is_err(),
            "{from} -> {to}"
        );
    }
    let mut project = project;
    project
        .set_text(
            &ws.0.join("old.wl"),
            "asset a file \"missing.txt\"\n".into(),
        )
        .unwrap();
    assert!(project
        .preview_source_lifecycle(&request("old.wl", "new.wl"))
        .is_err());
    let manifest = ws.0.join(".world/project.json");
    project
        .create_authoring_document(
            &manifest,
            br#"{"schema_version":1,"required_features":[],"maps":{}}"#.to_vec(),
        )
        .unwrap();
    let baseline = project.content_baseline();
    assert!(project
        .set_authoring_document(
            &manifest,
            br#"{"schema_version":1,"required_features":["future.required"]}"#.to_vec()
        )
        .is_err());
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn legacy_create_and_saved_move_undo_redo_reuse_save_baselines() {
    let ws = Workspace::new(&[("world.wl", "event start\n  -> END\n")]);
    let mut project = ws.open();
    project.add_file(Path::new("old.wl")).unwrap();
    assert!(project.source_selection().is_none());
    project.save().unwrap();
    let before = project.clone();
    let plan = project
        .preview_source_lifecycle(&request("old.wl", "new.wl"))
        .unwrap();
    project.apply_source_lifecycle_plan(&plan).unwrap();
    project.save().unwrap();
    let after = project.clone();
    assert!(project.restore(before));
    project.save().unwrap();
    assert!(ws.0.join("old.wl").exists());
    assert!(!ws.0.join("new.wl").exists());
    assert!(project.restore(after));
    project.save().unwrap();
    assert!(!ws.0.join("old.wl").exists());
    assert!(ws.0.join("new.wl").exists());
}

#[test]
fn file_state_owner_fingerprint_guard_refuses_without_migrating_saves() {
    let ws = Workspace::new(&[("world.wl", "include \"old.wl\"\ntag ready\nstate file_state on file \"old.wl\" with ready\nevent start\n  -> END\n"), ("old.wl", "// source\n")]);
    let mut project = ws.open();
    assert!(
        !project.compile().has_errors(),
        "{:?}",
        project.compile().diagnostics
    );
    let baseline = project.content_baseline();
    let error = project
        .preview_source_lifecycle(&request("old.wl", "new.wl"))
        .unwrap_err();
    assert!(error.contains("指纹") && error.contains("Story"), "{error}");
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn source_move_handles_say_fragment_scope_and_same_name_other_kind() {
    let ws = Workspace::new(&[
        ("world.wl", "include \"old.wl\"\ncharacter speaker as \"说话者\"\nentity old kind place\nentity other kind place\nrelation_type nearby\nrelation_def r type nearby from entity old to entity other\n  scope_ref file \"old.wl\"\nevent start\n  say speaker \"[[file:old.wl|章节]] [[entity:old|同名实体]]\"\n  call part()\n  -> END\n"),
        ("old.wl", "fragment part()\n  [[file:world.wl|入口]]\n"),
        (".world/project.json", "{\"schema_version\":1,\"language_version\":\"1.11\",\"required_features\":[\"content.entities.v1\",\"content.relations.v1\"]}"),
    ]);
    let mut project = ws.open();
    assert!(
        !project.compile().has_errors(),
        "{:?}",
        project.compile().diagnostics
    );
    let fingerprint = project.compile().analysis.fingerprint;
    let plan = project
        .preview_source_lifecycle(&request("old.wl", "章节/new.wl"))
        .unwrap();
    project.apply_source_lifecycle_plan(&plan).unwrap();
    let source = project.document(&project.entry).unwrap();
    assert!(source.contains("[[file:章节/new.wl|章节]] [[entity:old|同名实体]]"));
    assert!(source.contains("scope_ref file \"章节/new.wl\""));
    assert!(source.contains("entity old kind place"));
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
}

#[cfg(unix)]
#[test]
fn symlink_source_destination_and_parent_case_alias_are_rejected() {
    use std::os::unix::fs::symlink;
    let ws = Workspace::new(&[
        ("world.wl", "include \"old.wl\"\nevent start\n  -> END\n"),
        ("old.wl", "// source\n"),
        ("Upper/existing.txt", "ordinary"),
    ]);
    let project = ws.open();
    assert!(project
        .preview_source_lifecycle(&request("old.wl", "upper/new.wl"))
        .is_err());
    symlink(ws.0.join("old.wl"), ws.0.join("link.wl")).unwrap();
    let baseline = project.content_baseline();
    assert!(project
        .preview_source_lifecycle(&request("old.wl", "new.wl"))
        .is_err());
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn cancellation_at_final_commit_boundary_preserves_the_whole_candidate() {
    let ws = Workspace::new(&[
        ("world.wl", "include \"old.wl\"\nevent start\n  -> END\n"),
        ("old.wl", "// source\n"),
    ]);
    let mut project = ws.open();
    let request = request("old.wl", "new.wl");
    let mut checks = 0;
    let plan = project
        .preview_source_lifecycle_cancellable(&request, || {
            checks += 1;
            false
        })
        .unwrap();
    let baseline = project.content_baseline();
    let mut apply_checks = 0;
    assert!(project
        .apply_source_lifecycle_cancellable(&request, &plan.plan_digest, || {
            apply_checks += 1;
            apply_checks > checks
        })
        .is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert!(ws.0.join("old.wl").exists());
    assert!(!ws.0.join("new.wl").exists());
}

#[cfg(unix)]
#[test]
fn read_only_source_refuses_before_any_buffer_changes() {
    use std::os::unix::fs::PermissionsExt;
    let ws = Workspace::new(&[
        ("world.wl", "include \"old.wl\"\nevent start\n  -> END\n"),
        ("old.wl", "// source\n"),
    ]);
    let project = ws.open();
    let baseline = project.content_baseline();
    std::fs::set_permissions(ws.0.join("old.wl"), std::fs::Permissions::from_mode(0o444)).unwrap();
    let error = project
        .preview_source_lifecycle(&request("old.wl", "new.wl"))
        .unwrap_err();
    assert!(error.contains("只读"), "{error}");
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn raw_attachment_of_an_inbound_rewritten_source_is_not_silently_changed() {
    let ws = Workspace::new(&[
        ("world.wl", "include \"old.wl\"\nevent start\n  -> END\n"),
        ("old.wl", "asset manuscript file \"world.wl\"\n"),
    ]);
    let project = ws.open();
    let baseline = project.content_baseline();
    let error = project
        .preview_source_lifecycle(&request("old.wl", "new.wl"))
        .unwrap_err();
    assert!(error.contains("原始附件"), "{error}");
    assert_eq!(project.content_baseline(), baseline);
}

#[cfg(unix)]
#[test]
fn direct_create_and_include_readonly_entry_and_manifest_are_zero_change() {
    use std::os::unix::fs::PermissionsExt;
    let ws = Workspace::new(&[
        ("world.wl", "event start\n  -> END\n"),
        ("old.wl", "// source\n"),
        ("archive.wl", "// archive\n"),
        (".world/project.json", EXPLICIT),
    ]);
    let mut project = ws.open();
    let baseline = project.content_baseline();
    let entry = ws.0.join("world.wl");
    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o444)).unwrap();
    assert!(project
        .add_file(Path::new("new.wl"))
        .unwrap_err()
        .contains("只读"));
    assert_eq!(project.content_baseline(), baseline);
    assert!(project
        .include_file(&ws.0.join("old.wl"))
        .unwrap_err()
        .contains("只读"));
    assert_eq!(project.content_baseline(), baseline);
    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(
        ws.0.join(".world/project.json"),
        std::fs::Permissions::from_mode(0o444),
    )
    .unwrap();
    assert!(project
        .add_file(Path::new("new.wl"))
        .unwrap_err()
        .contains("只读"));
    assert_eq!(project.content_baseline(), baseline);
    assert!(!ws.0.join("new.wl").exists());
}

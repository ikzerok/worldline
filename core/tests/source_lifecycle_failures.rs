//! 失败类型在 core 守卫原点给出；旧 String 入口只投影 message，不解析中文。
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::project::Project;
use worldline_core::source_lifecycle::{
    SourceLifecycleFailure as Failure, SourceLifecycleFailureKind as Kind,
    SourceLifecycleRequest as Request,
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wl-failure-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        let path = Project::new(&path).root;
        fs::write(
            path.join("world.wl"),
            "include \"people.wl\"\nevent entry\n  -> END\n",
        )
        .unwrap();
        fs::write(path.join("people.wl"), "character a\ncharacter b\n").unwrap();
        Self(path)
    }
    fn open(&self) -> Project {
        Project::open(&self.0).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn moved(to: &str) -> Request {
    Request::Move {
        from: "people.wl".into(),
        to: to.into(),
    }
}
fn assert_preview_error(project: &Project, request: &Request, kind: Kind) -> Failure {
    let baseline = project.content_baseline();
    let sources = project.sources();
    let failure = project
        .preview_source_lifecycle_classified(request)
        .unwrap_err();
    assert_eq!(failure.kind, kind, "{}", failure.message);
    assert_eq!(
        project.preview_source_lifecycle(request).unwrap_err(),
        failure.message
    );
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.sources(), sources);
    failure
}

#[test]
fn illegal_paths_are_typed_at_the_guard_and_legacy_details_are_unchanged() {
    let ws = Workspace::new();
    let project = ws.open();
    for to in ["../escape.wl", "world.wl", "world.wl/child.wl", "新\\章.wl"] {
        assert_preview_error(&project, &moved(to), Kind::IllegalPath);
    }
    let request = Request::Move {
        from: "world.wl".into(),
        to: "entry.wl".into(),
    };
    let failure = assert_preview_error(&project, &request, Kind::IllegalPath);
    assert_eq!(
        serde_json::to_value(&failure).unwrap()["kind"],
        "illegal_path"
    );
    assert!(!ws.0.join("entry.wl").exists());
}

#[test]
fn stale_preview_external_changes_and_new_source_are_source_changed() {
    let ws = Workspace::new();
    let mut project = ws.open();
    let request = moved("资料/people.wl");
    let plan = project
        .preview_source_lifecycle_classified(&request)
        .unwrap();
    assert_eq!(plan, project.preview_source_lifecycle(&request).unwrap());
    let mut changed = plan.clone();
    changed.entry_after = "tampered".into();
    let baseline = project.content_baseline();
    assert_eq!(
        project
            .apply_source_lifecycle_plan_classified(&changed)
            .unwrap_err()
            .kind,
        Kind::SourceChanged
    );
    assert_eq!(
        project
            .apply_source_lifecycle_classified(&request, "stale")
            .unwrap_err()
            .kind,
        Kind::SourceChanged
    );
    assert_eq!(project.content_baseline(), baseline);
    let mut edited = project.clone();
    edited
        .set_text(&edited.entry.clone(), "event entry\n  ${\n".into())
        .unwrap();
    let dirty = edited.sources();
    assert_eq!(
        edited
            .apply_source_lifecycle_plan_classified(&plan)
            .unwrap_err()
            .kind,
        Kind::SourceChanged
    );
    assert_eq!(edited.sources(), dirty);
    fs::write(ws.0.join("people.wl"), "// external\n").unwrap();
    let failure = project
        .apply_source_lifecycle_plan_classified(&plan)
        .unwrap_err();
    assert_eq!(failure.kind, Kind::SourceChanged);
    assert_eq!(
        project.apply_source_lifecycle_plan(&plan).unwrap_err(),
        failure.message
    );
    assert_eq!(project.content_baseline(), baseline);
    fs::write(ws.0.join("people.wl"), "character a\ncharacter b\n").unwrap();
    fs::write(ws.0.join("new.wl"), "// new\n").unwrap();
    assert_preview_error(&project, &request, Kind::SourceChanged);
    assert!(!ws.0.join("资料/people.wl").exists());
}

#[test]
fn known_runtime_and_loading_changes_are_semantic_change() {
    let ws = Workspace::new();
    fs::write(ws.0.join("world.wl"), "include \"people.wl\"\ntag ready\nstate s on file \"people.wl\" with ready\nevent entry\n  -> END\n").unwrap();
    assert_preview_error(&ws.open(), &moved("new.wl"), Kind::SemanticChange);
    fs::write(ws.0.join("world.wl"), "// implicit source order\n").unwrap();
    fs::write(ws.0.join("a.wl"), "event first\n  -> END\n").unwrap();
    fs::write(ws.0.join("b.wl"), "event second\n  -> END\n").unwrap();
    let request = Request::Move {
        from: "a.wl".into(),
        to: "z.wl".into(),
    };
    assert_preview_error(&ws.open(), &request, Kind::SemanticChange);
}

#[test]
fn unknown_errors_bad_source_budgets_and_cancel_never_guess_semantic_change() {
    // 即使底层消息看起来与某个类别完全相同，也不按文字赋类。
    for message in [
        "移动改变运行身份/指纹",
        "源码组织预览已过期",
        "目标路径不允许链接或目录联接",
    ] {
        let failure = Failure::from(message);
        assert_eq!(failure.kind, Kind::UnableToProve);
        assert_eq!(failure.message, message);
    }
    let ws = Workspace::new();
    let mut project = ws.open();
    let request = moved("new.wl");
    let baseline = project.content_baseline();
    let error = project
        .preview_source_lifecycle_cancellable_classified(&request, || true)
        .unwrap_err();
    assert_eq!(error.kind, Kind::UnableToProve);
    assert_eq!(project.content_baseline(), baseline);
    let plan = project
        .preview_source_lifecycle_classified(&request)
        .unwrap();
    assert_eq!(
        project
            .apply_source_lifecycle_cancellable_classified(&request, &plan.plan_digest, || true)
            .unwrap_err()
            .kind,
        Kind::UnableToProve
    );
    project
        .set_text(&project.entry.clone(), "event entry\n  ${\n".into())
        .unwrap();
    assert_preview_error(&project, &request, Kind::UnableToProve);
    let mut large = ws.open();
    let document = large.documents[&large.entry].clone();
    for i in 0..4096 {
        large
            .documents
            .insert(ws.0.join(format!("extra-{i}.wl")), document.clone());
    }
    assert_preview_error(&large, &request, Kind::UnableToProve);
}

#[test]
fn typed_success_has_the_same_plan_and_transaction_as_legacy() {
    let ws = Workspace::new();
    let mut typed = ws.open();
    let mut legacy = ws.open();
    let request = moved("新章/people.wl");
    let plan = typed.preview_source_lifecycle_classified(&request).unwrap();
    assert_eq!(plan, legacy.preview_source_lifecycle(&request).unwrap());
    assert_eq!(
        typed.apply_source_lifecycle_plan_classified(&plan).unwrap(),
        legacy.apply_source_lifecycle_plan(&plan).unwrap()
    );
    assert_eq!(typed.sources(), legacy.sources());
    assert!(!ws.0.join("新章/people.wl").exists());
    typed.save().unwrap();
    assert_eq!(ws.open().sources(), typed.sources());
    assert!(!ws.0.join("people.wl").exists());
}

#[cfg(unix)]
#[test]
fn link_inventory_and_readonly_write_guards_are_illegal_path() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let ws = Workspace::new();
    let project = ws.open();
    let request = moved("new.wl");
    symlink(ws.0.join("people.wl"), ws.0.join("linked.wl")).unwrap();
    assert_preview_error(&project, &request, Kind::IllegalPath);
    fs::remove_file(ws.0.join("linked.wl")).unwrap();
    fs::set_permissions(ws.0.join("people.wl"), fs::Permissions::from_mode(0o444)).unwrap();
    assert_preview_error(&project, &request, Kind::IllegalPath);
    fs::set_permissions(ws.0.join("people.wl"), fs::Permissions::from_mode(0o644)).unwrap();
}

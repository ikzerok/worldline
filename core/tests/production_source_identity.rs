#![cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::{
    production_script::{
        ProductionExportOptions, ProductionFormat, ProductionScope, ProductionScriptRequest,
    },
    project::Project,
    TargetRef,
};

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture(second: &str, include_first: bool) -> (Workspace, Project) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "production-source-identity-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(
        root.join("world.wl"),
        "character a\ncharacter b\nevent start\n  say a \"ENTRY_A\"\n  -> END\n",
    )
    .unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#,
    )
    .unwrap();
    if include_first {
        fs::create_dir_all(root.join("a")).unwrap();
        fs::write(
            root.join("a/b.wl"),
            "event visible\n  say a \"PUBLICABODY\"\n  -> END\n",
        )
        .unwrap();
    }
    let other = root.join(second);
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(
        other,
        "event private\n  say b \"PRIVATEBBODY\" direction \"PRIVATEBNOTE\"\n  -> END\n",
    )
    .unwrap();
    // 使用真实原生 Project 加载，不借助会提前拒绝不便携文件名的只读快照入口。
    let project = Project::open(&root).unwrap();
    let compiled = project.compile_read_only().unwrap();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    (Workspace(root), project)
}
fn request() -> ProductionScriptRequest {
    let mut request = ProductionScriptRequest::new(ProductionScope::Project);
    request.speaker = Some(TargetRef::new("character", "a"));
    request
}

#[cfg(unix)]
#[test]
fn production_rejects_lossy_path_alias_before_selected_speaker_can_receive_other_body() {
    let (_work, project) = fixture("a\\b.wl", true);
    let first = project.root.join("a/b.wl");
    let second = project.root.join("a\\b.wl");
    assert_ne!(first, second);
    assert!(first.is_file() && second.is_file());
    let first_bytes = fs::read(&first).unwrap();
    let second_bytes = fs::read(&second).unwrap();
    assert_ne!(first_bytes, second_bytes);
    let baseline = project.content_baseline();
    let error = project
        .production_script_snapshot(&[], &[], &request())
        .unwrap_err();
    assert_eq!(error.code, "INVALID_SOURCE");
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(fs::read(first).unwrap(), first_bytes);
    assert_eq!(fs::read(second).unwrap(), second_bytes);
}

#[cfg(unix)]
#[test]
fn production_path_identity_guard_precedes_scope_and_speaker_filters() {
    let (_work, project) = fixture("a\\b.wl", true);
    let mut input = request();
    input.scope = ProductionScope::CurrentTarget {
        target: TargetRef::new("event", "start"),
    };
    input.search = "ENTRY_A".into();
    let error = project
        .production_script_snapshot(&[], &[], &input)
        .unwrap_err();
    assert_eq!(error.code, "INVALID_SOURCE");
}

#[cfg(unix)]
#[test]
fn production_rejects_single_non_roundtrippable_source_even_without_a_collision_peer() {
    let (_work, project) = fixture("a\\b.wl", false);
    assert!(!project.root.join("a/b.wl").exists());
    let mut input = request();
    input.speaker = Some(TargetRef::new("character", "b"));
    let error = project
        .production_script_snapshot(&[], &[], &input)
        .unwrap_err();
    assert_eq!(error.code, "INVALID_SOURCE");
}

#[test]
fn production_distinct_portable_directories_keep_exact_source_and_speaker_in_all_formats() {
    let (_work, project) = fixture("other/b.wl", true);
    let snapshot = project
        .production_script_snapshot(&[], &[], &request())
        .unwrap();
    let page = snapshot.page(0, 100).unwrap();
    assert_eq!(page.total, 2);
    let row = page
        .rows
        .iter()
        .find(|row| row.source.file == "a/b.wl")
        .unwrap();
    assert_eq!(row.speaker.as_ref().unwrap().target.id, "a");
    let hit = project
        .production_script_source_hit(&[], &[], &snapshot, &row.row_key)
        .unwrap();
    assert_eq!(hit.path, project.root.join("a/b.wl"));
    assert_eq!(hit.line, 2);
    assert!(hit.preview.contains("PUBLICABODY"));
    assert!(!hit.preview.contains("PRIVATEBBODY"));
    for format in [
        ProductionFormat::Json,
        ProductionFormat::Markdown,
        ProductionFormat::Csv,
    ] {
        let artifact = snapshot
            .export(&ProductionExportOptions {
                schema_version: 1,
                format,
                include_direction: true,
            })
            .unwrap();
        let output = String::from_utf8(artifact.bytes().to_vec()).unwrap();
        assert!(output.contains("PUBLICABODY"));
        assert!(!output.contains("PRIVATEBBODY"));
        assert!(!output.contains("PRIVATEBNOTE"));
    }
}

#[cfg(unix)]
#[test]
fn production_path_guard_precedes_valid_locale_and_status_filters() {
    let (_work, original) = fixture("a\\b.wl", true);
    fs::write(
        original.root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.11","required_features":["content.localization.v1"],"localizations":{"en":".world/en.json"}}"#,
    )
    .unwrap();
    fs::write(
        original.root.join(".world/en.json"),
        r#"{"schema_version":1,"required_features":["content.localization.v1"],"source_locale":"zh","target_locale":"en","entries":{}}"#,
    )
    .unwrap();
    let project = Project::open(&original.root).unwrap();
    let mut input = request();
    input.target_locale = Some("en".into());
    input.statuses = vec![worldline_core::production_script::ProductionStatus::Translated];
    input.locale_policy = worldline_core::production_script::ProductionLocalePolicy::SourceFallback;
    let error = project
        .production_script_snapshot(&[], &[], &input)
        .unwrap_err();
    assert_eq!(error.code, "INVALID_SOURCE");
}

#[test]
fn production_unicode_native_path_components_remain_exact_and_navigable() {
    // Windows 的系统目录分隔符合法；不可把它与 Unix 文件名中的字面反斜杠混淆。
    let (_work, project) = fixture("另一目录/雪.wl", true);
    let mut input = request();
    input.speaker = Some(TargetRef::new("character", "b"));
    let snapshot = project
        .production_script_snapshot(&[], &[], &input)
        .unwrap();
    let page = snapshot.page(0, 100).unwrap();
    assert_eq!(page.total, 1);
    let row = &page.rows[0];
    assert_eq!(row.source.file, "另一目录/雪.wl");
    let hit = project
        .production_script_source_hit(&[], &[], &snapshot, &row.row_key)
        .unwrap();
    assert_eq!(hit.path, project.root.join("另一目录").join("雪.wl"));
    assert_eq!(hit.line, 2);
    assert!(hit.preview.contains("PRIVATEBBODY"));
}

#[cfg(unix)]
#[test]
fn production_refuses_non_utf8_source_identity_before_lossy_compilation() {
    use std::os::unix::ffi::OsStringExt;
    let (_work, original) = fixture("other/b.wl", true);
    let opaque = original
        .root
        .join(std::ffi::OsString::from_vec(b"opaque-\xff.wl".to_vec()));
    fs::write(&opaque, "event opaque\n  say b \"OPAQUEBODY\"\n  -> END\n").unwrap();
    let project = Project::open(&original.root).unwrap();
    assert!(opaque.is_file());
    let baseline = project.content_baseline();
    let error = project
        .production_script_snapshot(&[], &[], &request())
        .unwrap_err();
    assert_eq!(error.code, "INVALID_SOURCE");
    assert_eq!(project.content_baseline(), baseline);
    assert!(opaque.is_file());
}

#[cfg(unix)]
#[test]
fn production_refresh_with_ambiguous_path_invalidates_navigation_and_refuses_new_snapshot() {
    let (_work, mut project) = fixture("other/b.wl", true);
    let snapshot = project
        .production_script_snapshot(&[], &[], &request())
        .unwrap();
    let page = snapshot.page(0, 100).unwrap();
    let row = page
        .rows
        .iter()
        .find(|row| row.source.file == "a/b.wl")
        .unwrap();
    fs::write(
        project.root.join("a\\b.wl"),
        "event later_private\n  say b \"LATERPRIVATE\"\n  -> END\n",
    )
    .unwrap();
    project.refresh().unwrap();
    assert_eq!(
        project
            .validate_production_script(&[], &[], &snapshot)
            .unwrap_err()
            .code,
        "STALE_SNAPSHOT"
    );
    assert_eq!(
        project
            .production_script_source_hit(&[], &[], &snapshot, &row.row_key)
            .unwrap_err()
            .code,
        "STALE_SNAPSHOT"
    );
    assert_eq!(
        project
            .production_script_snapshot(&[], &[], &request())
            .unwrap_err()
            .code,
        "INVALID_SOURCE"
    );
}

#[test]
fn production_public_rows_and_calls_keep_relative_display_path_order() {
    let (_work, original) = fixture("a-foo.wl", true);
    for relative in ["a/b.wl", "a-foo.wl"] {
        let path = original.root.join(relative);
        let text = fs::read_to_string(&path).unwrap();
        fs::write(path, text.replace("  -> END", "  call shared()\n  -> END")).unwrap();
    }
    let source = fs::read_to_string(&original.entry).unwrap();
    fs::write(
        &original.entry,
        format!("{source}fragment shared()\n  return\n"),
    )
    .unwrap();
    let project = Project::open(&original.root).unwrap();
    let mut input = request();
    input.speaker = None;
    let snapshot = project
        .production_script_snapshot(&[], &[], &input)
        .unwrap();
    let page = snapshot.page(0, 100).unwrap();
    assert_eq!(
        page.rows
            .iter()
            .map(|row| row.source.file.as_str())
            .collect::<Vec<_>>(),
        vec!["a-foo.wl", "a/b.wl", "world.wl"],
    );
    assert_eq!(
        snapshot
            .call_sites()
            .iter()
            .map(|call| call.source.file.as_str())
            .collect::<Vec<_>>(),
        vec!["a-foo.wl", "a/b.wl"],
    );
}

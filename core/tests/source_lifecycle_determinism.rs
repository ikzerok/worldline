//! 同一稿件的真实预览/重算/保存回归；不得重试失败来掩盖随机拒绝。
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::catalog::TargetRef;
use worldline_core::project::Project;
use worldline_core::reader_export::{
    ReaderExportSelection, READER_FIELDS_FEATURE, READER_SITE_FEATURE,
};
use worldline_core::source_lifecycle::{SourceLifecyclePlan, SourceLifecycleRequest as Request};

const RUNS: usize = 100;
const MINIMAL: &[(&str, &str)] = &[
    (
        "world.wl",
        "include \"people.wl\"\nevent entry with lin, ling\n  两人物一事件\n  -> END\n",
    ),
    (
        "people.wl",
        "character lin as \"林\"\ncharacter ling as \"绫\"\n",
    ),
];
const SALT_MIST: &[(&str, &str)] = &[
    (
        "world.wl",
        include_str!("fixtures/source_lifecycle/salt_mist/world.wl"),
    ),
    (
        "设定/人物与地点.wl",
        include_str!("fixtures/source_lifecycle/salt_mist/people.wl"),
    ),
    (
        ".world/project.json",
        include_str!("fixtures/source_lifecycle/salt_mist/project.json"),
    ),
];

struct Workspace(PathBuf);
impl Workspace {
    fn new(files: &[(&str, &str)]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wl-determinism-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        let root = Project::new(&path).root;
        for (relative, text) in files {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        fs::write(root.join("unreferenced.txt"), b"UNREFERENCED_PRIVATE_BYTES").unwrap();
        Self(root)
    }
    fn open(&self) -> Project {
        Project::open(&self.0).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
        let _ = fs::remove_file(self.0.with_extension("expected.json"));
    }
}
fn request(from: &str) -> Request {
    Request::Move {
        from: from.into(),
        to: "资料/船员.wl".into(),
    }
}
fn checked_compile(project: &mut Project) -> worldline_core::CompileResult {
    let result = project.compile();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}
fn snapshot(project: &mut Project, request: &Request) -> serde_json::Value {
    let plan = project.preview_source_lifecycle(request).unwrap();
    let catalog = checked_compile(project).analysis.catalog;
    serde_json::json!({"plan": plan, "catalog": catalog})
}

#[test]
fn minimal_and_salt_mist_preview_apply_are_stable_for_100_independent_projects() {
    for (files, source) in [(MINIMAL, "people.wl"), (SALT_MIST, "设定/人物与地点.wl")] {
        let ws = Workspace::new(files);
        let mut original = ws.open();
        let before_sources = original.sources();
        let before = checked_compile(&mut original);
        let request = request(source);
        let plan = original.preview_source_lifecycle(&request).unwrap();
        let expected = snapshot(&mut original, &request);
        let baseline = original.content_baseline();
        for iteration in 0..RUNS {
            assert_eq!(
                snapshot(&mut original, &request),
                expected,
                "same Project preview {iteration}: {source}"
            );
            // 同根独立打开，计划绑定的是同一源路径与字节，而非重写根目录后的新计划。
            let mut candidate = ws.open();
            assert_eq!(
                candidate.apply_source_lifecycle_plan(&plan).unwrap(),
                plan,
                "apply {iteration}: {source}"
            );
            assert_eq!(
                checked_compile(&mut candidate).analysis.fingerprint,
                before.analysis.fingerprint
            );
            assert!(candidate.document(&ws.0.join(source)).is_err());
            assert!(candidate.document(&ws.0.join("资料/船员.wl")).is_ok());
            assert!(candidate.restore(original.clone()));
            assert_eq!(
                candidate.sources(),
                before_sources,
                "undo {iteration}: {source}"
            );
        }
        assert_eq!(original.content_baseline(), baseline);
        assert!(!ws.0.join("资料/船员.wl").exists());
        assert_saved_move_roundtrip(&ws, original, &plan, &before_sources);
    }
}

fn assert_saved_move_roundtrip(
    ws: &Workspace,
    mut project: Project,
    plan: &SourceLifecyclePlan,
    before_sources: &std::collections::BTreeMap<PathBuf, String>,
) {
    let undo = project.clone();
    project.apply_source_lifecycle_plan(plan).unwrap();
    let expected = checked_compile(&mut project);
    project.save().unwrap();
    assert!(!plan.source_path.as_ref().unwrap().exists());
    assert!(plan.destination_path.as_ref().unwrap().exists());
    let mut reopened = ws.open();
    let after = checked_compile(&mut reopened);
    assert_eq!(after.program.entry, expected.program.entry);
    assert_eq!(after.program.files, expected.program.files);
    assert_eq!(after.analysis.fingerprint, plan.runtime_fingerprint_before);
    assert_eq!(
        serde_json::to_value(after.analysis.catalog).unwrap(),
        serde_json::to_value(expected.analysis.catalog).unwrap()
    );
    assert_eq!(
        fs::read(ws.0.join("unreferenced.txt")).unwrap(),
        b"UNREFERENCED_PRIVATE_BYTES"
    );
    assert!(project.restore(undo));
    assert_eq!(&project.sources(), before_sources);
    project.save().unwrap();
    assert_eq!(&ws.open().sources(), before_sources);
    for (path, text) in before_sources {
        assert_eq!(fs::read(path).unwrap(), text.as_bytes());
    }
    assert!(!plan.destination_path.as_ref().unwrap().exists());
}

#[test]
fn minimal_and_salt_mist_preview_match_in_100_fresh_processes() {
    for (files, source) in [(MINIMAL, "people.wl"), (SALT_MIST, "设定/人物与地点.wl")] {
        let ws = Workspace::new(files);
        let expected_path = ws.0.with_extension("expected.json");
        fs::write(
            &expected_path,
            serde_json::to_vec(&snapshot(&mut ws.open(), &request(source))).unwrap(),
        )
        .unwrap();
        for iteration in 0..RUNS {
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "source_move_process_probe", "--nocapture"])
                .env("WL_MOVE_PROBE_ROOT", &ws.0)
                .env("WL_MOVE_PROBE_SOURCE", source)
                .env("WL_MOVE_PROBE_EXPECTED", &expected_path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "process {iteration}: {source}\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn source_move_process_probe() {
    let Some(root) = std::env::var_os("WL_MOVE_PROBE_ROOT") else {
        return;
    };
    let source = std::env::var("WL_MOVE_PROBE_SOURCE").unwrap();
    let expected: serde_json::Value = serde_json::from_slice(
        &fs::read(std::env::var_os("WL_MOVE_PROBE_EXPECTED").unwrap()).unwrap(),
    )
    .unwrap();
    let mut project = Project::open(Path::new(&root)).unwrap();
    assert_eq!(snapshot(&mut project, &request(&source)), expected);
}

#[test]
fn same_line_duplicate_references_and_legacy_relations_survive_move() {
    let ws = Workspace::new(&[
        ("world.wl", "include \"people.wl\"\nevent entry with lin, ling\n  [[file:people.wl|人]] [[file:people.wl|人]]\n  -> END\n"),
        ("people.wl", "character lin as \"林\"\n  relation ling as \"伙伴\"\n  relation ling as \"伙伴\"\ncharacter ling as \"绫\"\n  relation lin as \"朋友\"\n"),
    ]);
    let mut project = ws.open();
    let before = checked_compile(&mut project);
    let refs = &before.analysis.catalog.references;
    let duplicate_count = refs
        .iter()
        .filter(|r| r.kind == "正文链接" && r.line == 3)
        .count();
    assert_eq!(duplicate_count, 2);
    assert_eq!(before.analysis.catalog.text_links.len(), 2);
    assert_eq!(before.analysis.catalog.legacy_relations.len(), 3);
    let plan = project
        .preview_source_lifecycle(&request("people.wl"))
        .unwrap();
    project.apply_source_lifecycle_plan(&plan).unwrap();
    let after = checked_compile(&mut project);
    assert_eq!(after.analysis.catalog.references.len(), refs.len());
    assert_eq!(
        after
            .analysis
            .catalog
            .references
            .iter()
            .filter(|r| r.kind == "正文链接" && r.line == 3)
            .count(),
        duplicate_count
    );
    assert_eq!(after.analysis.catalog.text_links.len(), 2);
    assert_eq!(after.analysis.catalog.legacy_relations.len(), 3);
    assert_eq!(after.analysis.fingerprint, before.analysis.fingerprint);
}

#[test]
fn changed_reference_target_and_executable_order_reject_old_plan_without_writes() {
    let ws = Workspace::new(&[
        ("world.wl", "include \"people.wl\"\nentity a kind place\nentity b kind place\nevent entry with lin, ling\n  [[entity:a|地方]]\n  choice \"甲\"\n    -> END\n  choice \"乙\"\n    -> END\n"),
        ("people.wl", "character lin\ncharacter ling\n"),
        (".world/project.json", "{\"schema_version\":1,\"language_version\":\"1.10\",\"required_features\":[]}"),
    ]);
    let mut original = ws.open();
    let original_fingerprint = checked_compile(&mut original).analysis.fingerprint;
    let plan = original
        .preview_source_lifecycle(&request("people.wl"))
        .unwrap();
    let source = original.document(&original.entry).unwrap().to_string();
    for changed_source in [
        source.replace("[[entity:a|", "[[entity:b|"),
        source
            .replace("choice \"甲\"", "choice \"TMP\"")
            .replace("choice \"乙\"", "choice \"甲\"")
            .replace("choice \"TMP\"", "choice \"乙\""),
    ] {
        let mut candidate = original.clone();
        candidate
            .set_text(&candidate.entry.clone(), changed_source.clone())
            .unwrap();
        let fingerprint = checked_compile(&mut candidate).analysis.fingerprint;
        if changed_source.contains("[[entity:b|") {
            assert_eq!(fingerprint, original_fingerprint);
        } else {
            assert_ne!(fingerprint, original_fingerprint);
        }
        let baseline = candidate.content_baseline();
        let sources = candidate.sources();
        assert!(candidate.apply_source_lifecycle_plan(&plan).is_err());
        assert_eq!(candidate.content_baseline(), baseline);
        assert_eq!(candidate.sources(), sources);
        assert_eq!(fs::read_to_string(&candidate.entry).unwrap(), source);
        assert!(!ws.0.join("资料/船员.wl").exists());
    }
}

#[test]
fn character_and_entity_rename_and_reader_v1_v3_remain_stable_for_100_runs() {
    let ws = Workspace::new(SALT_MIST);
    let project = ws.open();
    for target in [
        TargetRef::new("character", "lingzhou"),
        TargetRef::new("entity", "harbor_place"),
    ] {
        let new_id = format!("{}_new", target.id);
        let first = project.plan_rename_target(&target, &new_id).unwrap();
        for _ in 0..RUNS {
            assert_eq!(project.plan_rename_target(&target, &new_id).unwrap(), first);
            let mut candidate = ws.open();
            candidate.apply_rename_plan(&first).unwrap();
            checked_compile(&mut candidate);
        }
    }
    for schema_version in [1, 3] {
        let selection = ReaderExportSelection {
            schema_version,
            site_title: "盐雾公开审阅".into(),
            required_features: if schema_version == 3 {
                vec![READER_SITE_FEATURE.into(), READER_FIELDS_FEATURE.into()]
            } else {
                Vec::new()
            },
            objects: vec![TargetRef::new("entity", "harbor_place")],
            fields: Vec::new(),
            manuscripts: Vec::new(),
            attachments: Vec::new(),
            maps: Vec::new(),
        };
        let first = project.preview_reader_export(&selection).unwrap();
        let expected = project
            .build_reader_export(&selection, &first.plan_digest)
            .unwrap();
        for _ in 0..RUNS {
            assert_eq!(project.preview_reader_export(&selection).unwrap(), first);
            assert_eq!(
                project
                    .build_reader_export(&selection, &first.plan_digest)
                    .unwrap(),
                expected
            );
        }
        let package = expected
            .values()
            .flat_map(|bytes| bytes.iter().copied())
            .collect::<Vec<_>>();
        let text = String::from_utf8_lossy(&package);
        for excluded in [
            "海图员",
            "绫舟只是普通文字",
            "议会正式管辖雾港",
            "UNREFERENCED_PRIVATE_BYTES",
            "belong",
        ] {
            assert!(
                !text.contains(excluded),
                "v{schema_version} leaked {excluded}"
            );
        }
    }
}

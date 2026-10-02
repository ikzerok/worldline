use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "reader-profile-revalidation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".world")).unwrap();
        fs::write(root.join("world.wl"), "event opening\n  公开正文\n").unwrap();
        fs::write(
            root.join(".world/project.json"),
            br#"{"schema_version":1,"language_version":"1.9","required_features":[],"future_key":{"keep":true}}"#,
        )
        .unwrap();
        Self(root)
    }

    fn project(&self) -> Project {
        Project::open(&self.0).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn profile(project: &Project) -> ReaderPublicationProfile {
    project
        .create_reader_profile(
            "public",
            &ReaderExportSelection {
                schema_version: READER_SITE_SCHEMA_VERSION,
                required_features: vec![READER_SITE_FEATURE.into()],
                site_title: "公开阅读".into(),
                objects: vec![TargetRef::new("event", "opening")],
                fields: Vec::new(),
                maps: Vec::new(),
                manuscripts: Vec::new(),
                attachments: Vec::new(),
            },
        )
        .unwrap()
}

#[test]
fn fresh_rehearsal_rejects_disk_changes_injected_before_finish() {
    for change in ["source", "manifest", "profile", "directory"] {
        let fixture = Fixture::new();
        let mut project = fixture.project();
        let (plan, candidate) = project
            .prepare_save_reader_profile(&profile(&project))
            .unwrap();
        let baseline = project.content_baseline();
        let sources = project.sources();
        let documents: BTreeMap<_, _> = project
            .authoring_documents
            .iter()
            .map(|(path, document)| (path.clone(), document.bytes().to_vec()))
            .collect();
        let destination = fixture.0.join(&plan.document_path);
        match change {
            "source" => fs::write(fixture.0.join("world.wl"), "event changed\n").unwrap(),
            "manifest" => fs::write(
                fixture.0.join(".world/project.json"),
                br#"{"schema_version":99,"required_features":["future.v1"]}"#,
            )
            .unwrap(),
            "profile" => {
                fs::create_dir_all(destination.parent().unwrap()).unwrap();
                fs::write(&destination, b"external author bytes").unwrap();
            }
            "directory" => fs::create_dir_all(&destination).unwrap(),
            _ => unreachable!(),
        }
        assert!(
            project
                .finish_save_reader_profile(&plan, plan.clone(), candidate)
                .is_err(),
            "{change}"
        );
        assert_eq!(project.content_baseline(), baseline, "{change}");
        assert_eq!(project.sources(), sources, "{change}");
        assert_eq!(
            project
                .authoring_documents
                .iter()
                .map(|(path, document)| (path.clone(), document.bytes().to_vec()))
                .collect::<BTreeMap<_, _>>(),
            documents,
            "{change}"
        );
        assert!(!project.is_dirty(), "{change}");
        if change == "profile" {
            assert_eq!(fs::read(destination).unwrap(), b"external author bytes");
        }
    }
}

#[test]
fn fresh_rehearsal_preserves_unknown_fields_source_and_full_restore() {
    let fixture = Fixture::new();
    let mut project = fixture.project();
    let initial = project
        .preview_save_reader_profile(&profile(&project))
        .unwrap();
    project.apply_save_reader_profile(&initial).unwrap();
    project.save().unwrap();
    let destination = fixture.0.join(&initial.document_path);
    let mut stored: Value = serde_json::from_slice(&fs::read(&destination).unwrap()).unwrap();
    stored["future_extra"] = json!({"raw":"保留", "array":[1,2,3]});
    fs::write(&destination, serde_json::to_vec(&stored).unwrap()).unwrap();
    let mut project = fixture.project();
    let entry = project.entry.clone();
    project
        .set_text(&entry, "event opening\n  尚未保存的正文\n".into())
        .unwrap();
    let previous = project.clone();
    let before = project.export_files().unwrap();
    let options = project.compile_options();
    let sources = project.sources();
    let mut updated = project.reader_profiles().unwrap().remove(0);
    updated.title = "更新标题".into();
    let plan = project.preview_save_reader_profile(&updated).unwrap();
    assert_eq!(project.export_files().unwrap(), before);
    project.apply_save_reader_profile(&plan).unwrap();
    assert_eq!(project.sources(), sources);
    assert_eq!(project.compile_options(), options);
    assert!(project.is_dirty());
    let saved: Value =
        serde_json::from_slice(project.authoring_document(&destination).unwrap().bytes()).unwrap();
    assert_eq!(saved["future_extra"], stored["future_extra"]);
    assert_eq!(saved["title"], "更新标题");
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&destination).unwrap()).unwrap(),
        stored
    );
    assert!(project.restore(previous));
    assert_eq!(project.export_files().unwrap(), before);
}

#[test]
fn fresh_rehearsal_rejects_every_modified_plan_field_and_stale_source() {
    let fixture = Fixture::new();
    let project = fixture.project();
    let plan = project
        .preview_save_reader_profile(&profile(&project))
        .unwrap();
    let before = project.export_files().unwrap();
    for field in ["profile", "baseline", "path", "before_hash", "digest"] {
        let mut changed = plan.clone();
        match field {
            "profile" => changed.profile.title.push('改'),
            "baseline" => changed.content_baseline.push('0'),
            "path" => changed.document_path.push('x'),
            "before_hash" => changed.document_before_hash = Some("0".into()),
            "digest" => changed.plan_digest.push('0'),
            _ => unreachable!(),
        }
        let mut candidate = project.clone();
        assert!(
            candidate.apply_save_reader_profile(&changed).is_err(),
            "{field}"
        );
        assert_eq!(candidate.export_files().unwrap(), before, "{field}");
    }
    let mut stale = project.clone();
    let entry = stale.entry.clone();
    stale
        .set_text(&entry, "event opening\n  新正文\n".into())
        .unwrap();
    let changed = stale.export_files().unwrap();
    assert!(stale.apply_save_reader_profile(&plan).is_err());
    assert_eq!(stale.export_files().unwrap(), changed);
}

#[test]
fn fresh_rehearsal_rechecks_existing_profile_and_keeps_read_only_bytes() {
    for change in ["schema", "feature"] {
        let fixture = Fixture::new();
        let mut project = fixture.project();
        let initial = project
            .preview_save_reader_profile(&profile(&project))
            .unwrap();
        project.apply_save_reader_profile(&initial).unwrap();
        project.save().unwrap();
        let mut updated = initial.profile;
        updated.title = "更新标题".into();
        let (plan, candidate) = project.prepare_save_reader_profile(&updated).unwrap();
        let baseline = project.content_baseline();
        let destination = fixture.0.join(&plan.document_path);
        let mut stored: Value = serde_json::from_slice(&fs::read(&destination).unwrap()).unwrap();
        if change == "schema" {
            stored["schema_version"] = json!(99);
        } else {
            stored["required_features"] = json!([READER_PROFILES_FEATURE, "future.v1"]);
        }
        let external = serde_json::to_vec(&stored).unwrap();
        fs::write(&destination, &external).unwrap();
        assert!(project
            .finish_save_reader_profile(&plan, plan.clone(), candidate)
            .is_err());
        assert_eq!(project.content_baseline(), baseline);
        assert!(!project.is_dirty());
        let mut reopened = fixture.project();
        assert!(reopened
            .authoring_document(&destination)
            .unwrap()
            .is_read_only());
        let before = reopened.export_files().unwrap();
        assert!(reopened.apply_save_reader_profile(&plan).is_err());
        assert_eq!(reopened.export_files().unwrap(), before);
        assert_eq!(fs::read(&destination).unwrap(), external);
    }
}

#[test]
fn fresh_rehearsal_rejects_same_bytes_hard_link_injected_before_finish() {
    let fixture = Fixture::new();
    let mut project = fixture.project();
    let initial = project
        .preview_save_reader_profile(&profile(&project))
        .unwrap();
    project.apply_save_reader_profile(&initial).unwrap();
    project.save().unwrap();
    let destination = fixture.0.join(&initial.document_path);
    let alias = fixture.0.join(".world/same-bytes.json");
    fs::copy(&destination, &alias).unwrap();
    let manifest = fixture.0.join(".world/project.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["graph_views"] = json!({"same_bytes":".world/same-bytes.json"});
    fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut project = fixture.project();
    let (plan, candidate) = project
        .prepare_save_reader_profile(&initial.profile)
        .unwrap();
    let baseline = project.content_baseline();
    let before = project.export_files().unwrap();
    fs::remove_file(&destination).unwrap();
    fs::hard_link(&alias, &destination).unwrap();
    assert!(candidate.checkpoint_disk_baselines_match().is_ok());
    assert!(project
        .finish_save_reader_profile(&plan, plan.clone(), candidate)
        .is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.export_files().unwrap(), before);
    assert!(!project.is_dirty());
}

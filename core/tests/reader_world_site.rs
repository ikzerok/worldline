use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::{project::Project, reader_export::*, TargetRef};

#[path = "reader_world_site/maps.rs"]
mod maps;
#[path = "reader_world_site/performance.rs"]
mod performance;
#[path = "reader_world_site/profiles.rs"]
mod profiles;

const SOURCE: &str = r#"let score = 1234567
let secret = "CANARY_PRIVATE_INITIAL"
character mei as "梅"
entity harbor kind place as "公开港口"
  description "潮汐与 Harbor public history"
  property climate = "温暖 Warm"
  property private_note = "CANARY_PRIVATE_FIELD"
  property hidden_ref = ref("entity", "vault")
entity lighthouse kind place as "灯塔"
entity vault kind document as "CANARY_PRIVATE_DISPLAY"
alias entity harbor as "Harbour 灯港"
alias entity vault as "CANARY_PRIVATE_ALIAS"
period era as "时代"
period dawn as "黎明" within era
relation_type maintains as "维护"
  direction directed
relation_def caretaking type maintains from entity harbor to entity lighthouse
  description "港口维护灯塔"
  source_note "CANARY_PRIVATE_SOURCE_NOTE"
  scope period dawn
relation_def confidential type maintains from entity harbor to entity vault
  source_note "CANARY_HIDDEN_ENDPOINT"
event opening as "启程" during dawn with mei
  从 [[entity:harbor|港口]] 启程，Begin the journey.
  if secret == "CANARY_PRIVATE_CONDITION"
    这是一段公开的静态分支正文。
  choice "前往灯塔" if score > 0
    set score = score + 1
    -> arrival
event arrival as "抵达" during dawn follows opening after seen("opening")
  旅程结束。
  -> END
event unpublished as "CANARY_PRIVATE_EVENT"
  CANARY_PRIVATE_BODY
  -> END
asset picture image "art.png" as "公开插画"
"#;

pub(crate) struct Fixture {
    pub root: PathBuf,
}
impl Fixture {
    fn new(name: &str, source: &str, language: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "reader-site-v3-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".world")).unwrap();
        fs::write(root.join("world.wl"), source).unwrap();
        fs::write(root.join("art.png"), [137, 80, 78, 71, 13, 10]).unwrap();
        fs::write(root.join("unreferenced.txt"), b"CANARY_UNREFERENCED_BACKUP").unwrap();
        fs::write(
            root.join(".world/project.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema_version":1,"language_version":language,
                "required_features":["content.object_refs.v1","content.relations.v1"],
                "future_key":{"keep":"preserve"}
            }))
            .unwrap(),
        )
        .unwrap();
        Self { root }
    }
    fn project(&self) -> Project {
        Project::open(&self.root).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn selection() -> ReaderExportSelection {
    ReaderExportSelection {
        schema_version: 3,
        required_features: vec![
            READER_SITE_FEATURE.into(),
            READER_FIELDS_FEATURE.into(),
            READER_STORY_FEATURE.into(),
        ],
        site_title: "潮汐世界 Atlas".into(),
        objects: [
            ("entity", "harbor"),
            ("entity", "lighthouse"),
            ("character", "mei"),
            ("period", "era"),
            ("period", "dawn"),
            ("relation", "caretaking"),
            ("relation", "confidential"),
            ("event", "opening"),
            ("event", "arrival"),
            ("variable", "score"),
        ]
        .into_iter()
        .map(|(kind, id)| TargetRef::new(kind, id))
        .collect(),
        fields: vec![ReaderFieldSelection {
            target: TargetRef::new("entity", "harbor"),
            keys: vec!["climate".into()],
        }],
        maps: Vec::new(),
        manuscripts: Vec::new(),
        attachments: vec!["picture".into()],
    }
}
fn package(
    project: &Project,
    choice: &ReaderExportSelection,
) -> (ReaderExportPreview, BTreeMap<PathBuf, Vec<u8>>) {
    let preview = project.preview_reader_export(choice).unwrap();
    let files = project
        .build_reader_export(choice, &preview.plan_digest)
        .unwrap();
    (preview, files)
}
fn route(preview: &ReaderExportPreview, kind: &str, id: &str) -> String {
    preview
        .included
        .iter()
        .find(|entry| entry.target.as_ref() == Some(&TargetRef::new(kind, id)))
        .unwrap()
        .output_path
        .clone()
}
fn all_text(files: &BTreeMap<PathBuf, Vec<u8>>) -> String {
    files
        .iter()
        .map(|(path, bytes)| format!("{} {}", path.display(), String::from_utf8_lossy(bytes)))
        .collect()
}
fn hash(bytes: &[u8]) -> String {
    let mut value = 0xcbf29ce484222325u64;
    for byte in bytes {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100000001b3);
    }
    format!("{value:016x}")
}

#[test]
fn typed_pages_search_manifest_and_canary_isolation_are_consistent() {
    let fixture = Fixture::new("typed", SOURCE, "1.10");
    let project = fixture.project();
    let before = project.export_files().unwrap();
    let baseline = project.content_baseline();
    let (preview, files) = package(&project, &selection());
    let text = all_text(&files);
    assert!(!text.contains("CANARY"), "私有内容进入公开包");
    assert!(!text.contains(fixture.root.to_str().unwrap()));
    for value in [
        "Harbour 灯港",
        "温暖 Warm",
        "维护",
        "父时段",
        "显式先后关系",
        "score",
        "未公开条件",
        "END",
        "seen(启程)",
    ] {
        assert!(text.contains(value), "缺少公开投影 {value}");
    }
    assert!(!text.contains("1234567"));
    let character =
        String::from_utf8_lossy(&files[Path::new(&route(&preview, "character", "mei"))]);
    assert!(
        character.contains("启程"),
        "公开人物和事件成员关系必须可反向阅读"
    );
    for path in [
        "index.html",
        "objects/index.html",
        "objects/kind-entity.html",
        "maps/index.html",
        "manuscripts/index.html",
        "timeline.html",
        "relations.html",
        "stories.html",
        "search.html",
    ] {
        assert!(files.contains_key(Path::new(path)), "缺页 {path}");
    }
    let search: serde_json::Value =
        serde_json::from_slice(&files[Path::new("search-index.json")]).unwrap();
    for page in &preview.content {
        let entries: Vec<_> = search
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["url"].as_str() == Some(&page.output_path))
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["text"].as_str(), Some(page.text.as_str()));
        assert!(entries[0]["kind"].is_string());
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&files[Path::new("reader-manifest.json")]).unwrap();
    let resources = manifest["resources"].as_array().unwrap();
    assert_eq!(resources.len(), files.len() - 1);
    for resource in resources {
        let bytes = &files[Path::new(resource["path"].as_str().unwrap())];
        assert_eq!(resource["bytes"].as_u64(), Some(bytes.len() as u64));
        assert_eq!(resource["hash"].as_str(), Some(hash(bytes).as_str()));
    }
    let js = String::from_utf8_lossy(&files[Path::new("reader.js")]);
    assert!(js.contains("textContent") && !js.contains("fetch("));
    assert!(js.contains("selectedKind") && js.contains("aliases"));
    assert_eq!(project.export_files().unwrap(), before);
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.is_dirty());
}

#[test]
fn stable_routes_survive_reordering_additions_and_display_changes() {
    let fixture = Fixture::new("routes", SOURCE, "1.10");
    let mut project = fixture.project();
    let mut choice = selection();
    let original = project.preview_reader_export(&choice).unwrap();
    choice.objects.reverse();
    let reordered = project.preview_reader_export(&choice).unwrap();
    assert_eq!(
        route(&original, "entity", "harbor"),
        route(&reordered, "entity", "harbor")
    );
    let path = fixture.root.join("world.wl");
    let source = project
        .document(&path)
        .unwrap()
        .replace("公开港口", "新港口")
        .replace("Harbour 灯港", "港湾 Harbor Bay");
    project
        .set_text(
            &path,
            format!("{source}\nentity extra kind place as \"新增\"\n"),
        )
        .unwrap();
    choice.objects.push(TargetRef::new("entity", "extra"));
    let changed = project.preview_reader_export(&choice).unwrap();
    assert_eq!(
        route(&original, "entity", "harbor"),
        route(&changed, "entity", "harbor")
    );
    assert!(project
        .build_reader_export(&choice, &original.plan_digest)
        .is_err());
    let current = project.preview_reader_export(&choice).unwrap();
    let baseline = project.content_baseline();
    fs::write(fixture.root.join("art.png"), [9, 8, 7]).unwrap();
    assert_eq!(project.content_baseline(), baseline);
    assert!(project
        .build_reader_export(&choice, &current.plan_digest)
        .is_err());
    let mut asset_choice = choice.clone();
    asset_choice.objects.clear();
    asset_choice.fields.clear();
    let mut profile = project
        .create_reader_profile("asset_only", &asset_choice)
        .unwrap();
    let asset_preview = project.preview_reader_profile(&profile).unwrap();
    profile
        .routes
        .iter_mut()
        .find(|entry| entry.target.as_ref() == Some(&TargetRef::new("asset", "picture")))
        .unwrap()
        .output_path = "assets/a0042.png".into();
    assert!(
        project
            .build_reader_profile(&profile, &asset_preview.plan_digest)
            .is_err(),
        "仅附件的站点也必须绑定实际输出路由"
    );
}

#[test]
fn features_are_explicit_and_old_versions_keep_their_privacy() {
    let fixture = Fixture::new("versions", SOURCE, "1.10");
    let project = fixture.project();
    let mut choice = selection();
    choice.required_features.push("unknown.v1".into());
    assert!(project.preview_reader_export(&choice).is_err());
    choice = selection();
    choice.required_features.push(READER_SITE_FEATURE.into());
    assert!(project.preview_reader_export(&choice).is_err());
    choice = selection();
    choice.schema_version = 2;
    choice.required_features = vec![READER_FIELDS_FEATURE.into()];
    let (_, files) = package(&project, &choice);
    assert!(!all_text(&files).contains("Harbour 灯港"));
    assert!(!files.contains_key(Path::new("timeline.html")));
    choice.fields.clear();
    choice.schema_version = 1;
    choice.required_features.clear();
    assert!(project
        .preview_reader_export(&choice)
        .unwrap()
        .content
        .is_empty());
}

#[test]
fn v3_accepts_2000_objects_and_rejects_over_budget_before_compile() {
    let source = (0..2000)
        .map(|id| format!("entity place_{id} kind place as \"地点{id}\"\n"))
        .collect::<String>();
    let fixture = Fixture::new("budget", &source, "1.10");
    let project = fixture.project();
    let mut choice = selection();
    choice.objects = (0..2000)
        .map(|id| TargetRef::new("entity", &format!("place_{id}")))
        .collect();
    choice.fields.clear();
    choice.attachments.clear();
    let (_, files) = package(&project, &choice);
    assert!(files.len() > 2000);
    choice.objects.push(TargetRef::new("entity", "missing"));
    assert!(project
        .preview_reader_export(&choice)
        .unwrap_err()
        .contains("数量限制"));
    choice.objects.pop();
    choice.schema_version = 2;
    choice.required_features = vec![READER_FIELDS_FEATURE.into()];
    assert!(project.preview_reader_export(&choice).is_err());
}

#[test]
fn unselected_strong_reference_and_speaker_never_create_backlinks() {
    let source = r#"entity alpha kind place as "Alpha public"
  property private_ref = ref("entity", "beta")
entity beta kind place as "Beta public"
character speaker as "Speaker public"
let score = 1
event opening as "Opening public"
  say speaker "公开台词" direction "CANARY_PRIVATE_DIRECTION"
  set score = 2
  -> END
"#;
    let fixture = Fixture::new("backlink", source, "1.11");
    let project = fixture.project();
    let mut choice = selection();
    choice.objects = [
        ("entity", "alpha"),
        ("entity", "beta"),
        ("character", "speaker"),
        ("variable", "score"),
        ("event", "opening"),
    ]
    .into_iter()
    .map(|(kind, id)| TargetRef::new(kind, id))
    .collect();
    choice.fields.clear();
    choice.attachments.clear();
    choice
        .required_features
        .retain(|feature| feature != READER_STORY_FEATURE);
    let (preview, files) = package(&project, &choice);
    let page = |kind, id| {
        String::from_utf8_lossy(&files[Path::new(&route(&preview, kind, id))]).into_owned()
    };
    assert!(!page("entity", "beta").contains("Alpha public"));
    assert!(!page("variable", "score").contains("Opening public"));
    assert!(!page("event", "opening").contains("Speaker public"));
    assert!(page("event", "opening").contains("公开台词"));
    assert!(!all_text(&files).contains("CANARY"));
    choice.fields.push(ReaderFieldSelection {
        target: TargetRef::new("entity", "alpha"),
        keys: vec!["private_ref".into()],
    });
    let (preview, files) = package(&project, &choice);
    assert!(
        String::from_utf8_lossy(&files[Path::new(&route(&preview, "entity", "beta"))])
            .contains("Alpha public")
    );
}

#[test]
fn late_include_public_title_changes_invalidate_old_preview() {
    let fixture = Fixture::new("late-include", SOURCE, "1.10");
    let mut project = fixture.project();
    let extra = fixture.root.join("extra.wl");
    fs::write(&extra, "entity extra_public kind place as \"旧标题\"\n").unwrap();
    let entry = fixture.root.join("world.wl");
    let source = project.document(&entry).unwrap().to_owned();
    project
        .set_text(&entry, format!("include \"extra.wl\"\n{source}"))
        .unwrap();
    assert!(!project.documents.contains_key(&extra));
    let mut choice = selection();
    choice.objects = vec![TargetRef::new("entity", "extra_public")];
    choice.fields.clear();
    choice.attachments.clear();
    let preview = project.preview_reader_export(&choice).unwrap();
    let baseline = project.content_baseline();
    assert!(!project.documents.contains_key(&extra));
    fs::write(&extra, "entity extra_public kind place as \"新标题\"\n").unwrap();
    assert_eq!(project.content_baseline(), baseline);
    assert!(project
        .build_reader_export(&choice, &preview.plan_digest)
        .is_err());
}

#[test]
fn nested_reader_target_refs_reject_unknown_fields() {
    let mut value = serde_json::to_value(selection()).unwrap();
    value["objects"][0]["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ReaderExportSelection>(value).is_err());
    let mut value = serde_json::to_value(selection()).unwrap();
    value["fields"][0]["target"]["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ReaderExportSelection>(value).is_err());
    let route = serde_json::json!({"target":{"kind":"entity","id":"harbor","unexpected":true},
        "manuscript_id":null,"chapter_id":null,"output_path":"objects/o0001.html"});
    assert!(serde_json::from_value::<ReaderProfileRoute>(route).is_err());
}

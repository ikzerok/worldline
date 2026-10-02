//! 已登记源码路径使用正式字段与原始 token；未知字段、附着状态和拒绝均整批保留。
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::collaboration::{capture_text_anchor, CommentAnchor};
use worldline_core::presentation_commands::document_hash;
use worldline_core::project::Project;
use worldline_core::source_lifecycle::SourceLifecycleRequest;

struct Fixture {
    root: PathBuf,
    manifest: Value,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "worldline-lifecycle-json-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".world")).unwrap();
        fs::write(
            root.join("world.wl"),
            "include \"old.wl\"\nevent start\n  正文。\n  -> END\n",
        )
        .unwrap();
        fs::write(root.join("old.wl"), "character traveler as \"旅人\"\n").unwrap();
        Self {
            root,
            manifest: json!({
                "schema_version":1,"language_version":"1.10","entry":"world.wl",
                "required_features":["workspace.source_sets.v1","presentation.maps.v1",
                    "presentation.vector_scene.v1","presentation.graph_views.v1",
                    "collaboration.comments.v1","presentation.presets.v1",
                    "catalog.saved_queries.v1","reader.profiles.v1",
                    "presentation.manuscripts.v1","content.localization.v1"],
                "source_config":{"mode":"explicit","active":["world.wl","old.wl"],"archived":[]}
            }),
        }
    }
    fn register(&mut self, category: &str, id: &str, value: Value) -> PathBuf {
        let relative = format!(".world/{id}.json");
        if self.manifest.get(category).is_none() {
            self.manifest[category] = json!({});
        }
        self.manifest[category][id] = json!(relative);
        let path = self.root.join(relative);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        path
    }
    fn open(&self) -> Project {
        fs::write(
            self.root.join(".world/project.json"),
            serde_json::to_vec(&self.manifest).unwrap(),
        )
        .unwrap();
        Project::open(&self.root).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn request() -> SourceLifecycleRequest {
    SourceLifecycleRequest::Move {
        from: "old.wl".into(),
        to: "章节/新 稿.wl".into(),
    }
}
fn read(project: &Project, path: &Path) -> Value {
    serde_json::from_slice(project.authoring_document(path).unwrap().bytes()).unwrap()
}
fn comment(anchor: Value) -> Value {
    json!({"schema_version":1,"id":"note","author":"作者","body":"old.wl", "anchor":anchor})
}

#[test]
fn all_supported_registered_path_slots_move_without_reformatting_extensions() {
    let mut fixture = Fixture::new();
    let old = fixture.root.join("old.wl").to_string_lossy().into_owned();
    let target = json!({"kind":"file","id":old});
    let map = fixture.register("maps", "map", json!({
        "schema_version":1,"required_features":["presentation.vector_scene.v1"],"id":"map","title":"地图",
        "canvas":{"width":100,"height":100,"unit":"normalized"},
        "layer_order":["objects"],"layers":{"objects":{"title":"对象","visible_default":true,"locked":false}},
        "placements":{"marker":{"layer_id":"objects","target_ref":target,"scope_refs":[target],
            "geometry":{"kind":"point","position":[0.2,0.3]},"annotation":"old.wl","role":""}},
        "scene":{"schema_version":1,"view_box":[0,0,100,100],"preserve_aspect_ratio":"xMidYMid meet",
            "root_order":{"objects":["node"]},"nodes":{"node":{"id":"node","layer_id":"objects",
                "geometry":{"kind":"point","position":[10,20]},"target_ref":target,"scope_refs":[target]}}}
    }));
    let raw = fs::read_to_string(&map).unwrap();
    let preserved = format!(r#", "extension" : {{"saved":"\u006fld.wl","target_ref":{target}}} "#);
    fs::write(&map, format!("{}{preserved}}}", &raw[..raw.len() - 1])).unwrap();
    let graph = fixture.register(
        "graph_views",
        "graph",
        json!({
            "schema_version":1,"id":"graph","title":"网络","focus":target,"filters":{"depth":1},
            "positions":{format!("file:{old}"):[0,0]},"hidden_relation_ids":[]
        }),
    );
    let object_comment = fixture.register(
        "comments",
        "note",
        comment(json!({"kind":"object","target":target})),
    );
    let quote = "character traveler as \"旅人\"";
    let text_comment = fixture.register(
        "comments",
        "text_note",
        json!({
            "schema_version":1,"id":"text_note","author":"作者","body":"old.wl",
            "anchor":{"kind":"text_range","path":"old.wl","start_line":1,"end_line":1,
                "quote":quote,"baseline_hash":document_hash(quote.as_bytes())}
        }),
    );
    let preset = fixture.register(
        "presets",
        "preset",
        json!({
            "schema_version":1,"id":"preset","title":"展示","map_id":"map","scope_refs":[target]
        }),
    );
    let query = fixture.register("saved_queries", "query", json!({
        "schema_version":1,"id":"query","name":"范围","query":{"schema_version":1,"filters":[
            {"dimension":"relation","values":[{"related":target}]},
            {"dimension":"author_scope","source_files":["old.wl"]}]},"extension":{"source_files":["old.wl"]}
    }));
    let reader = fixture.register("reader_profiles", "reader", json!({
        "schema_version":1,"required_features":["reader.profiles.v1"],"id":"reader","title":"读者",
        "selection":{"schema_version":2,"required_features":["reader.fields.v1"],"site_title":"读者",
            "objects":[target],"fields":[{"target":target,"keys":["note"]}],"manuscripts":[],"attachments":[]},"routes":[]
    }));
    let manuscript = fixture.register("manuscripts", "book", json!({
        "schema_version":1,"id":"book","title":"书稿","entries":[{"id":"chapter","kind":"chapter","title":"章",
            "target_ref":{"kind":"event","id":"start"},"pov":{"kind":"character","id":"traveler"},"perspective":target}]
    }));
    let locale = fixture.register("localizations", "fr", json!({
        "schema_version":1,"required_features":["content.localization.v1"],"source_locale":"en","target_locale":"fr",
        "entries":{},"extension":{"path":"old.wl"}
    }));
    let manuscript_before = fs::read(&manuscript).unwrap();
    let locale_before = fs::read(&locale).unwrap();
    let mut project = fixture.open();
    let plan = project.preview_source_lifecycle(&request()).unwrap();
    assert_eq!(
        plan.changes
            .iter()
            .filter(|change| change.kind == "authoring")
            .map(|change| change.occurrences.len())
            .sum::<usize>(),
        14
    );
    project.apply_source_lifecycle_plan(&plan).unwrap();
    let new = fixture
        .root
        .join("章节/新 稿.wl")
        .to_string_lossy()
        .into_owned();
    let mapped = json!({"kind":"file","id":new});
    let map_value = read(&project, &map);
    assert_eq!(map_value["placements"]["marker"]["target_ref"], mapped);
    assert_eq!(map_value["placements"]["marker"]["scope_refs"][0], mapped);
    assert_eq!(map_value["scene"]["nodes"]["node"]["target_ref"], mapped);
    assert_eq!(map_value["scene"]["nodes"]["node"]["scope_refs"][0], mapped);
    assert!(
        std::str::from_utf8(project.authoring_document(&map).unwrap().bytes())
            .unwrap()
            .contains(&preserved)
    );
    assert_eq!(read(&project, &graph)["focus"], mapped);
    assert!(read(&project, &graph)["positions"]
        .get(format!("file:{new}"))
        .is_some());
    assert_eq!(read(&project, &object_comment)["anchor"]["target"], mapped);
    assert_eq!(
        read(&project, &text_comment)["anchor"]["path"],
        "章节/新 稿.wl"
    );
    assert_eq!(read(&project, &text_comment)["anchor"]["quote"], quote);
    assert_eq!(read(&project, &preset)["scope_refs"][0], mapped);
    assert_eq!(
        read(&project, &query)["query"]["filters"][0]["values"][0]["related"],
        mapped
    );
    assert_eq!(
        read(&project, &query)["query"]["filters"][1]["source_files"][0],
        "章节/新 稿.wl"
    );
    assert_eq!(
        read(&project, &query)["extension"]["source_files"][0],
        "old.wl"
    );
    assert_eq!(read(&project, &reader)["selection"]["objects"][0], mapped);
    assert_eq!(
        read(&project, &reader)["selection"]["fields"][0]["target"],
        mapped
    );
    assert_eq!(
        project.authoring_document(&manuscript).unwrap().bytes(),
        manuscript_before
    );
    assert_eq!(
        project.authoring_document(&locale).unwrap().bytes(),
        locale_before
    );
    project.save().unwrap();
    let reopened = Project::open(&fixture.root).unwrap();
    assert_eq!(read(&reopened, &graph)["focus"], mapped);
    assert_eq!(
        reopened.authoring_document(&map).unwrap().bytes(),
        project.authoring_document(&map).unwrap().bytes()
    );
}

#[test]
fn rebasing_an_attached_comment_line_refuses_without_recapturing_quote() {
    let mut fixture = Fixture::new();
    let project = fixture.open();
    let anchor = capture_text_anchor(&project, &project.entry, 1, 1).unwrap();
    assert!(matches!(anchor, CommentAnchor::TextRange { .. }));
    fixture.register(
        "comments",
        "note",
        comment(serde_json::to_value(anchor).unwrap()),
    );
    let project = fixture.open();
    let baseline = project.content_baseline();
    let error = project.preview_source_lifecycle(&request()).unwrap_err();
    assert!(error.contains("批注") && error.contains("附着"), "{error}");
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn registered_proposals_are_an_explicit_fail_closed_migration_boundary() {
    let mut fixture = Fixture::new();
    fixture.manifest["required_features"]
        .as_array_mut()
        .unwrap()
        .push(json!("collaboration.proposals.v1"));
    fixture.register("proposals", "proposal", json!({"schema_version":1}));
    let project = fixture.open();
    let baseline = project.content_baseline();
    let error = project.preview_source_lifecycle(&request()).unwrap_err();
    assert!(
        error.contains("proposals") && error.contains("基线"),
        "{error}"
    );
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn malformed_unknown_required_and_unknown_schema_documents_refuse_even_without_old_path() {
    for invalid in [
        "{broken".to_string(),
        json!({"schema_version":9,"id":"note","author":"作者","body":"正文","anchor":{"kind":"object","target":{"kind":"event","id":"start"}}}).to_string(),
        json!({"schema_version":1,"required_features":["future.path_semantics.v1"],"id":"note","author":"作者","body":"正文","anchor":{"kind":"object","target":{"kind":"event","id":"start"}}}).to_string(),
        json!({"schema_version":1,"id":"note","author":"作者","body":"正文","anchor":{"kind":"unknown"}}).to_string(),
    ] {
        let mut fixture = Fixture::new();
        let path = fixture.register("comments", "note", comment(json!({"kind":"object","target":{"kind":"event","id":"start"}})));
        fs::write(path, invalid).unwrap();
        let project = fixture.open();
        let baseline = project.content_baseline();
        assert!(project.preview_source_lifecycle(&request()).is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
}

#[test]
fn unsupported_manuscript_file_targets_and_reader_file_routes_refuse() {
    for reader in [false, true] {
        let mut fixture = Fixture::new();
        let target = json!({"kind":"file","id":fixture.root.join("old.wl")});
        if reader {
            fixture.register("reader_profiles", "reader", json!({
                "schema_version":1,"required_features":["reader.profiles.v1"],"id":"reader","title":"读者",
                "selection":{"schema_version":1,"site_title":"读者","objects":[],"manuscripts":[],"attachments":[]},
                "routes":[{"target":target,"output_path":"objects/o0001.html"}]
            }));
        } else {
            fixture.register("manuscripts", "book", json!({
                "schema_version":1,"id":"book","title":"书稿","entries":[{"id":"chapter","kind":"chapter","title":"章","target_ref":target}]
            }));
        }
        let project = fixture.open();
        let baseline = project.content_baseline();
        assert!(project.preview_source_lifecycle(&request()).is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
}

#[test]
fn unknown_nested_reader_selection_and_query_versions_refuse() {
    for reader in [false, true] {
        let mut fixture = Fixture::new();
        if reader {
            fixture.register("reader_profiles", "reader", json!({
                "schema_version":1,"required_features":["reader.profiles.v1"],"id":"reader","title":"读者",
                "selection":{"schema_version":99,"site_title":"读者","objects":[],"manuscripts":[],"attachments":[]},"routes":[]
            }));
        } else {
            fixture.register("saved_queries", "query", json!({
                "schema_version":1,"id":"query","name":"范围","query":{"schema_version":99,"filters":[]}
            }));
        }
        let project = fixture.open();
        let baseline = project.content_baseline();
        assert!(project.preview_source_lifecycle(&request()).is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
}

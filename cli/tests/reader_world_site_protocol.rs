//! 完整世界站 v3 的协议薄封装回归；公开投影和计划只取自 core。
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::project::Project;
use worldline_core::reader_export::{ReaderExportPreview, ReaderExportSelection};

const SOURCE: &str = r#"world orchard as "灯园世界"
  description "公开世界简介。"
entity beacon kind place as "石灯台"
  description "PUBLIC_WORLDSITE_PROSE 海风吹过。English beacon."
  property public_note = "PUBLIC_WORLDSITE_FIELD"
  property private_note = "PRIVATE_CANARY_FIELD"
entity vault kind place as "PRIVATE_CANARY_TITLE"
  description "PRIVATE_CANARY_BODY"
alias entity beacon as "Stone Lantern"
alias entity beacon as "归航之灯"
alias entity vault as "PRIVATE_CANARY_ALIAS"
period age as "航海纪"
period spring as "春汛" within age
relation_type passage as "通航"
  direction directed
  from entity
  to entity
relation_def channel type passage from entity beacon to entity vault
  description "公开航道说明。"
  source_note "PRIVATE_CANARY_SOURCE_NOTE"
event docking as "靠岸" during spring
  PUBLIC_WORLDSITE_STORY
  [[entity:beacon|前往石灯台]]
  [[entity:vault|PRIVATE_CANARY_LINK_LABEL]]
  -> sailing
event sailing as "再启航" during spring follows docking
  风起，可以出发。
  -> END
event hidden as "PRIVATE_CANARY_EVENT_TITLE"
  PRIVATE_CANARY_STORY
  -> END
asset emblem image "assets/emblem.png" as "公开灯徽"
asset secret file "private/draft.txt" as "PRIVATE_CANARY_ASSET_NAME"
attach event docking with emblem, secret
"#;

// 完整 1x1 PNG，确保逐文件比较也覆盖二进制附件。
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0,
    0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 100, 248, 15, 0, 1, 5, 1, 1,
    39, 24, 227, 102, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn fixture(name: &str) -> (PathBuf, PathBuf) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let home = std::env::temp_dir().join(format!(
        "cli-reader-v3-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&home).unwrap();
    let root = home.join("workspace");
    for directory in [".world", "assets", "private", ".agent"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    fs::write(root.join("world.wl"), SOURCE).unwrap();
    fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.relations.v1"]}"#).unwrap();
    fs::write(root.join("assets/emblem.png"), PNG).unwrap();
    fs::write(root.join("private/draft.txt"), "PRIVATE_CANARY_ATTACHMENT").unwrap();
    fs::write(root.join(".agent/notes.md"), "PRIVATE_CANARY_UNREFERENCED").unwrap();
    (home, root)
}

fn selection() -> ReaderExportSelection {
    serde_json::from_value(json!({
        "schema_version":3,
        "required_features":["reader.world_site.v1","reader.fields.v1"],
        "site_title":"灯园公开站",
        "objects":[
            {"kind":"world","id":"orchard"},
            {"kind":"entity","id":"beacon"},
            {"kind":"period","id":"age"},
            {"kind":"period","id":"spring"},
            {"kind":"relation","id":"channel"},
            {"kind":"event","id":"docking"},
            {"kind":"event","id":"sailing"}
        ],
        "fields":[{"target":{"kind":"entity","id":"beacon"},"keys":["public_note"]}],
        "manuscripts":[],"maps":[],"attachments":["emblem"]
    }))
    .unwrap()
}

fn disk_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn assert_public_site(
    files: &BTreeMap<PathBuf, Vec<u8>>,
    plan: &ReaderExportPreview,
    choice: &ReaderExportSelection,
    root: &Path,
) {
    for path in [
        "index.html",
        "timeline.html",
        "relations.html",
        "stories.html",
        "objects/kind-entity.html",
        "search.html",
        "search-data.js",
        "style.css",
    ] {
        assert!(files.contains_key(Path::new(path)), "missing {path}");
    }
    let all = files
        .iter()
        .map(|(path, bytes)| format!("{}\n{}", path.display(), String::from_utf8_lossy(bytes)))
        .collect::<Vec<_>>()
        .join("\n");
    for public in [
        "PUBLIC_WORLDSITE_PROSE",
        "PUBLIC_WORLDSITE_FIELD",
        "PUBLIC_WORLDSITE_STORY",
        "Stone Lantern",
        "归航之灯",
        "未公开内容",
        "静态分支阅读",
    ] {
        assert!(all.contains(public), "missing {public}");
    }
    for private in [
        "PRIVATE_CANARY",
        "private/draft.txt",
        ".agent/notes.md",
        "world.wl",
    ] {
        assert!(!all.contains(private), "leaked {private}");
    }
    assert!(!all.contains(root.to_str().unwrap()));
    assert!(files.values().any(|bytes| bytes == PNG));
    let index: Value = serde_json::from_slice(&files[Path::new("search-index.json")]).unwrap();
    let entries = index.as_array().unwrap();
    for target in &choice.objects {
        let included = plan
            .included
            .iter()
            .find(|item| item.target.as_ref() == Some(target))
            .unwrap();
        assert!(files.contains_key(Path::new(&included.output_path)));
        let entry = entries
            .iter()
            .find(|entry| entry["url"].as_str() == Some(included.output_path.as_str()))
            .unwrap();
        assert_eq!(entry["kind"].as_str(), Some(target.kind.as_str()));
    }
    let beacon = entries
        .iter()
        .find(|entry| entry["title"] == "石灯台")
        .unwrap();
    assert_eq!(beacon["kind"], "entity");
    for alias in ["Stone Lantern", "归航之灯"] {
        assert!(beacon["aliases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == alias));
    }
    let searchable = format!(
        "{} {} {}",
        beacon["title"], beacon["aliases"], beacon["text"]
    )
    .to_lowercase();
    for query in [
        "stone lantern",
        "归航之灯",
        "海风",
        "english beacon",
        "public_worldsite_field",
    ] {
        assert!(searchable.contains(query), "not searchable: {query}");
    }
    for page in &plan.content {
        let entry = entries
            .iter()
            .find(|entry| entry["url"].as_str() == Some(page.output_path.as_str()))
            .unwrap();
        assert_eq!(entry["text"].as_str(), Some(page.text.as_str()));
    }
    let js = String::from_utf8_lossy(&files[Path::new("reader.js")]);
    assert!(js.contains("toLocaleLowerCase"));
    assert!(js.contains("textContent"));
    assert!(!js.contains("fetch("));
    let manifest: Value =
        serde_json::from_slice(&files[Path::new("reader-manifest.json")]).unwrap();
    assert_eq!(
        manifest["resources"].as_array().unwrap().len(),
        files.len() - 1
    );
}

fn command(
    root: &Path,
    operation: &str,
    choice: &ReaderExportSelection,
    output: Option<(&str, &Path)>,
) -> (i32, Value) {
    let mut args = vec![
        "reader-export".into(),
        operation.into(),
        root.to_string_lossy().into_owned(),
        "--selection-json".into(),
        serde_json::to_string(choice).unwrap(),
        "--json".into(),
    ];
    if let Some((digest, destination)) = output {
        args.extend([
            "--plan-digest".into(),
            digest.into(),
            "--out".into(),
            destination.to_string_lossy().into_owned(),
        ]);
    }
    let mut bytes = Vec::new();
    let code = wl::run(&args, &mut bytes, &mut Cursor::new(Vec::<u8>::new())).unwrap();
    (code, serde_json::from_slice(&bytes).unwrap())
}

#[test]
fn v3_cli_plan_and_published_bytes_equal_core_without_private_content() {
    let (home, root) = fixture("publish");
    let destination = home.join("公开站点");
    let before = disk_files(&root);
    let choice = selection();
    let project = Project::open(&root).unwrap();
    let plan = project.preview_reader_export(&choice).unwrap();
    let expected = project
        .build_reader_export(&choice, &plan.plan_digest)
        .unwrap();
    let (code, preview) = command(&root, "preview", &choice, None);
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["ok"], true);
    assert_eq!(preview["operation"], "preview");
    assert_eq!(preview["plan"], json!(plan));
    assert_eq!(preview["baseline"], plan.content_baseline);
    assert!(!destination.exists());
    assert_eq!(disk_files(&root), before);
    let (code, applied) = command(
        &root,
        "apply",
        &choice,
        Some((&plan.plan_digest, &destination)),
    );
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["ok"], true);
    assert_eq!(applied["plan"], json!(plan));
    assert_eq!(applied["output"], json!(destination));
    let published = disk_files(&destination);
    assert_eq!(published, expected);
    assert_public_site(&published, &plan, &choice, &root);
    assert_eq!(disk_files(&root), before);
    let (code, replay) = command(
        &root,
        "apply",
        &choice,
        Some((&plan.plan_digest, &destination)),
    );
    assert_eq!(code, 1, "{replay}");
    assert_eq!(replay["ok"], false);
    assert_eq!(replay["error"]["code"], "EXPORT_FAILED");
    assert_eq!(disk_files(&destination), published);
    let (code, after) = command(&root, "preview", &choice, None);
    assert_eq!(code, 0);
    assert_eq!(after["plan"], preview["plan"]);
    assert_eq!(disk_files(&root), before);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn v3_cli_stale_digest_and_existing_target_fail_without_writes() {
    let (home, root) = fixture("rejections");
    let choice = selection();
    let (code, original) = command(&root, "preview", &choice, None);
    assert_eq!(code, 0, "{original}");
    let old_digest = original["plan"]["plan_digest"].as_str().unwrap();
    fs::write(
        root.join("world.wl"),
        SOURCE.replace("PUBLIC_WORLDSITE_PROSE", "PUBLIC_UPDATED_PROSE"),
    )
    .unwrap();
    let existing = home.join("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("index.html"), "KEEP_EXISTING_SITE").unwrap();
    fs::write(existing.join("extra.bin"), [0, 255, 17]).unwrap();
    let destination = home.join("stale-new-target");
    let before = disk_files(&home);
    for output in [&destination, &existing] {
        let (code, stale) = command(&root, "apply", &choice, Some((old_digest, output)));
        assert_eq!(code, 1, "{stale}");
        assert_eq!(stale["ok"], false);
        assert_eq!(stale["error"]["code"], "STALE_PLAN");
        assert_eq!(disk_files(&home), before);
        assert!(!destination.exists());
    }
    let project = Project::open(&root).unwrap();
    let current = project.preview_reader_export(&choice).unwrap();
    assert_ne!(current.plan_digest, old_digest);
    let (code, preview) = command(&root, "preview", &choice, None);
    assert_eq!(code, 0);
    assert_eq!(preview["plan"], json!(current));
    let (code, rejected) = command(
        &root,
        "apply",
        &choice,
        Some((&current.plan_digest, &existing)),
    );
    assert_eq!(code, 1, "{rejected}");
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["error"]["code"], "EXPORT_FAILED");
    assert_eq!(disk_files(&home), before);
    assert!(!destination.exists());
    fs::remove_dir_all(home).unwrap();
}

use std::{fs, path::PathBuf};
use worldline_core::{project::Project, reader_export::ReaderExportSelection};
fn fixture(name: &str) -> (PathBuf, Project) {
    let root = std::env::temp_dir().join(format!("reader-fields-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join(".world/project.json"),r#"{"schema_version":1,"language_version":"1.10","required_features":["content.object_refs.v1"]}"#).unwrap();
    fs::write(
        root.join("world.wl"),
        r#"character mei as "梅"
  property appearance = "蓝外套\n<script>bad()</script>"
  property empty = ""
  property zero = 0
  property no = false
  property secret = "PRIVATE_SECRET_SENTINEL"
entity secret_id_sentinel kind place as "PRIVATE_NAME_SENTINEL"
entity harbor kind place as "公开港口"
  property destination = ref("entity", "secret_id_sentinel")
event start
  公开正文
  -> END
"#,
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    (root, project)
}
fn request() -> ReaderExportSelection {
    serde_json::from_value(serde_json::json!({"schema_version":2,"required_features":["reader.fields.v1"],"site_title":"公开","objects":[{"kind":"character","id":"mei"},{"kind":"entity","id":"harbor"}],"manuscripts":[],"attachments":[],"fields":[{"target":{"kind":"character","id":"mei"},"keys":["appearance","empty","zero","no"]},{"target":{"kind":"entity","id":"harbor"},"keys":["destination"]}]})).unwrap()
}
#[test]
fn explicit_fields_preview_matches_public_bytes_and_hides_secrets() {
    let (root, project) = fixture("privacy");
    let selection = request();
    let preview = project.preview_reader_export(&selection).unwrap();
    let files = project
        .build_reader_export(&selection, &preview.plan_digest)
        .unwrap();
    let all = files
        .iter()
        .map(|(p, b)| format!("{} {}", p.display(), String::from_utf8_lossy(b)))
        .collect::<String>();
    for secret in [
        "PRIVATE_SECRET_SENTINEL",
        "secret_id_sentinel",
        "PRIVATE_NAME_SENTINEL",
        root.to_str().unwrap(),
    ] {
        assert!(!all.contains(secret), "leak {secret}");
    }
    assert!(all.contains("&lt;script&gt;"));
    assert!(all.contains("false"));
    assert!(all.contains("（空字符串）"));
    assert!(all.contains("zero"));
    assert!(preview
        .content
        .iter()
        .any(|p| p.text.contains("蓝外套") && !p.empty_content));
    let search: serde_json::Value =
        serde_json::from_slice(&files[&PathBuf::from("search-index.json")]).unwrap();
    for (entry, page) in search.as_array().unwrap().iter().zip(&preview.content) {
        assert_eq!(entry["text"].as_str(), Some(page.text.as_str()));
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn old_selection_remains_private_and_new_selection_is_strict() {
    let (root, project) = fixture("strict");
    let mut selection = request();
    selection.schema_version = 1;
    assert!(project.preview_reader_export(&selection).is_err());
    selection.fields.clear();
    selection.required_features.clear();
    let preview = project.preview_reader_export(&selection).unwrap();
    assert!(preview.content.is_empty());
    let preview_json = serde_json::to_value(&preview).unwrap();
    assert!(preview_json.get("content").is_none());
    let selection_json = serde_json::to_value(&selection).unwrap();
    assert!(selection_json.get("fields").is_none());
    assert!(selection_json.get("required_features").is_none());
    let mut selection = request();
    selection.required_features.push("unknown".into());
    assert!(project.preview_reader_export(&selection).is_err());
    let mut selection = request();
    selection.fields[0].keys.push("missing".into());
    assert!(project.preview_reader_export(&selection).is_err());
    let mut selection = request();
    selection.fields[0].keys.push("appearance".into());
    assert!(project.preview_reader_export(&selection).is_err());
    let mut selection = request();
    selection.objects.pop();
    assert!(project.preview_reader_export(&selection).is_err());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn field_change_expires_plan_without_mutation() {
    let (root, mut project) = fixture("stale");
    let selection = request();
    let preview = project.preview_reader_export(&selection).unwrap();
    let path = root.join("world.wl");
    let source = project.document(&path).unwrap().replace("蓝外套", "红外套");
    project.set_text(&path, source).unwrap();
    let before = project.content_baseline();
    assert!(project
        .build_reader_export(&selection, &preview.plan_digest)
        .is_err());
    assert_eq!(before, project.content_baseline());
    fs::remove_dir_all(root).unwrap();
}

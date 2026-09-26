use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use worldline_core::catalog::TargetRef;
use worldline_core::project::Project;
use worldline_core::reader_export::{ReaderExportSelection, ReaderManuscriptSelection};

fn root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "worldline-reader-export-{name}-{}",
        std::process::id()
    ))
}

fn project_root(name: &str) -> PathBuf {
    let root = root(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::create_dir_all(root.join("private")).unwrap();
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::create_dir_all(root.join(".world/manuscripts")).unwrap();
    fs::write(
        root.join("world.wl"),
        r#"entity beacon kind place as "Public Beacon"
  description "Public entity description."
  property private_note = "ENTITY_PRIVATE_SENTINEL"
entity secret_archive kind document as "HIDDEN_ENTITY_TITLE_SENTINEL"
  description "HIDDEN_ENTITY_BODY_SENTINEL"
event public_event as "Public Event"
  Published story.
  [[entity:beacon|Public beacon link]]
  [[event:secret_event|HIDDEN_LINK_LABEL_SENTINEL]]
  -> END
event secret_event as "HIDDEN_EVENT_TITLE_SENTINEL"
  HIDDEN_EVENT_BODY_SENTINEL.
  -> END
asset public_art image "assets/public.png" as "Public art"
asset private_art file "private/secret.txt" as "HIDDEN_ATTACHMENT_NAME_SENTINEL"
attach event public_event with public_art, private_art
"#,
    )
    .unwrap();
    fs::write(root.join("assets/public.png"), [0, 1, 255, 2]).unwrap();
    fs::write(
        root.join("private/secret.txt"),
        b"PRIVATE_ATTACHMENT_SENTINEL",
    )
    .unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"language_version":"1.10","required_features":["presentation.manuscripts.v1"],"manuscripts":{"book":".world/manuscripts/book.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/manuscripts/book.json"),
        br#"{"schema_version":1,"id":"book","title":"Public Book","private":"MANUSCRIPT_PRIVATE_SENTINEL","entries":[{"id":"private_section","kind":"section","title":"HIDDEN_SECTION_TITLE_SENTINEL","goal":"SECTION_PRIVATE_SENTINEL"},{"id":"published_chapter","kind":"chapter","parent_id":"private_section","title":"Public Chapter","target_ref":{"kind":"event","id":"public_event"},"summary":"Public chapter summary.","status":"published","goal":"CHAPTER_PRIVATE_SENTINEL"},{"id":"draft_chapter","kind":"chapter","title":"HIDDEN_DRAFT_TITLE_SENTINEL","target_ref":{"kind":"event","id":"secret_event"},"summary":"HIDDEN_DRAFT_SUMMARY_SENTINEL","status":"draft","goal":"DRAFT_GOAL_SENTINEL"}]}"#,
    )
    .unwrap();
    fs::write(root.join(".agent/private.md"), "AGENT_PRIVATE_SENTINEL").unwrap();
    root
}

fn selection() -> ReaderExportSelection {
    ReaderExportSelection {
        schema_version: 1,
        site_title: "Public Site".into(),
        objects: vec![
            TargetRef::new("event", "public_event"),
            TargetRef::new("entity", "beacon"),
        ],
        manuscripts: vec![ReaderManuscriptSelection {
            id: "book".into(),
            chapters: vec!["published_chapter".into()],
        }],
        attachments: vec!["public_art".into()],
    }
}

fn output_bytes(files: &BTreeMap<PathBuf, Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (path, content) in files {
        bytes.extend_from_slice(path.to_string_lossy().as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(content);
        bytes.push(0);
    }
    bytes
}

fn assert_local_html_links_resolve(files: &BTreeMap<PathBuf, Vec<u8>>) {
    for (page, bytes) in files {
        if page.extension().is_none_or(|extension| extension != "html") {
            continue;
        }
        let html = String::from_utf8(bytes.clone()).unwrap();
        for attribute in ["href=\"", "src=\""] {
            let mut rest = html.as_str();
            while let Some(start) = rest.find(attribute) {
                rest = &rest[start + attribute.len()..];
                let end = rest.find('\"').unwrap();
                let url = &rest[..end];
                assert!(!url.contains("://"), "unexpected remote URL {url}");
                let candidate = page.parent().unwrap_or(Path::new("")).join(url);
                let mut normalized = PathBuf::new();
                for component in candidate.components() {
                    match component {
                        std::path::Component::Normal(value) => normalized.push(value),
                        std::path::Component::ParentDir => {
                            assert!(
                                normalized.pop(),
                                "URL escapes package root: {page:?} -> {url}"
                            );
                        }
                        std::path::Component::CurDir => {}
                        _ => panic!("invalid package URL {url}"),
                    }
                }
                assert!(
                    files.contains_key(&normalized),
                    "missing local link {page:?} -> {url}"
                );
                rest = &rest[end + 1..];
            }
        }
    }
}

#[test]
fn explicit_selection_builds_an_offline_reader_without_private_sentinels() {
    let root = project_root("allowlist");
    let project = Project::open(&root.join("world.wl")).unwrap();
    let before_baseline = project.content_baseline();
    let before_dirty = project.is_dirty();
    let plan = project.preview_reader_export(&selection()).unwrap();
    assert_eq!(project.content_baseline(), before_baseline);
    assert_eq!(project.is_dirty(), before_dirty);
    assert!(plan.exclusions.iter().any(|item| {
        item.target
            .as_ref()
            .is_some_and(|target| target.id == "secret_event")
            && item.reason_code == "target_not_selected"
    }));
    assert!(plan.exclusions.iter().any(|item| {
        item.chapter_id.as_deref() == Some("draft_chapter")
            && item.reason_code == "chapter_not_selected"
    }));

    let files = project
        .build_reader_export(&selection(), &plan.plan_digest)
        .unwrap();
    assert!(files.contains_key(Path::new("index.html")));
    assert!(files.contains_key(Path::new("search.html")));
    assert!(files.contains_key(Path::new("search-index.json")));
    assert!(files.keys().any(|path| path.starts_with("objects")));
    assert!(files.keys().any(|path| path.starts_with("manuscripts")));
    assert!(files
        .keys()
        .any(|path| path.extension().is_some_and(|extension| extension == "png")));

    let output = output_bytes(&files);
    for secret in [
        "ENTITY_PRIVATE_SENTINEL",
        "HIDDEN_ENTITY_TITLE_SENTINEL",
        "HIDDEN_ENTITY_BODY_SENTINEL",
        "HIDDEN_EVENT_TITLE_SENTINEL",
        "secret_event",
        "secret_archive",
        "HIDDEN_EVENT_BODY_SENTINEL",
        "HIDDEN_LINK_LABEL_SENTINEL",
        "private_art",
        "HIDDEN_ATTACHMENT_NAME_SENTINEL",
        "PRIVATE_ATTACHMENT_SENTINEL",
        "MANUSCRIPT_PRIVATE_SENTINEL",
        "HIDDEN_SECTION_TITLE_SENTINEL",
        "private_section",
        "SECTION_PRIVATE_SENTINEL",
        "CHAPTER_PRIVATE_SENTINEL",
        "HIDDEN_DRAFT_TITLE_SENTINEL",
        "draft_chapter",
        "HIDDEN_DRAFT_SUMMARY_SENTINEL",
        "DRAFT_GOAL_SENTINEL",
        "AGENT_PRIVATE_SENTINEL",
        ".world/manuscripts/book.json",
        "private/secret.txt",
        ".agent/private.md",
    ] {
        assert!(
            !output
                .windows(secret.len())
                .any(|window| window == secret.as_bytes()),
            "reader package leaked {secret}"
        );
    }
    let all_text = files
        .iter()
        .filter_map(|(path, bytes)| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .filter(|extension| matches!(*extension, "html" | "json" | "js" | "css"))
                .map(|_| String::from_utf8_lossy(bytes).into_owned())
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(all_text.contains("Published story."));
    assert!(all_text.contains("Public chapter summary."));
    assert!(all_text.contains("Public beacon link"));
    assert!(all_text.contains("未公开内容"));
    assert!(!all_text.contains("exclusions"));
    assert!(!all_text.contains("https://"));
    assert!(!all_text.contains("http://"));
    assert!(all_text.contains("textContent"));
    assert!(files[Path::new("assets/a0001.png")] == [0, 1, 255, 2]);
    assert_local_html_links_resolve(&files);

    for path in files.keys() {
        assert!(path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_))));
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn stale_plan_and_existing_destination_never_write_or_overwrite() {
    let root = project_root("atomic");
    let mut project = Project::open(&root.join("world.wl")).unwrap();
    let plan = project.preview_reader_export(&selection()).unwrap();
    project
        .set_text(&root.join("world.wl"), "entity changed kind place\n".into())
        .unwrap();
    let stale_target = root.parent().unwrap().join("reader-stale-target");
    let _ = fs::remove_dir_all(&stale_target);
    assert!(project
        .export_reader_site(&selection(), &plan.plan_digest, &stale_target)
        .is_err());
    assert!(!stale_target.exists());

    let asset_root = project_root("stale-asset");
    let project = Project::open(&asset_root.join("world.wl")).unwrap();
    let plan = project.preview_reader_export(&selection()).unwrap();
    fs::write(asset_root.join("assets/public.png"), [9, 8, 7, 6]).unwrap();
    let stale_asset_target = asset_root
        .parent()
        .unwrap()
        .join("reader-stale-asset-target");
    let _ = fs::remove_dir_all(&stale_asset_target);
    assert!(project
        .export_reader_site(&selection(), &plan.plan_digest, &stale_asset_target)
        .is_err());
    assert!(!stale_asset_target.exists());

    let existing = root.parent().unwrap().join("reader-existing-target");
    let _ = fs::remove_dir_all(&existing);
    fs::create_dir_all(&existing).unwrap();
    fs::write(existing.join("sentinel.txt"), "KEEP_EXISTING_SENTINEL").unwrap();
    assert!(project
        .export_reader_site(&selection(), "wrong-plan", &existing)
        .is_err());
    assert_eq!(
        fs::read_to_string(existing.join("sentinel.txt")).unwrap(),
        "KEEP_EXISTING_SENTINEL"
    );
    let _ = fs::remove_dir_all(existing);
    let _ = fs::remove_dir_all(asset_root);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn active_attachment_formats_and_unescaped_site_markup_are_rejected_or_escaped() {
    let root = project_root("active-format");
    let project = Project::open(&root.join("world.wl")).unwrap();
    let mut request = selection();
    request.attachments = vec!["private_art".into()];
    assert!(project.preview_reader_export(&request).is_err());

    request.attachments.clear();
    request.site_title = "<script>PUBLIC_TITLE_SENTINEL</script>".into();
    let plan = project.preview_reader_export(&request).unwrap();
    let files = project
        .build_reader_export(&request, &plan.plan_digest)
        .unwrap();
    let output = output_bytes(&files);
    assert!(!String::from_utf8_lossy(&output).contains("<script>PUBLIC_TITLE_SENTINEL</script>"));
    assert!(String::from_utf8_lossy(&files[Path::new("index.html")])
        .contains("&lt;script&gt;PUBLIC_TITLE_SENTINEL"));
    assert_local_html_links_resolve(&files);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn pure_content_projects_export_and_arbitrary_file_targets_are_rejected() {
    let root = root("pure-content");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("world.wl"),
        "event setting as \"Pure Content\"\n  A simple public story.\n  -> END\n",
    )
    .unwrap();
    let project = Project::open(&root.join("world.wl")).unwrap();
    let request = ReaderExportSelection {
        schema_version: 1,
        site_title: "Pure Site".into(),
        objects: vec![TargetRef::new("event", "setting")],
        manuscripts: Vec::new(),
        attachments: Vec::new(),
    };
    let plan = project.preview_reader_export(&request).unwrap();
    assert!(project
        .build_reader_export(&request, &plan.plan_digest)
        .unwrap()
        .contains_key(Path::new("index.html")));

    let mut invalid = request;
    invalid.objects = vec![TargetRef::new("file", "C:/private/source.wl")];
    assert!(project.preview_reader_export(&invalid).is_err());
    let _ = fs::remove_dir_all(root);
}

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
        maps: Vec::new(),
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
fn malformed_selected_manuscript_stays_out_of_the_reader_but_remains_in_backup() {
    let root = project_root("malformed-manuscript");
    let manuscript_path = root.join(".world/manuscripts/book.json");
    let malformed = [b'{', 0xff, b'}'];
    fs::write(&manuscript_path, malformed).unwrap();
    let project = Project::open(&root.join("world.wl")).unwrap();
    assert!(project.preview_reader_export(&selection()).is_err());

    let target = root
        .parent()
        .unwrap()
        .join(format!("reader-malformed-output-{}", std::process::id()));
    let _ = fs::remove_dir_all(&target);
    assert!(project
        .export_reader_site(&selection(), "unreviewed", &target)
        .is_err());
    assert!(!target.exists());

    let backup = project.export_files().unwrap();
    assert_eq!(backup[Path::new(".world/manuscripts/book.json")], malformed);
    assert_eq!(
        backup[Path::new(".agent/private.md")],
        b"AGENT_PRIVATE_SENTINEL"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reader_export_rejects_a_manuscript_selection_over_the_chapter_budget() {
    let root = project_root("chapter-budget");
    let project = Project::open(&root.join("world.wl")).unwrap();
    let mut request = selection();
    request.attachments = vec!["private_art".into()];
    request.manuscripts[0].chapters = (0..=5_000)
        .map(|index| format!("chapter_{index}"))
        .collect();

    let error = project.preview_reader_export(&request).unwrap_err();
    assert!(error.contains("5000 限制"), "unexpected error: {error}");
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
        maps: Vec::new(),
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

#[test]
fn maps_export_only_explicit_geometry_and_safe_links() {
    use worldline_core::reader_export::ReaderMapSelection;
    let root = project_root("maps-explicit");
    let manifest_path = root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["maps"] = serde_json::json!({"atlas":".world/atlas.json"});
    manifest["required_features"]
        .as_array_mut()
        .unwrap()
        .extend([
            serde_json::json!("presentation.maps.v1"),
            serde_json::json!("presentation.geometry.line_area.v1"),
        ]);
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let map = serde_json::json!({
        "schema_version":1, "id":"atlas", "title":"Atlas <safe>",
        "canvas":{"width":1000,"height":500,"unit":"normalized"},
        "raster_layers":[{"id":"base","asset":{"kind":"asset","id":"public_art"},"rect":[0.25,0.25,0.75,0.75]}],
        "layer_order":["top","places"],
        "layers":{"places":{"title":"PRIVATE_LAYER_TITLE","visible_default":false,"locked":false},"top":{"title":"TOP_PRIVATE_LAYER","visible_default":true,"locked":false}},
        "placements":{
            "public":{"role":"note","layer_id":"top","geometry":{"kind":"polygon","points":[[0.1,0.1],[0.8,0.1],[0.4,0.8]]},"annotation":"<script>alert(1)</script>","label_override":"Public <label>","target_ref":{"kind":"entity","id":"beacon"},"style":{"stroke":"red\" onload=\"evil","fill":"#112233","fill_opacity":0.4,"stroke_opacity":0.7,"stroke_width":3}},
            "hidden_target":{"role":"note","layer_id":"places","geometry":{"kind":"point","position":[0.2,0.3]},"annotation":"public note","style":{"stroke_width":0,"fill_opacity":0,"stroke_opacity":0.25},"target_ref":{"kind":"entity","id":"secret_archive"}},
            "private":{"target_ref":null,"role":"note","layer_id":"places","geometry":{"kind":"point","position":[0.6,0.3]},"annotation":"UNSELECTED_MAP_SECRET"}
        }
    });
    let map_path = root.join(".world/atlas.json");
    fs::write(&map_path, serde_json::to_vec(&map).unwrap()).unwrap();
    let mut project = Project::open(&root.join("world.wl")).unwrap();
    let mut request = selection();
    request.maps.push(ReaderMapSelection {
        id: "atlas".into(),
        placements: vec!["public".into(), "hidden_target".into()],
        raster_layers: vec!["base".into()],
    });
    let plan = project
        .preview_reader_export(&request)
        .unwrap_or_else(|error| panic!("{error}: {:?}", project.map_index().diagnostics));
    let files = project
        .build_reader_export(&request, &plan.plan_digest)
        .unwrap();
    let html = String::from_utf8(files[Path::new("maps/m0001.html")].clone()).unwrap();
    assert!(html.contains("<polygon"));
    assert!(html.contains("fill-opacity=\"0.4\""));
    assert!(html.contains("stroke-opacity=\"0.7\""));
    let point = html
        .split("<circle")
        .nth(1)
        .unwrap()
        .split("/>")
        .next()
        .unwrap();
    assert!(point.contains("stroke-width=\"0\""));
    assert!(point.contains("fill-opacity=\"0\""));
    assert!(point.contains("stroke-opacity=\"0.25\""));
    assert!(html.find("<polygon").unwrap() < html.find("<circle").unwrap());
    let mut reordered = request.clone();
    reordered.maps[0].placements.reverse();
    let reordered_plan = project.preview_reader_export(&reordered).unwrap();
    let reordered_files = project
        .build_reader_export(&reordered, &reordered_plan.plan_digest)
        .unwrap();
    assert_eq!(
        files[Path::new("maps/m0001.html")],
        reordered_files[Path::new("maps/m0001.html")]
    );
    assert!(html.contains("../assets/a0001.png"));
    assert!(html.contains("x=\"250\" y=\"125\" width=\"500\" height=\"250\""));
    assert!(html.contains("../objects/"));
    assert!(html.contains("未公开内容"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("onload="));
    let all = String::from_utf8_lossy(&output_bytes(&files)).into_owned();
    for secret in [
        "UNSELECTED_MAP_SECRET",
        "PRIVATE_LAYER_TITLE",
        "secret_archive",
        ".world/atlas.json",
    ] {
        assert!(!all.contains(secret), "leaked {secret}");
    }
    assert_local_html_links_resolve(&files);
    let mut missing_asset = request.clone();
    missing_asset.attachments.clear();
    assert!(project.preview_reader_export(&missing_asset).is_err());
    let mut duplicate = request.clone();
    duplicate.maps[0].placements.push("public".into());
    assert!(project.preview_reader_export(&duplicate).is_err());
    let mut changed = map;
    changed["title"] = "Changed".into();
    fs::write(&map_path, serde_json::to_vec(&changed).unwrap()).unwrap();
    project = Project::open(&root.join("world.wl")).unwrap();
    assert!(project
        .build_reader_export(&request, &plan.plan_digest)
        .is_err());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn text_labels_export_escaped_multiline_and_never_unselected_text() {
    use worldline_core::reader_export::ReaderMapSelection;
    let root = project_root("text-label-export");
    let manifest_path = root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["maps"] = serde_json::json!({"atlas":".world/atlas.json"});
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let marker = |text: &str| serde_json::json!({"target_ref":null,"role":"文字标签","annotation":"公开文字", "layer_id":"labels", "geometry":{"kind":"text","position":[0.2,0.3],"text":text,"font_size":24,"color":"#123456"}});
    let map = serde_json::json!({"schema_version":1,"id":"atlas","title":"文字地图", "required_features":["presentation.geometry.text.v1"],"canvas":{"width":1000,"height":600,"unit":"normalized"},"layer_order":["labels"],"layers":{"labels":{"title":"labels","visible_default":true,"locked":false}},"placements":{"public":marker("  雾港  灯塔\n\n<script>&标记 "),"secret":marker("PRIVATE_LABEL_SENTINEL")}});
    fs::write(
        root.join(".world/atlas.json"),
        serde_json::to_vec(&map).unwrap(),
    )
    .unwrap();
    let project = Project::open(&root.join("world.wl")).unwrap();
    let mut request = selection();
    request.maps.push(ReaderMapSelection {
        id: "atlas".into(),
        placements: vec!["public".into()],
        raster_layers: vec![],
    });
    let plan = project.preview_reader_export(&request).unwrap();
    let files = project
        .build_reader_export(&request, &plan.plan_digest)
        .unwrap();
    let html = String::from_utf8(files[Path::new("maps/m0001.html")].clone()).unwrap();
    assert!(html.contains("<text xml:space=\"preserve\" font-family=\"sans-serif\""));
    assert!(html.contains("<tspan x=\"200\" y=\"204\">  雾港  灯塔</tspan>"));
    assert!(html.contains("<tspan x=\"200\" y=\"232.8\"></tspan>"));
    assert!(html.contains("&lt;script&gt;&amp;标记 </tspan>"));
    assert!(!html.contains("<script>&标记"));
    assert!(!String::from_utf8_lossy(&output_bytes(&files)).contains("PRIVATE_LABEL_SENTINEL"));
    let _ = fs::remove_dir_all(root);
}

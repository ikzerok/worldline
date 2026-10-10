use super::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const SOURCE: &str = r#"let n = 1
character a as "同名"
character b as "同名"
fragment leaf()
  say a "Shared" direction "PRIVATE_LEAF" #wl-localization:leaf_line
  return
fragment left()
  call leaf()
  return
fragment right()
  call leaf()
  return
event start with b
  同名: ordinary narrative
  say a "Main {n} [[character:a|A]]" direction "PRIVATE_MAIN" #wl-localization:main_line
  say b "OTHER_ROLE_SECRET"
  if n > 0
    call left()
    call right()
  call left()
  -> END
event other with a
  say b "UNSELECTED_SECRET"
  -> END
"#;
fn path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "production-script-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
fn fixture(source: &str) -> Project {
    let files=BTreeMap::from([
        (PathBuf::from("world.wl"),source.as_bytes().to_vec()),
        (PathBuf::from(".world/project.json"),serde_json::to_vec(&json!({"schema_version":1,"language_version":"1.12","required_features":["presentation.manuscripts.v1","content.localization.v1","content.choice_presentation.v1"],"manuscripts":{"book":".world/book.json"},"localizations":{"en":".world/en.json"}})).unwrap()),
        (PathBuf::from(".world/book.json"),serde_json::to_vec(&json!({"schema_version":1,"id":"book","title":"Book","entries":[
            {"id":"one","kind":"chapter","title":"First","pov":{"kind":"character","id":"b"},"target_ref":{"kind":"event","id":"start"}},
            {"id":"two","kind":"chapter","title":"Second","target_ref":{"kind":"event","id":"start"}}]})).unwrap()),
        (PathBuf::from(".world/en.json"),serde_json::to_vec(&json!({"schema_version":1,"required_features":["content.localization.v1"],"source_locale":"zh","target_locale":"en","entries":{}})).unwrap()),
    ]);
    on_disk(&path(), &files)
}
fn request() -> ProductionScriptRequest {
    let mut request = ProductionScriptRequest::new(ProductionScope::CurrentTarget {
        target: TargetRef::new("event", "start"),
    });
    request.speaker = Some(TargetRef::new("character", "a"));
    request
}
fn snapshot(project: &Project, request: &ProductionScriptRequest) -> ProductionScriptSnapshot {
    project
        .production_script_snapshot(&[], &[], request)
        .unwrap()
}
fn options(format: ProductionFormat) -> ProductionExportOptions {
    ProductionExportOptions {
        schema_version: 1,
        format,
        include_direction: false,
    }
}
fn translate(project: &mut Project) -> Value {
    let snapshot = snapshot(project, &request());
    let mut entries = serde_json::Map::new();
    for row in snapshot.rows {
        entries.insert(
            row.stable_line_id.unwrap(),
            json!({"source_revision":row.source_revision,"translation_parts":row.source_parts}),
        );
    }
    let value = json!({"schema_version":1,"required_features":["content.localization.v1"],"source_locale":"zh","target_locale":"en","entries":entries});
    write_locale(project, &value);
    value
}
fn write_locale(project: &mut Project, value: &Value) {
    project
        .set_authoring_document(
            &project.root.join(".world/en.json"),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
}
#[test]
fn production_closure_deduplicates_definitions_calls_and_chapter_occurrences() {
    let project = fixture(SOURCE);
    let mut input = request();
    input.scope = ProductionScope::Manuscript {
        query: Box::new(crate::manuscript::ManuscriptQueryRequest {
            manuscript_id: "book".into(),
            offset: 1,
            limit: 1,
            collapsed: vec!["one".into()],
            ..Default::default()
        }),
        chapter_ids: None,
        expected_query_key: None,
    };
    let result = snapshot(&project, &input);
    assert_eq!(result.summary().selected_chapter_occurrences, 2);
    assert_eq!(result.summary().root_targets, 1);
    assert_eq!(result.summary().definition_count, 4);
    assert_eq!(result.summary().added_fragment_definitions, 3);
    assert_eq!(result.summary().call_sites, 5);
    assert_eq!(result.summary().matching_rows, 2);
    let leaf = result
        .rows
        .iter()
        .find(|row| row.stable_line_id.as_deref() == Some("leaf_line"))
        .unwrap();
    assert_eq!(leaf.external_call_uses.len(), 2);
    assert!(leaf.control_ancestry.is_empty());
    assert!(result
        .call_sites()
        .iter()
        .any(|call| !call.control_ancestry.is_empty()));
    assert!(result
        .rows
        .iter()
        .all(|row| row.speaker.as_ref().unwrap().target.id == "a"));
    input.include_fragments = false;
    let direct = snapshot(&project, &input);
    assert_eq!(direct.summary().matching_rows, 1);
    assert_eq!(direct.summary().call_sites, 3);
    assert!(!direct.summary().includes_fragment_closure);
    input.scope = ProductionScope::Project;
    let all = snapshot(&project, &input);
    assert_eq!(all.summary().matching_rows, 2);
    assert!(all.summary().includes_fragment_closure);
}
#[test]
fn production_private_output_uses_sanitized_fields_in_all_formats() {
    let project = fixture(SOURCE);
    let result = snapshot(&project, &request());
    let main = result
        .rows
        .iter()
        .find(|row| row.stable_line_id.as_deref() == Some("main_line"))
        .unwrap();
    assert_eq!(result.author_direction(&main.row_key), Some("PRIVATE_MAIN"));
    let page = serde_json::to_string(&result.page(0, 100).unwrap()).unwrap();
    assert!(!page.contains("direction"));
    for format in [
        ProductionFormat::Json,
        ProductionFormat::Markdown,
        ProductionFormat::Csv,
    ] {
        let artifact = result.export(&options(format)).unwrap();
        let rendered = String::from_utf8(artifact.bytes().to_vec()).unwrap();
        for forbidden in [
            "PRIVATE_MAIN",
            "PRIVATE_LEAF",
            "OTHER_ROLE_SECRET",
            "UNSELECTED_SECRET",
            project.root.to_str().unwrap(),
            "say a",
            "excerpt",
        ] {
            assert!(
                !rendered.contains(forbidden),
                "{format:?} leaked {forbidden}"
            );
            assert!(
                !rendered
                    .replace('\\', "")
                    .contains(&forbidden.replace('\\', "")),
                "{format:?} leaked an escaped {forbidden}"
            );
        }
        let mut explicit = options(format);
        explicit.include_direction = true;
        let artifact = result.export(&explicit).unwrap();
        let included = String::from_utf8_lossy(artifact.bytes());
        match format {
            ProductionFormat::Markdown => {
                assert!(included.contains("- 演出备注（源语言）：PRIVATE\\_MAIN\n"))
            }
            ProductionFormat::Csv => assert!(included.contains("\"'PRIVATE_MAIN\"")),
            ProductionFormat::Json => {
                let document: Value = serde_json::from_slice(artifact.bytes()).unwrap();
                let row = document["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["stable_line_id"] == "main_line")
                    .unwrap();
                assert_eq!(row["direction"], "PRIVATE_MAIN");
            }
        }
    }
    let exact = result.export(&options(ProductionFormat::Json)).unwrap();
    let value: Value = serde_json::from_slice(exact.bytes()).unwrap();
    assert!(value["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row.get("direction").is_none()));
    assert_eq!(value["metadata_language"], "source");
}
#[test]
fn production_selected_locale_strict_ignores_other_role_missing_translation() {
    let mut project = fixture(SOURCE);
    let mut locale = translate(&mut project);
    let mut input = request();
    input.target_locale = Some("en".into());
    let translated = snapshot(&project, &input);
    assert!(translated
        .rows
        .iter()
        .all(|row| row.status == ProductionStatus::Translated));
    translated.export(&options(ProductionFormat::Json)).unwrap();
    locale["entries"]
        .as_object_mut()
        .unwrap()
        .remove("leaf_line");
    write_locale(&mut project, &locale);
    let missing = snapshot(&project, &input);
    assert_eq!(
        missing
            .rows
            .iter()
            .filter(|row| row.status == ProductionStatus::Missing)
            .count(),
        1
    );
    assert_eq!(
        missing
            .export(&options(ProductionFormat::Csv))
            .unwrap_err()
            .code,
        "LOCALE_INCOMPLETE"
    );
    input.locale_policy = ProductionLocalePolicy::SourceFallback;
    let fallback = snapshot(&project, &input);
    assert_eq!(fallback.summary().source_fallback_rows, 1);
    let row = fallback
        .rows
        .iter()
        .find(|row| row.used_source_fallback)
        .unwrap();
    assert_eq!(row.source_parts, row.selected_parts);
    fallback.export(&options(ProductionFormat::Json)).unwrap();
    input.target_locale = Some("unknown".into());
    assert_eq!(
        project
            .production_script_snapshot(&[], &[], &input)
            .unwrap_err()
            .code,
        "UNKNOWN_LOCALE"
    );
}
#[test]
fn production_stale_invalid_locale_and_global_integrity_remain_distinct() {
    let mut project = fixture(SOURCE);
    let mut locale = translate(&mut project);
    locale["entries"]["main_line"]["source_revision"] = json!("old");
    write_locale(&mut project, &locale);
    let mut input = request();
    input.target_locale = Some("en".into());
    assert!(snapshot(&project, &input)
        .rows
        .iter()
        .any(|row| row.status == ProductionStatus::Stale));
    locale["entries"]["main_line"]["translation_parts"] =
        json!([{"type":"text","text":"lost tokens"}]);
    write_locale(&mut project, &locale);
    assert!(snapshot(&project, &input)
        .rows
        .iter()
        .any(|row| row.status == ProductionStatus::Invalid));
    locale["schema_version"] = json!(999);
    let invalid_bytes = serde_json::to_vec(&locale).unwrap();
    assert!(project
        .set_authoring_document(&project.root.join(".world/en.json"), invalid_bytes.clone())
        .is_err());
    // 外部未知版本文档可以被原样读取，但不能通过受保护 setter 制造。
    std::fs::write(project.root.join(".world/en.json"), &invalid_bytes).unwrap();
    let reopened = Project::open_read_only(&project.root).unwrap();
    assert_eq!(
        reopened.authoring_documents[&project.root.join(".world/en.json")].bytes(),
        invalid_bytes
    );
    assert!(reopened
        .production_script_snapshot(&[], &[], &input)
        .is_err());
    let project = fixture(&SOURCE.replace(
        "OTHER_ROLE_SECRET\"",
        "OTHER_ROLE_SECRET\" #wl-localization:main_line",
    ));
    assert!(project
        .production_script_snapshot(&[], &[], &request())
        .is_err());
}
#[test]
fn production_draft_and_metadata_freshness_do_not_use_translation_revision_alone() {
    let mut project = fixture(SOURCE);
    let first = snapshot(&project, &request());
    let row = first
        .rows
        .iter()
        .find(|row| row.stable_line_id.as_deref() == Some("main_line"))
        .unwrap();
    project
        .production_script_source_hit(&[], &[], &first, &row.row_key)
        .unwrap();
    let revised = SOURCE
        .replace("PRIVATE_MAIN", "CHANGED_PRIVATE")
        .replace("character a as \"同名\"", "character a as \"改名\"");
    project
        .set_text(&project.root.join("world.wl"), revised)
        .unwrap();
    assert_eq!(
        project
            .validate_production_script(&[], &[], &first)
            .unwrap_err()
            .code,
        "STALE_SNAPSHOT"
    );
    let next = snapshot(&project, &request());
    let next_row = next
        .rows
        .iter()
        .find(|row| row.stable_line_id.as_deref() == Some("main_line"))
        .unwrap();
    assert_ne!(first.key(), next.key());
    assert_eq!(row.source_revision, next_row.source_revision);
    let mut buffer = project
        .open_source_writing_buffer(Path::new("world.wl"))
        .unwrap();
    buffer.replace_source(buffer.source().replace("Main", "Draft"));
    let before = project.content_baseline();
    let draft = project
        .production_script_snapshot(&[buffer.clone()], &[], &request())
        .unwrap();
    assert_ne!(draft.key(), next.key());
    assert_eq!(before, project.content_baseline());
    assert_eq!(
        project
            .production_script_snapshot(&[buffer.clone(), buffer.clone()], &[], &request())
            .unwrap_err()
            .code,
        "DUPLICATE_DRAFT"
    );
    buffer.replace_source("event broken\n  if\n".into());
    assert!(project
        .production_script_snapshot(&[buffer], &[], &request())
        .is_err());
}
#[test]
fn production_filtering_precedes_paging_and_budgets_never_truncate() {
    let mut source = "character a\ncharacter b\nevent start\n".to_string();
    for index in 0..125 {
        source.push_str(&format!("  say a \"line{index}\"\n"));
    }
    source.push_str("  -> END\n");
    let project = fixture(&source);
    let mut input = request();
    let result = snapshot(&project, &input);
    assert_eq!(result.page(0, 100).unwrap().total, 125);
    assert_eq!(result.page(100, 100).unwrap().rows.len(), 25);
    assert!(result.page(125, 100).unwrap().rows.is_empty());
    assert!(result.page(0, 101).is_err());
    input.search = "line12".into();
    let filtered = snapshot(&project, &input);
    assert_eq!(filtered.summary().matching_rows, 6);
    input.search = "no such words".into();
    assert_eq!(snapshot(&project, &input).summary().matching_rows, 0);
    input.search.clear();
    input.limits.rows = 124;
    assert_eq!(
        project
            .production_script_snapshot(&[], &[], &input)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
}
#[test]
fn production_strict_machine_input_and_expected_keys() {
    for raw in [
        r#"{"schema_version":1,"scope":{"kind":"current_target","target":{"kind":"event","id":"start","extra":true}}}"#,
        r#"{"schema_version":1,"scope":{"kind":"project"},"speaker":{"kind":"character","id":"a","extra":true}}"#,
        r#"{"schema_version":1,"schema_version":1,"scope":{"kind":"project"}}"#,
        r#"{"schema_version":1,"scope":{"kind":"project"},"typo":true}"#,
    ] {
        assert!(
            parse_production_script_request(raw).is_err(),
            "accepted {raw}"
        );
    }
    assert!(parse_production_export_options(
        r#"{"schema_version":1,"format":"csv","include_direction":false,"raw":true}"#
    )
    .is_err());
    let project = fixture(SOURCE);
    let mut input = request();
    let first = snapshot(&project, &input);
    input.expected_snapshot_key = Some(first.key().into());
    assert_eq!(snapshot(&project, &input).key(), first.key());
    input.expected_snapshot_key = Some("old".into());
    assert_eq!(
        project
            .production_script_snapshot(&[], &[], &input)
            .unwrap_err()
            .code,
        "STALE_SNAPSHOT"
    );
}
#[test]
fn production_csv_all_cells_are_prefixed_quoted_and_markdown_escapes() {
    let project = fixture(&SOURCE.replace(
        "Shared",
        r#"=1+1\n<script>[click](https://bad.test)</script>,\"quoted\""#,
    ));
    let result = snapshot(&project, &request());
    let artifact = result.export(&options(ProductionFormat::Csv)).unwrap();
    let csv = String::from_utf8(artifact.bytes().to_vec()).unwrap();
    assert!(csv.starts_with("\"'record_type\",\"'metadata\""));
    assert!(csv.ends_with("\r\n"));
    assert!(csv.contains("\"'=1+1\n"));
    assert!(csv.contains("\"\"quoted\"\""));
    let artifact = result.export(&options(ProductionFormat::Markdown)).unwrap();
    let markdown = String::from_utf8(artifact.bytes().to_vec()).unwrap();
    assert!(!markdown.contains("<script>"));
    assert!(markdown.contains("&lt;script&gt;"));
    assert!(!markdown.contains("[click](https://bad.test)"));
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn production_native_new_file_transaction_cancellation_and_race_preserve_targets() {
    let project = fixture(SOURCE);
    let result = snapshot(&project, &request());
    let artifact = result.export(&options(ProductionFormat::Json)).unwrap();
    let directory = path();
    std::fs::create_dir_all(&directory).unwrap();
    let destination = directory.join("script.json");
    write_production_script_new(&project.root, &destination, &artifact, &mut || Ok(())).unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), artifact.bytes());
    assert!(
        write_production_script_new(&project.root, &destination, &artifact, &mut || Ok(()))
            .is_err()
    );
    let cancelled = directory.join("cancelled.json");
    let mut count = 0;
    assert!(
        write_production_script_new(&project.root, &cancelled, &artifact, &mut || {
            count += 1;
            if count == 2 {
                Err("cancelled".into())
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    assert!(!cancelled.exists());
    let race = directory.join("race.json");
    let mut count = 0;
    assert!(
        write_production_script_new(&project.root, &race, &artifact, &mut || {
            count += 1;
            if count == 2 {
                std::fs::write(&race, b"winner").unwrap();
            }
            Ok(())
        })
        .is_err()
    );
    assert_eq!(std::fs::read(&race).unwrap(), b"winner");
    assert!(!std::fs::read_dir(&directory).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".worldline-")));
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn production_cross_file_sources_and_nested_scene_overlap_remain_unique() {
    let root = path();
    let files = BTreeMap::from([
        (
            PathBuf::from("world.wl"),
            b"character a\nevent start\ninclude \"body.wl\"\n".to_vec(),
        ),
        (
            PathBuf::from("body.wl"),
            b"  scene inside\n    say a \"Included\" direction \"PRIVATE_INCLUDED\"\n  -> END\n"
                .to_vec(),
        ),
        (
            PathBuf::from(".world/project.json"),
            br#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#.to_vec(),
        ),
    ]);
    let project = on_disk(&root, &files);
    let result = snapshot(&project, &request());
    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    assert_eq!(row.source.file, "body.wl");
    assert_eq!(row.declaration, TargetRef::new("scene", "start.inside"));
    let hit = project
        .production_script_source_hit(&[], &[], &result, &row.row_key)
        .unwrap();
    // 导航路径使用 Project 已规范化的根目录，避免 temp_dir 的短路径别名。
    assert_eq!(hit.path, project.root.join("body.wl"));
    assert!(hit.preview.contains("Included"));
    let mut input = request();
    input.scope = ProductionScope::CurrentTarget {
        target: TargetRef::new("scene", "start.inside"),
    };
    let scene = snapshot(&project, &input);
    assert_eq!(scene.rows.len(), 1);
    assert_eq!(scene.rows[0].source.file, "body.wl");
}
#[test]
fn production_role_only_change_preserves_translation_but_invalidates_snapshot() {
    let mut project = fixture(SOURCE);
    translate(&mut project);
    let mut input = request();
    input.speaker = None;
    input.target_locale = Some("en".into());
    let first = snapshot(&project, &input);
    let source = project
        .document(&project.entry)
        .unwrap()
        .replace("say a \"Main", "say b \"Main");
    project.set_text(&project.entry.clone(), source).unwrap();
    let second = snapshot(&project, &input);
    assert_ne!(first.key(), second.key());
    let old = first
        .rows
        .iter()
        .find(|row| row.stable_line_id.as_deref() == Some("main_line"))
        .unwrap();
    let new = second
        .rows
        .iter()
        .find(|row| row.stable_line_id.as_deref() == Some("main_line"))
        .unwrap();
    assert_eq!(old.source_revision, new.source_revision);
    assert_eq!(new.status, ProductionStatus::Translated);
    assert_eq!(new.speaker.as_ref().unwrap().target.id, "b");
}
#[test]
fn production_zero_selected_rows_preserve_scope_role_locale_header() {
    let project = fixture(SOURCE);
    let mut input = request();
    input.search = "absent".into();
    input.target_locale = Some("en".into());
    let result = snapshot(&project, &input);
    assert_eq!(result.summary().matching_rows, 0);
    let artifact = result.export(&options(ProductionFormat::Json)).unwrap();
    let value: Value = serde_json::from_slice(artifact.bytes()).unwrap();
    assert_eq!(value["scope_kind"], "current_target");
    assert_eq!(value["speaker"]["id"], "a");
    assert_eq!(value["target_locale"], "en");
    assert!(value["rows"].as_array().unwrap().is_empty());
}

fn on_disk(root: &Path, files: &BTreeMap<PathBuf, Vec<u8>>) -> Project {
    for (relative, bytes) in files {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    Project::open(root).unwrap()
}
#[path = "tests/guards.rs"]
mod guards;

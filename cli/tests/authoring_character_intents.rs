#![cfg(not(target_arch = "wasm32"))]

use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{authoring_intents::AuthoringIntent, catalog::TargetRef, project::Project};

const SOURCE: &str = "// 未选中的原稿😀\r\nevent start\r\n  你看见林😀。\r\n  -> END";
struct Workspace(PathBuf);
impl Workspace {
    fn new(manifest: Option<&str>) -> (Self, Project) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-character-intent-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("world.wl"), SOURCE).unwrap();
        fs::write(root.join("people.wl"), "// 作者资料原稿\r\n").unwrap();
        if let Some(manifest) = manifest {
            fs::create_dir_all(root.join(".world")).unwrap();
            fs::write(root.join(".world/project.json"), manifest).unwrap();
        }
        let project = Project::open(&root).unwrap();
        (Self(root), project)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn character_intent(project: &Project) -> Value {
    let offset = SOURCE.rfind("林😀").unwrap();
    json!({
        "expected_baseline": project.content_baseline(),
        "target": {
            "kind":"create_character",
            "value": {"path": project.root.join("people.wl"), "draft": {"id":"lin","display":"林😀"}}
        },
        "selection": {"path":project.entry,"start":offset,"end":offset+"林😀".len(),"expected_text":"林😀"},
        "placement":null
    })
}

fn invoke(project: &Project, operation: &str, intent: &Value) -> (i32, Value) {
    let args = vec![
        "authoring-intent".into(),
        operation.into(),
        project.root.to_string_lossy().into_owned(),
        "--intent-json".into(),
        intent.to_string(),
        "--json".into(),
    ];
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::<u8>::new())).unwrap();
    let payload = serde_json::from_slice(&out).unwrap_or_else(|error| {
        panic!(
            "CLI 必须返回结构化 JSON：{error}: {}",
            String::from_utf8_lossy(&out)
        )
    });
    (code, payload)
}

#[test]
fn new_character_dto_uses_generic_cli_preview_apply_save_and_stale_guards() {
    let (work, project) = Workspace::new(None);
    let intent = character_intent(&project);
    let decoded: AuthoringIntent = serde_json::from_value(intent.clone()).unwrap();
    let expected = project.preview_authoring_intent(&decoded).unwrap();
    let (code, preview) = invoke(&project, "preview", &intent);
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["ok"], true);
    assert_eq!(preview["operation"], "preview");
    assert_eq!(preview["target"], json!({"kind":"character","id":"lin"}));
    assert_eq!(preview["language_version"], "1.9");
    assert_eq!(preview["baseline"], project.content_baseline());
    assert_eq!(preview["new_baseline"], expected.new_baseline);
    assert_eq!(preview["changed_files"], json!(expected.changed_files));
    assert_eq!(
        preview["reference_impact"],
        json!(expected.reference_impact)
    );
    assert_eq!(fs::read_to_string(&project.entry).unwrap(), SOURCE);
    assert_eq!(
        fs::read_to_string(work.0.join("people.wl")).unwrap(),
        "// 作者资料原稿\r\n"
    );
    assert!(!work.0.join(".world").exists());
    let (code, applied) = invoke(&project, "apply", &intent);
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["ok"], true);
    assert_eq!(applied["operation"], "apply");
    assert_eq!(applied["changed_files"].as_array().unwrap().len(), 2);
    let expected_source = SOURCE.replacen("你看见林😀", "你看见[[character:lin|林😀]]", 1);
    assert_eq!(fs::read_to_string(&project.entry).unwrap(), expected_source);
    let people = fs::read_to_string(work.0.join("people.wl")).unwrap();
    assert!(people.starts_with("character lin as \"林😀\"\n"));
    assert!(people.ends_with("// 作者资料原稿\r\n"));
    assert!(!people.contains("entity lin"));
    let reopened = Project::open(&work.0).unwrap();
    assert_eq!(reopened.language_version(), "1.9");
    assert_eq!(applied["baseline"], reopened.content_baseline());
    assert_eq!(applied["new_baseline"], reopened.content_baseline());
    assert!(!reopened.is_dirty());
    assert!(!work.0.join(".world/project.json").exists());
    let compiled = reopened.compile_object_search_snapshot();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert!(compiled
        .analysis
        .catalog
        .object(&TargetRef::new("character", "lin"))
        .is_some());
    assert!(compiled.analysis.catalog.entities.is_empty());
    assert_eq!(compiled.analysis.catalog.text_links.len(), 1);
    assert_eq!(
        compiled.analysis.catalog.text_links[0].target,
        TargetRef::new("character", "lin")
    );
    let (code, repeated) = invoke(&project, "apply", &intent);
    assert_eq!(code, 1, "{repeated}");
    assert_eq!(repeated["error"]["code"], "STALE_BASELINE");
    assert_eq!(fs::read_to_string(&project.entry).unwrap(), expected_source);
    assert_eq!(
        fs::read_to_string(work.0.join("people.wl")).unwrap(),
        people
    );
}

#[test]
fn generic_cli_keeps_entity_language_gate_and_returns_structured_character_errors() {
    let (work, project) = Workspace::new(None);
    let intent = character_intent(&project);
    let mut entity = intent.clone();
    entity["target"]["kind"] = json!("create_entity");
    entity["target"]["value"]["draft"]["entity_type"] = json!("place");
    let (code, rejected) = invoke(&project, "apply", &entity);
    assert_eq!(code, 1, "{rejected}");
    assert_eq!(rejected["error"]["code"], "LANGUAGE_VERSION_REQUIRED");
    assert_eq!(fs::read_to_string(&project.entry).unwrap(), SOURCE);
    assert_eq!(
        fs::read_to_string(work.0.join("people.wl")).unwrap(),
        "// 作者资料原稿\r\n"
    );
    let mut invalid = intent.clone();
    invalid["target"]["value"]["draft"]["display"] = json!("   ");
    let (code, rejected) = invoke(&project, "apply", &invalid);
    assert_eq!(code, 1, "{rejected}");
    assert_eq!(rejected["error"]["code"], "INTENT_REJECTED");
    let mut malformed = intent.clone();
    malformed["target"]["kind"] = json!("create_person");
    let (code, rejected) = invoke(&project, "preview", &malformed);
    assert_eq!(code, 2, "{rejected}");
    assert_eq!(rejected["error"]["code"], "INVALID_INTENT");
    let (code, preview) = invoke(&project, "preview", &intent);
    assert_eq!(code, 0, "{preview}");
    fs::write(&project.entry, "event start\n  外部新稿。\n  -> END\n").unwrap();
    let (code, stale) = invoke(&project, "apply", &intent);
    assert_eq!(code, 1, "{stale}");
    assert_eq!(stale["error"]["code"], "STALE_BASELINE");
    assert_eq!(
        fs::read_to_string(&project.entry).unwrap(),
        "event start\n  外部新稿。\n  -> END\n"
    );
    assert_eq!(
        fs::read_to_string(work.0.join("people.wl")).unwrap(),
        "// 作者资料原稿\r\n"
    );
}

#[test]
fn old_existing_and_entity_dtos_still_work_alongside_formal_character_creation() {
    let manifest = r#"{"schema_version":1,"language_version":"1.10","required_features":[],"extension":{"keep":true}}"#;
    let (work, project) = Workspace::new(Some(manifest));
    let mut entity = character_intent(&project);
    entity["target"]["kind"] = json!("create_entity");
    entity["target"]["value"]["draft"]["entity_type"] = json!("place");
    let (code, preview) = invoke(&project, "preview", &entity);
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["target"], json!({"kind":"entity","id":"lin"}));
    let (code, applied) = invoke(&project, "apply", &entity);
    assert_eq!(code, 0, "{applied}");
    let mut current = Project::open(&work.0).unwrap();
    let source = format!(
        "{}\n// 第二次操作保留既有链接\nevent second\n  另一位林😀。\n  -> END\n",
        current.document(&current.entry).unwrap()
    );
    current
        .set_text(&current.entry.clone(), source.clone())
        .unwrap();
    current.save().unwrap();
    let mut character = character_intent(&current);
    let offset = source.rfind("林😀").unwrap();
    character["selection"]["start"] = json!(offset);
    character["selection"]["end"] = json!(offset + "林😀".len());
    let (code, applied) = invoke(&current, "apply", &character);
    assert_eq!(code, 0, "{applied}");
    let current = Project::open(&work.0).unwrap();
    let compiled = current.compile_object_search_snapshot();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert!(compiled
        .analysis
        .catalog
        .object(&TargetRef::new("entity", "lin"))
        .is_some());
    assert!(compiled
        .analysis
        .catalog
        .object(&TargetRef::new("character", "lin"))
        .is_some());
    assert_eq!(compiled.analysis.catalog.text_links.len(), 2);
    assert_eq!(
        fs::read_to_string(work.0.join(".world/project.json")).unwrap(),
        manifest
    );
    let mut reuse = character_intent(&current);
    let mut source = current.document(&current.entry).unwrap().to_owned();
    source.push_str("\nevent third\n  再看林😀。\n  -> END\n");
    let mut current = current;
    current
        .set_text(&current.entry.clone(), source.clone())
        .unwrap();
    current.save().unwrap();
    reuse["expected_baseline"] = json!(current.content_baseline());
    reuse["target"] = json!({"kind":"existing","value":{"kind":"entity","id":"lin"}});
    let offset = source.rfind("林😀").unwrap();
    reuse["selection"]["start"] = json!(offset);
    reuse["selection"]["end"] = json!(offset + "林😀".len());
    let (code, preview) = invoke(&current, "preview", &reuse);
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["target"], json!({"kind":"entity","id":"lin"}));
    assert_eq!(preview["changed_files"].as_array().unwrap().len(), 1);
    assert_eq!(fs::read_to_string(&current.entry).unwrap(), source);
    let (code, applied) = invoke(&current, "apply", &reuse);
    assert_eq!(code, 0, "{applied}");
    let reopened = Project::open(&work.0).unwrap();
    let compiled = reopened.compile_object_search_snapshot();
    assert_eq!(compiled.analysis.catalog.text_links.len(), 3);
    assert!(reopened
        .document(&reopened.entry)
        .unwrap()
        .contains("再看[[entity:lin|林😀]]。"));
}

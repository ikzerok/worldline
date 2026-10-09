use super::*;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::localization::{LocalizationEdit, LocalizationPart};

static NEXT: AtomicU64 = AtomicU64::new(0);
const SOURCE: &str = "let traveler = \"Ari\"\nevent start\n  Hello {traveler}! #wl-localization:greeting\n  choice \"Continue\" #wl-localization:go\n    -> END\n";

fn fixture(name: &str) -> (PathBuf, Server, String) {
    let root = std::env::temp_dir().join(format!(
        "v033-rpc-locale-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), SOURCE).unwrap();
    fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.localization.v1"]}"#).unwrap();
    let mut server = Server::default();
    let opened = call(&mut server, "project.open", json!({"path":root}));
    assert_eq!(opened["ok"], true, "{opened}");
    let id = opened["project_id"].as_str().unwrap().into();
    (root, server, id)
}

fn call(server: &mut Server, method: &str, params: Value) -> Value {
    server
        .call(method, &params)
        .unwrap_or_else(|error| panic!("{method}: {} {}", error.code, error.message))
}

fn request(server: &Server, id: &str) -> LocalizationEditDraft {
    let project = &server.projects[id].project;
    let page = project
        .query_localization_catalog(&LocalizationCatalogQuery::default())
        .unwrap();
    LocalizationEditDraft {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh-Hans".into(),
        source_baseline: page.source_baseline,
        edits: page
            .entries
            .into_iter()
            .map(|entry| LocalizationEdit {
                id: entry.id.unwrap(),
                source_revision: entry.source_revision.unwrap(),
                translation_parts: entry
                    .source_parts
                    .into_iter()
                    .map(|part| match part {
                        LocalizationPart::Text { text } => LocalizationPart::Text {
                            text: if text == "Continue" {
                                "继续".into()
                            } else {
                                text.replace("Hello ", "你好，")
                            },
                        },
                        other => other,
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn translate(server: &mut Server, id: &str) -> Value {
    let draft = request(server, id);
    let preview = call(
        server,
        "localization.edit.preview",
        json!({"project_id":id,"request":draft}),
    );
    assert_eq!(preview["plan"]["can_apply"], true, "{preview}");
    let applied = call(
        server,
        "localization.edit.apply",
        json!({"project_id":id,"request":draft,"plan_digest":preview["plan"]["plan_digest"]}),
    );
    assert_eq!(applied["ok"], true, "{applied}");
    assert_eq!(applied["applied"], true);
    assert_eq!(applied["saved"], false);
    applied
}

fn locale_open(story: &str) -> Value {
    json!({"story_id":story,"seed":71,"capabilities":["runtime.localization.v1"],
        "localization":{"schema_version":1,"target_locale":"zh-Hans","policy":"strict"}})
}

#[test]
fn in_memory_translation_compiles_into_a_frozen_localized_story_without_saving() {
    let (root, mut server, id) = fixture("memory");
    let before = call(&mut server, "compile", json!({"project_id":id}));
    let before_story = before["story_id"].as_str().unwrap().to_owned();
    let applied = translate(&mut server, &id);
    assert!(!root.join(".world/localization/zh-Hans.json").exists());
    let catalog = call(
        &mut server,
        "localization.catalog",
        json!({"project_id":id,"request":{
            "schema_version":1,"target_locale":"zh-Hans","limit":50
        }}),
    );
    assert_eq!(catalog["page"]["status_counts"]["translated"], 2);
    let old = call(&mut server, "session.open", locale_open(&before_story));
    assert_eq!(
        old["ok"], false,
        "old story must retain its pre-translation snapshot"
    );
    let compiled = call(&mut server, "compile", json!({"project_id":id}));
    let story = compiled["story_id"].as_str().unwrap().to_owned();
    let session = call(&mut server, "session.open", locale_open(&story));
    assert_eq!(session["capabilities"], json!(["runtime.localization.v1"]));
    let session_id = session["session_id"].as_str().unwrap().to_owned();
    let turn = call(
        &mut server,
        "session.continue",
        json!({"session_id":session_id}),
    );
    assert_eq!(turn["outputs"][0]["content"], "你好，Ari!");
    assert_eq!(
        turn["outputs"][0]["localization"]["source_content"],
        "Hello Ari!"
    );
    assert_eq!(turn["choices"][0]["label"], "继续");
    assert!(!root.join(".world/localization/zh-Hans.json").exists());
    let save = call(
        &mut server,
        "project.save",
        json!({"project_id":id,"expected_baseline":applied["baseline"]}),
    );
    assert_eq!(save["saved"], true, "{save}");
    assert!(root.join(".world/localization/zh-Hans.json").exists());
}

#[test]
fn locale_persistence_replay_and_protocol_capability_are_explicit() {
    let (_, mut server, id) = fixture("persistence");
    translate(&mut server, &id);
    let compiled = call(&mut server, "compile", json!({"project_id":id}));
    let story = compiled["story_id"].as_str().unwrap().to_owned();
    let mut unnegotiated = locale_open(&story);
    unnegotiated.as_object_mut().unwrap().remove("capabilities");
    assert!(server.call("session.open", &unnegotiated).is_err());
    let opened = call(&mut server, "session.open", locale_open(&story));
    let session = opened["session_id"].as_str().unwrap().to_owned();
    call(
        &mut server,
        "session.continue",
        json!({"session_id":session}),
    );
    let save = call(&mut server, "session.save", json!({"session_id":session}));
    let old_entry = call(
        &mut server,
        "session.open",
        json!({"story_id":story,"save":save["save"]}),
    );
    assert_eq!(old_entry["ok"], false);
    let mut restore = locale_open(&story);
    restore.as_object_mut().unwrap().remove("seed");
    restore["save"] = save["save"].clone();
    let restored = call(&mut server, "session.open", restore);
    assert!(restored["session_id"].is_string(), "{restored}");
    call(
        &mut server,
        "session.choose",
        json!({"session_id":session,"index":0}),
    );
    let trace = call(&mut server, "session.trace", json!({"session_id":session}));
    let replay = call(
        &mut server,
        "trace.replay",
        json!({"story_id":story,"trace":trace["trace"]}),
    );
    assert_eq!(replay["ok"], true, "{replay}");
    let mut draft = request(&server, &id);
    draft.edits[0].translation_parts.insert(
        0,
        LocalizationPart::Text {
            text: "修订 ".into(),
        },
    );
    let plan = call(
        &mut server,
        "localization.edit.preview",
        json!({"project_id":id,"request":draft}),
    );
    call(
        &mut server,
        "localization.edit.apply",
        json!({"project_id":id,"request":draft,"plan_digest":plan["plan"]["plan_digest"]}),
    );
    let changed = call(&mut server, "compile", json!({"project_id":id}));
    let rejected = call(
        &mut server,
        "trace.replay",
        json!({"story_id":changed["story_id"],"trace":trace["trace"]}),
    );
    assert_eq!(rejected["ok"], false, "{rejected}");
    assert_eq!(
        call(
            &mut server,
            "trace.replay",
            json!({"story_id":story,"trace":trace["trace"]})
        )["ok"],
        true
    );
}

#[test]
fn new_authoring_methods_reject_unknown_parameters_and_stale_plans_without_mutation() {
    let (_, mut server, id) = fixture("invalid");
    let initial = server.projects[&id].project.content_baseline();
    let draft = request(&server, &id);
    assert!(server
        .call(
            "localization.edit.preview",
            &json!({"project_id":id,"request":draft,"save":true})
        )
        .is_err());
    let rejected = call(
        &mut server,
        "localization.edit.apply",
        json!({"project_id":id,"request":draft,"plan_digest":"stale"}),
    );
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["applied"], false);
    assert_eq!(server.projects[&id].project.content_baseline(), initial);
    assert!(server
        .call(
            "compile",
            &json!({"project_id":id,"source":"event other\n  -> END\n"})
        )
        .is_err());
    assert!(server
        .call(
            "compile",
            &json!({"project_id":id,"file_name":"ignored.wl"})
        )
        .is_err());
}

#[test]
fn legacy_rpc_import_still_saves_immediately_with_original_parameters() {
    let (root, mut server, id) = fixture("legacy");
    let selection = json!({"schema_version":1,"source_locale":"en","target_locale":"zh-Hans","string_ids":["greeting"]});
    let exported = call(
        &mut server,
        "localization.export.preview",
        json!({"project_id":id,"selection":selection}),
    );
    assert_eq!(exported["plan"]["can_export"], true, "{exported}");
    let mut exchange = exported["plan"]["exchange"].clone();
    exchange["entries"][0]["translation_parts"] = exchange["entries"][0]["source_parts"].clone();
    exchange["entries"][0]["translation_parts"][0]["text"] = json!("你好，");
    let preview = call(
        &mut server,
        "localization.import.preview",
        json!({"project_id":id,"selection":selection,"exchange":exchange}),
    );
    assert_eq!(preview["plan"]["can_apply"], true, "{preview}");
    assert!(!root.join(".world/localization/zh-Hans.json").exists());
    let applied = call(
        &mut server,
        "localization.import.apply",
        json!({"project_id":id,"selection":selection,"exchange":exchange,"plan_digest":preview["plan"]["plan_digest"]}),
    );
    assert_eq!(applied["ok"], true, "{applied}");
    assert!(
        fs::read_to_string(root.join(".world/localization/zh-Hans.json"))
            .unwrap()
            .contains("你好，")
    );
    assert_eq!(fs::read(root.join("world.wl")).unwrap(), SOURCE.as_bytes());
}

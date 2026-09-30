use std::{fs, path::PathBuf};
use worldline_core::{project::Project, TargetRef};
const SOURCE: &str = r#"character doctor as "PRIVATE_SPEAKER_SENTINEL"
tag map as "海图"
state evidence on character doctor with map
let fuel = 8
rule fare(n: num) -> num = n * 2
rule ready() -> bool = count(members(state(evidence))) > 0
fragment receipt(place: str, selected: tag)
  local fee: num = fare(2)
  say doctor "{place}需要{fee}罐油。" direction "PRIVATE_DIRECTION_SENTINEL fare(2) doctor" #wl-localization:spoken
  choice "交出"
    become state(evidence) remove from tags(selected)
  return
event start
  call receipt("诊所", tag(map))
  普通 fare(2) receipt doctor 文本不应改动。
  choice "完成" if ready()
    -> END
"#;
fn fixture(name: &str) -> (PathBuf, Project) {
    let root =
        std::env::temp_dir().join(format!("language-authoring-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join(".world/project.json"),r#"{"schema_version":1,"language_version":"1.11","required_features":["content.localization.v1"]}"#).unwrap();
    fs::write(root.join("world.wl"), SOURCE).unwrap();
    let project = Project::open(&root).unwrap();
    (root, project)
}
#[test]
fn callable_and_speaker_references_block_deletion_and_rename_atomically() {
    let (root, mut project) = fixture("rename");
    for (kind, id, new_id) in [
        ("rule", "fare", "cost"),
        ("fragment", "receipt", "bill"),
        ("character", "doctor", "medic"),
        ("tag", "map", "chart"),
        ("state", "evidence", "proofs"),
    ] {
        let target = TargetRef::new(kind, id);
        let impact = project.deletion_impact(&target);
        assert!(impact.complete, "{:?}", impact.diagnostics);
        assert!(!impact.can_delete(), "{kind} {id}");
        assert!(!impact.content_references.is_empty());
        let before = project.content_baseline();
        let plan = project
            .plan_rename_target(&target, new_id)
            .unwrap_or_else(|e| panic!("{kind} {id}: {e}"));
        assert_eq!(before, project.content_baseline());
        project.apply_rename_plan(&plan).unwrap();
        assert!(project.apply_rename_plan(&plan).is_err());
    }
    let text = project.document(&root.join("world.wl")).unwrap();
    assert!(text.contains("普通 fare(2) receipt doctor 文本不应改动。"));
    assert!(text.contains("PRIVATE_DIRECTION_SENTINEL fare(2) doctor"));
    assert!(text.contains("say medic"));
    assert!(text.contains("call bill"));
    assert!(text.contains("tag(chart)"));
    assert!(text.contains("state(proofs)"));
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn fragment_say_translation_and_reader_do_not_disclose_author_data() {
    let (root, mut project) = fixture("privacy");
    let path = root.join("world.wl");
    let source = project
        .document(&path)
        .unwrap()
        .replace("character doctor", "character hidden_speaker_id_sentinel")
        .replace("say doctor", "say hidden_speaker_id_sentinel");
    project.set_text(&path, source).unwrap();
    let selection = worldline_core::localization::LocalizationSelection {
        schema_version: 1,
        source_locale: "zh".into(),
        target_locale: "en".into(),
        string_ids: vec!["spoken".into()],
    };
    let plan = project.preview_localization_export(&selection).unwrap();
    assert!(plan.can_export, "{:?}", plan.diagnostics);
    let json = serde_json::to_string(&plan.exchange).unwrap();
    assert!(!json.contains("PRIVATE_SPEAKER_SENTINEL"));
    assert!(!json.contains("PRIVATE_DIRECTION_SENTINEL"));
    assert_eq!(plan.exchange.entries[0].source.kind, "say");
    let request=serde_json::from_value(serde_json::json!({"schema_version":2,"required_features":["reader.fields.v1"],"site_title":"公开","objects":[{"kind":"event","id":"start"},{"kind":"fragment","id":"receipt"},{"kind":"rule","id":"fare"}],"manuscripts":[],"attachments":[]})).unwrap();
    let preview = project.preview_reader_export(&request).unwrap();
    let files = project
        .build_reader_export(&request, &preview.plan_digest)
        .unwrap();
    let bytes = files
        .iter()
        .map(|(p, b)| format!("{} {}", p.display(), String::from_utf8_lossy(b)))
        .collect::<String>();
    for secret in [
        "PRIVATE_SPEAKER_SENTINEL",
        "PRIVATE_DIRECTION_SENTINEL",
        "hidden_speaker_id_sentinel",
        "n * 2",
    ] {
        assert!(!bytes.contains(secret), "leak {secret}");
    }
    assert!(bytes.contains("罐油"));
    assert!(preview.content.iter().all(|p| !p.empty_content));
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn event_and_choice_forms_preserve_new_statement_bodies() {
    let (root, mut project) = fixture("forms");
    let (path, mut draft) = project.event_draft("start").unwrap();
    let call = "call receipt(\"诊所\", tag(map))";
    assert!(draft.body.contains(call));
    let mut choices = draft.choices();
    let choice = choices.first_mut().unwrap();
    choice.label = "结束本次".into();
    draft.write_choice(Some(choice.line), choice).unwrap();
    project
        .edit(|p| p.write_event(&path, Some("start"), &draft))
        .unwrap();
    let text = project.document(&path).unwrap();
    assert!(text.contains(call));
    assert!(text.contains("PRIVATE_DIRECTION_SENTINEL"));
    assert!(text.contains("#wl-localization:spoken"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rename_preserves_shadowed_parameters_literals_comments_and_stage_notes() {
    let (root, mut project) = fixture("rename-boundaries");
    let path = root.join("world.wl");
    let source = SOURCE
        .replace(
            "rule fare(n: num) -> num = n * 2",
            "rule fare(fare: num) -> num = fare * 2 // fare(2) 注释",
        )
        .replace("{place}需要{fee}罐油。", "{place}需要{fare(2)}罐油。")
        .replace(
            "PRIVATE_DIRECTION_SENTINEL fare(2) doctor",
            "PRIVATE_DIRECTION_SENTINEL [[rule:fare|私有]] fare(2) doctor",
        );
    project.set_text(&path, source).unwrap();
    let plan = project
        .plan_rename_target(&TargetRef::new("rule", "fare"), "cost")
        .unwrap();
    project.apply_rename_plan(&plan).unwrap();
    let text = project.document(&path).unwrap();
    assert!(text.contains("rule cost(fare: num) -> num = fare * 2 // fare(2) 注释"));
    assert!(text.contains("{cost(2)}"));
    assert!(text.contains("PRIVATE_DIRECTION_SENTINEL [[rule:fare|私有]] fare(2) doctor"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn external_change_rejects_the_entire_rename() {
    let (root, mut project) = fixture("rename-disk");
    let plan = project
        .plan_rename_target(&TargetRef::new("rule", "fare"), "cost")
        .unwrap();
    let baseline = project.content_baseline();
    fs::write(
        root.join("world.wl"),
        format!("{SOURCE}\n// external change\n"),
    )
    .unwrap();
    assert!(project.apply_rename_plan(&plan).is_err());
    assert_eq!(baseline, project.content_baseline());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn say_in_fragment_imports_translation_through_the_existing_sidecar_transaction() {
    let (root, mut project) = fixture("say-import");
    let selection = worldline_core::localization::LocalizationSelection {
        schema_version: 1,
        source_locale: "zh".into(),
        target_locale: "en".into(),
        string_ids: vec!["spoken".into()],
    };
    let mut exchange = project
        .preview_localization_export(&selection)
        .unwrap()
        .exchange;
    let parts = exchange.entries[0]
        .source_parts
        .iter()
        .map(|part| match part {
            worldline_core::localization::LocalizationPart::Text { text } => {
                worldline_core::localization::LocalizationPart::Text {
                    text: format!("translated {text}"),
                }
            }
            other => other.clone(),
        })
        .collect();
    exchange.entries[0].translation_parts = Some(parts);
    let preview = project
        .preview_localization_import(&selection, &exchange)
        .unwrap();
    assert!(preview.can_apply, "{:?}", preview.diagnostics);
    project
        .apply_localization_import(&selection, &exchange, &preview.plan_digest)
        .unwrap();
    let bytes = fs::read(root.join(".world/localization/en.json")).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("translated"));
    assert!(!text.contains("PRIVATE_DIRECTION_SENTINEL"));
    assert!(!text.contains("PRIVATE_SPEAKER_SENTINEL"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn escaped_interpolation_links_and_opaque_tags_are_not_renamed() {
    let (root, mut project) = fixture("escaped");
    let source=SOURCE.replace("rule ready()", "rule caption(value: str) -> str = value\nrule ready()")
        .replace("  普通 fare(2) receipt doctor 文本不应改动。",r#"  实际{fare(2)}，字面\{fare(2)\}，\[[rule:fare|伪链接]]，[[rule:fare|真链接]]，{caption("}")} #opaque:{fare(2)}"#)
        .replace("{place}需要{fee}罐油。",r#"实际{fare(2)}，字面\{fare(2)\}，\[[rule:fare|伪链接]]，[[rule:fare|真链接]]，{caption(\"}\")}"#);
    let path = root.join("world.wl");
    project.set_text(&path, source).unwrap();
    let plan = project
        .plan_rename_target(&TargetRef::new("rule", "fare"), "cost")
        .unwrap();
    project.apply_rename_plan(&plan).unwrap();
    let text = project.document(&path).unwrap();
    assert!(text.contains(r"字面\{fare(2)\}"), "{text}");
    assert!(text.contains(r"\[[rule:fare|伪链接]]"), "{text}");
    assert!(text.contains("[[rule:cost|真链接]]"), "{text}");
    assert!(text.contains("#opaque:{fare(2)}"), "{text}");
    assert!(text.contains("实际{cost(2)}"), "{text}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn quoted_static_identity_in_say_is_renamed_without_touching_escaped_links_or_strings() {
    let (root, mut project) = fixture("quoted-identity");
    let source = SOURCE.replace(
        "{place}需要{fee}罐油。",
        r#"{tag(\"map\")} [[tag:map|真链接]] \[[tag:map|伪链接]] {has(\"evidence\", \"map\")}"#,
    );
    let path = root.join("world.wl");
    project.set_text(&path, source).unwrap();
    let plan = project
        .plan_rename_target(&TargetRef::new("tag", "map"), "chart")
        .unwrap();
    project.apply_rename_plan(&plan).unwrap();
    let text = project.document(&path).unwrap();
    assert!(text.contains(r#"{tag(\"chart\")}"#), "{text}");
    assert!(text.contains("[[tag:chart|真链接]]"));
    assert!(text.contains(r"\[[tag:map|伪链接]]"));
    assert!(text.contains(r#"{has(\"evidence\", \"chart\")}"#));
    fs::remove_dir_all(root).unwrap();
}

use super::*;
use worldline_core::localization::{
    LocalizationCatalogQuery, LocalizationEdit, LocalizationEditDraft, LocalizationPart,
    LocalizationStatus,
};

fn translated(project: &mut Project) {
    let page = project
        .query_localization_catalog(&LocalizationCatalogQuery::default())
        .unwrap();
    let entry = &page.entries[0];
    let draft = LocalizationEditDraft {
        schema_version: 1,
        source_locale: "zh".into(),
        target_locale: "en".into(),
        source_baseline: page.source_baseline,
        edits: vec![LocalizationEdit {
            id: entry.id.clone().unwrap(),
            source_revision: entry.source_revision.clone().unwrap(),
            translation_parts: vec![LocalizationPart::Text {
                text: "translated".into(),
            }],
        }],
    };
    let plan = project.preview_localization_edit(&draft).unwrap();
    project
        .apply_localization_edit(&draft, &plan.plan_digest)
        .unwrap();
}
fn catalog(project: &Project) -> worldline_core::localization::LocalizationCatalogEntry {
    project
        .query_localization_catalog(&LocalizationCatalogQuery {
            target_locale: Some("en".into()),
            ..Default::default()
        })
        .unwrap()
        .entries
        .remove(0)
}

#[test]
fn speaker_and_direction_keep_translation_revision_but_kind_conversion_stales_with_id_retained() {
    let source = BASIC.replace("say a \"原句\"", "say a \"原句\" #wl-localization:line_a");
    let (_work, mut project) = Workspace::new(&source, "1.11", true);
    translated(&mut project);
    let translated_before = catalog(&project);
    assert_eq!(translated_before.status, LocalizationStatus::Translated);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let rows = project_rows(&project, &buffer);
    let old = &rows.statements[0];
    let mut draft = old.draft.clone();
    draft.speaker = Some(TargetRef::new("character", "b"));
    draft.direction = Some("作者备注".into());
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id.clone(),
            draft,
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    project.apply_dialogue_edit(&buffer, &plan).unwrap();
    let unchanged_revision = catalog(&project);
    assert_eq!(unchanged_revision.status, LocalizationStatus::Translated);
    assert_eq!(
        translated_before.source_revision,
        unchanged_revision.source_revision
    );
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let newer = project_rows(&project, &buffer);
    assert_ne!(rows.snapshot, newer.snapshot);
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Convert {
            statement_id: newer.statements[0].id.clone(),
            to: DialogueKind::Text,
            speaker: None,
            allow_direction_loss: true,
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    project.apply_dialogue_edit(&buffer, &plan).unwrap();
    let stale = catalog(&project);
    assert_eq!(stale.status, LocalizationStatus::StaleSource);
    assert_eq!(stale.id.as_deref(), Some("line_a"));
    assert_eq!(
        stale.translation_parts,
        unchanged_revision.translation_parts
    );
    assert_ne!(stale.source_revision, unchanged_revision.source_revision);
}

#[test]
fn direction_only_keeps_runtime_fingerprint_and_text_to_say_stales_translation() {
    let source = "character a\nevent start\n  原句 #wl-localization:line_a\n  -> END\n";
    let (_work, mut project) = Workspace::new(source, "1.11", true);
    translated(&mut project);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Convert {
            statement_id: old.id,
            to: DialogueKind::Say,
            speaker: Some(TargetRef::new("character", "a")),
            allow_direction_loss: false,
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    project.apply_dialogue_edit(&buffer, &plan).unwrap();
    assert_eq!(catalog(&project).status, LocalizationStatus::StaleSource);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let mut draft = old.draft;
    draft.direction = Some("只改变作者元数据".into());
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id,
            draft,
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    assert_eq!(
        plan.runtime_fingerprint_before,
        plan.runtime_fingerprint_after
    );
}

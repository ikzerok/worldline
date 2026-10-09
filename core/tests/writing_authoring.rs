#![cfg(not(target_arch = "wasm32"))]

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{
    authoring::{CharacterDraft, EntityDraft},
    authoring_intents::{IntentTarget, TextSelection},
    catalog::TargetRef,
    manuscript::{WritingAuthoringPlan, WritingAuthoringRequest, WritingBuffer},
    project::Project,
    LanguageVersion, Severity,
};

mod writing_authoring {
    use super::*;
    mod guards;
    mod migration;
}

const SOURCE: &str = "event start\n  你看见林😀。\n  -> END\n";

struct Workspace(PathBuf);
impl Workspace {
    fn new(files: &[(&str, &str)], manifest: Option<&str>) -> (Self, Project) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-writing-authoring-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for (path, text) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        if let Some(manifest) = manifest {
            fs::create_dir_all(root.join(".world")).unwrap();
            fs::write(root.join(".world/project.json"), manifest).unwrap();
        }
        let project = Project::open(&root).unwrap();
        (Self(root), project)
    }

    fn bytes(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    visit(root, &entry.path(), files);
                } else {
                    files.insert(
                        entry.path().strip_prefix(root).unwrap().to_owned(),
                        fs::read(entry.path()).unwrap(),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        visit(&self.0, &self.0, &mut files);
        files
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn start() -> TargetRef {
    TargetRef::new("event", "start")
}

fn character(path: &Path) -> IntentTarget {
    IntentTarget::CreateCharacter {
        path: path.to_owned(),
        draft: CharacterDraft {
            id: "lin".into(),
            display: "林😀".into(),
            ..Default::default()
        },
    }
}

fn entity(path: &Path) -> IntentTarget {
    IntentTarget::CreateEntity {
        path: path.to_owned(),
        draft: EntityDraft {
            id: "lin".into(),
            entity_type: "place".into(),
            display: "林😀".into(),
            description: "作者明确创建的地点".into(),
            ..Default::default()
        },
    }
}

fn request(
    project: &Project,
    buffer: &WritingBuffer,
    label: &str,
    target: IntentTarget,
) -> WritingAuthoringRequest {
    let offset = buffer.source().rfind(label).unwrap();
    WritingAuthoringRequest {
        expected_baseline: project.content_baseline(),
        source: start(),
        generation: buffer.generation(),
        selection: TextSelection {
            path: buffer.path().to_owned(),
            start: offset,
            end: offset + label.len(),
            expected_text: label.into(),
        },
        target,
        enable_entities: false,
    }
}

fn buffer_state(buffer: &WritingBuffer) -> (String, String, u64, bool) {
    (
        buffer.source().into(),
        buffer.baseline().into(),
        buffer.generation(),
        buffer.is_changed(),
    )
}

#[test]
fn existing_reference_preserves_utf8_crlf_and_all_unselected_bytes_until_explicit_apply() {
    let source = concat!(
        "character lin as \"林😀\"\r\n",
        "// 前言🧭\r\nevent start\r\n  你看见林😀，身后是海。\r\n  -> END\r\n",
        "\r\nevent later\r\n  下一来源保持原样。\r\n  -> END"
    );
    let (work, mut project) = Workspace::new(&[("world.wl", source)], None);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let draft = source.replace("身后是海", "身后是尚未应用的海🌊");
    buffer.replace_source(draft.clone());
    let before = buffer_state(&buffer);
    let baseline = project.content_baseline();
    let disk = work.bytes();
    let command = request(
        &project,
        &buffer,
        "林😀",
        IntentTarget::Existing(TargetRef::new("character", "lin")),
    );
    let plan = project
        .preview_writing_authoring(&[buffer.clone()], &command)
        .unwrap();
    let expected = draft.replacen("你看见林😀", "你看见[[character:lin|林😀]]", 1);
    assert!(!plan.creates_object());
    assert!(plan.can_apply);
    assert_eq!(plan.changed_files(), vec![project.entry.clone()]);
    assert_eq!(plan.changes[0].before.as_deref(), Some(source));
    assert_eq!(plan.changes[0].after, expected);
    assert!(plan.changes[0].includes_unapplied_draft);
    assert_eq!(plan.included_buffers.len(), 1);
    assert_eq!(plan.included_buffers[0].generation, 1);
    assert_eq!(buffer_state(&buffer), before);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(work.bytes(), disk);
    assert!(project
        .apply_writing_authoring(&[buffer.clone()], &plan)
        .is_err());
    project
        .insert_writing_reference(&mut buffer, &plan)
        .unwrap();
    assert_eq!(buffer.source(), expected);
    assert_eq!(buffer.generation(), 2);
    assert_eq!(buffer.baseline(), baseline);
    assert_eq!(project.document(&project.entry).unwrap(), source);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(work.bytes(), disk);
    assert_eq!(
        &buffer.source()[plan.link_start_utf8..plan.cursor_utf8],
        "[[character:lin|林😀]]"
    );
    assert!(buffer.source().is_char_boundary(plan.cursor_utf8));
    let inserted = buffer_state(&buffer);
    assert!(project
        .insert_writing_reference(&mut buffer, &plan)
        .is_err());
    assert_eq!(buffer_state(&buffer), inserted);
    project.apply_writing_buffer(&buffer).unwrap();
    assert_eq!(project.document(&project.entry).unwrap(), expected);
    assert_eq!(work.bytes(), disk);
    project.save().unwrap();
    let reopened = Project::open(&work.0).unwrap();
    assert_eq!(reopened.document(&reopened.entry).unwrap(), expected);
    let result = reopened.compile_object_search_snapshot();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(result.analysis.catalog.text_links.len(), 1);
    assert_eq!(result.analysis.catalog.text_links[0].source, start());
    assert_eq!(result.analysis.catalog.text_links[0].label, "林😀");
}

#[test]
fn new_character_and_same_file_full_draft_form_one_reversible_unsaved_transaction() {
    let source = format!("{SOURCE}\nevent later\n  另一个来源🧭。\n  -> END");
    let (work, mut project) = Workspace::new(&[("world.wl", &source)], None);
    let before = project.clone();
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let draft = source.replace("你看见", "未应用前缀：你看见");
    buffer.replace_source(draft.clone());
    let keep = buffer_state(&buffer);
    let command = request(&project, &buffer, "林😀", character(&project.entry));
    let disk = work.bytes();
    let baseline = project.content_baseline();
    let plan = project
        .preview_writing_authoring(&[buffer.clone()], &command)
        .unwrap();
    assert!(plan.can_apply);
    assert!(plan.creates_object());
    assert!(plan.migration.is_none());
    assert_eq!(plan.target, TargetRef::new("character", "lin"));
    assert_eq!(plan.included_buffers.len(), 1);
    assert_eq!(plan.changes.len(), 1);
    assert!(plan.changes[0].includes_unapplied_draft);
    assert_eq!(plan.changes[0].before.as_deref(), Some(source.as_str()));
    assert!(plan.changes[0].after.ends_with(&draft.replacen(
        "你看见林😀",
        "你看见[[character:lin|林😀]]",
        1
    )));
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(buffer_state(&buffer), keep);
    assert_eq!(work.bytes(), disk);
    assert!(project
        .insert_writing_reference(&mut buffer, &plan)
        .is_err());
    project
        .apply_writing_authoring(&[buffer.clone()], &plan)
        .unwrap();
    assert_eq!(project.language_version(), "1.9");
    assert!(project.is_dirty());
    assert_eq!(buffer_state(&buffer), keep);
    assert_eq!(work.bytes(), disk);
    let written = project.document(&project.entry).unwrap().to_owned();
    assert_eq!(written, plan.changes[0].after);
    assert!(written.starts_with("character lin as \"林😀\"\n"));
    assert!(!written.contains("entity lin"));
    assert_eq!(
        &written[plan.link_start_utf8..plan.cursor_utf8],
        "[[character:lin|林😀]]"
    );
    let compiled = project.compile_object_search_snapshot();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert!(compiled
        .analysis
        .catalog
        .object(&TargetRef::new("character", "lin"))
        .is_some());
    assert!(compiled.analysis.catalog.entities.is_empty());
    assert_eq!(
        compiled.analysis.fingerprint,
        plan.runtime_fingerprint_after
    );
    let after = project.clone();
    let after_baseline = project.content_baseline();
    assert!(project
        .apply_writing_authoring(&[buffer.clone()], &plan)
        .is_err());
    assert_eq!(project.content_baseline(), after_baseline);
    assert_eq!(project.document(&project.entry).unwrap(), written);
    assert!(project.restore(before.clone()));
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.document(&project.entry).unwrap(), source);
    assert!(project
        .compile_object_search_snapshot()
        .analysis
        .catalog
        .object(&TargetRef::new("character", "lin"))
        .is_none());
    assert_eq!(buffer_state(&buffer), keep);
    assert!(project.restore(after.clone()));
    assert_eq!(project.document(&project.entry).unwrap(), written);
    project.save().unwrap();
    let reopened = Project::open(&work.0).unwrap();
    assert_eq!(reopened.document(&reopened.entry).unwrap(), written);
    assert_eq!(reopened.language_version(), "1.9");
    assert!(project.restore(before));
    assert!(project.is_dirty(), "保存后撤销仍须保留相对磁盘的脏状态");
    assert!(project.restore(after));
    assert!(!project.is_dirty());
}

#[test]
fn destination_full_draft_is_mandatory_but_unrelated_invalid_draft_is_not_applied() {
    let destination = "// 人物资料原稿\n";
    let unrelated = "event aside\n  旁章原稿。\n  -> END\n";
    let (work, mut project) = Workspace::new(
        &[
            ("world.wl", SOURCE),
            ("people.wl", destination),
            ("aside.wl", unrelated),
        ],
        None,
    );
    let mut source = project.open_writing_buffer(&start()).unwrap();
    source.replace_source(SOURCE.replace("你看见", "来源整稿修改：你看见"));
    let path = work.0.join("people.wl");
    let mut people = project.open_source_writing_buffer(&path).unwrap();
    let people_draft = "// 未提交人物资料注释😀\ncharacter elder as \"前辈\"\n";
    people.replace_source(people_draft.into());
    let mut other = project
        .open_source_writing_buffer(&work.0.join("aside.wl"))
        .unwrap();
    other.replace_source("event\n  尚未修复的原稿😀\n".into());
    let states = [
        buffer_state(&source),
        buffer_state(&people),
        buffer_state(&other),
    ];
    let buffers = vec![source.clone(), people.clone(), other.clone()];
    let command = request(&project, &source, "林😀", character(&path));
    let disk = work.bytes();
    let before = project.clone();
    let baseline = project.content_baseline();
    let plan = project
        .preview_writing_authoring(&buffers, &command)
        .unwrap();
    assert_eq!(plan.changes.len(), 2);
    assert_eq!(plan.included_buffers.len(), 2);
    assert!(plan
        .changes
        .iter()
        .all(|change| change.includes_unapplied_draft));
    assert!(plan.included_buffers.iter().all(|entry| entry.changed));
    assert!(plan
        .included_buffers
        .iter()
        .all(|entry| entry.generation == 1));
    assert!(!plan
        .included_buffers
        .iter()
        .any(|entry| entry.path == other.path()));
    let people_change = plan
        .changes
        .iter()
        .find(|change| change.path == path)
        .unwrap();
    assert_eq!(people_change.before.as_deref(), Some(destination));
    assert!(people_change.after.ends_with(people_draft));
    assert!(people_change.after.contains("character lin as \"林😀\""));
    assert!(
        project
            .apply_writing_authoring(&[source.clone(), other.clone()], &plan)
            .is_err(),
        "确认后删掉资料目标缓冲不能省略它的全文输入"
    );
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(work.bytes(), disk);
    project.apply_writing_authoring(&buffers, &plan).unwrap();
    assert_eq!(project.document(&path).unwrap(), people_change.after);
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("来源整稿修改"));
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("[[character:lin|林😀]]"));
    assert_eq!(project.document(other.path()).unwrap(), unrelated);
    assert_eq!(buffer_state(&source), states[0]);
    assert_eq!(buffer_state(&people), states[1]);
    assert_eq!(buffer_state(&other), states[2]);
    assert_eq!(work.bytes(), disk);
    assert!(!project.compile_object_search_snapshot().has_errors());
    let other_text = other.source().to_owned();
    let other_generation = other.generation();
    other.rebase_unchanged_source(&project).unwrap();
    assert_eq!(other.source(), other_text);
    assert_eq!(other.generation(), other_generation);
    assert_eq!(other.baseline(), project.content_baseline());
    assert!(people.rebase_unchanged_source(&project).is_err());
    let after = project.clone();
    assert!(project.restore(before));
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.document(&path).unwrap(), destination);
    assert_eq!(project.document(&project.entry).unwrap(), SOURCE);
    assert!(project.restore(after));
    project.save().unwrap();
    let reopened = Project::open(&work.0).unwrap();
    assert_eq!(reopened.document(&path).unwrap(), people_change.after);
    assert_eq!(reopened.document(other.path()).unwrap(), unrelated);
    assert_eq!(
        reopened
            .compile_object_search_snapshot()
            .analysis
            .catalog
            .text_links
            .len(),
        1
    );
}

#[test]
fn one_file_buffer_supports_distinct_sources_and_refuses_a_selection_from_another_source() {
    let source = concat!(
        "character lin as \"林😀\"\n",
        "event start\n  第一来源的林😀。\n  -> END\n",
        "event second\n  第二来源的林😀。\n  -> END\n"
    );
    let (_work, project) = Workspace::new(&[("world.wl", source)], None);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let mut command = request(
        &project,
        &buffer,
        "林😀",
        IntentTarget::Existing(TargetRef::new("character", "lin")),
    );
    assert!(project
        .preview_writing_authoring(&[buffer.clone()], &command)
        .is_err());
    command.source = TargetRef::new("event", "second");
    let plan = project
        .preview_writing_authoring(&[buffer.clone()], &command)
        .unwrap();
    project
        .insert_writing_reference(&mut buffer, &plan)
        .unwrap();
    assert!(buffer.source().contains("第一来源的林😀。"));
    assert!(buffer
        .source()
        .contains("第二来源的[[character:lin|林😀]]。"));
    let first = project.project_writing_buffer(&buffer, &start()).unwrap();
    let second = project
        .project_writing_buffer(&buffer, &command.source)
        .unwrap();
    assert_eq!(first.generation, 1);
    assert_eq!(second.generation, 1);
    assert!(!first.source.contains("[[character:lin"));
    assert!(second.source.contains("[[character:lin"));
    assert_eq!(project.document(&project.entry).unwrap(), source);
}

#[test]
fn choice_label_is_a_valid_link_selection() {
    let source = "event start\n  choice \"向林😀问路\"\n    -> END\n";
    let (_work, mut project) = Workspace::new(&[("world.wl", source)], None);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let command = request(&project, &buffer, "林😀", character(&project.entry));
    let plan = project
        .preview_writing_authoring(std::slice::from_ref(&buffer), &command)
        .unwrap();
    project.apply_writing_authoring(&[buffer], &plan).unwrap();
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("choice \"向[[character:lin|林😀]]问路\""));
    let compiled = project.compile_object_search_snapshot();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert_eq!(compiled.analysis.catalog.text_links[0].source, start());
}

use super::*;
use ManuscriptChapterCreateFailureCode as Code;

#[test]
fn stale_digest_repeat_click_and_changed_revision_never_add_second_objects() {
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    let mut revision = Revision::default();
    let request = request(&project, revision);
    let plan = project
        .preview_manuscript_chapter_create(revision, &request)
        .unwrap();
    let baseline = project.content_baseline();
    assert_eq!(
        project
            .apply_manuscript_chapter_create(&mut revision, &request, "tampered")
            .unwrap_err()
            .code,
        Code::StaleBaseline
    );
    assert_eq!(baseline, project.content_baseline());
    let mut wrong_revision = revision.next_presentation();
    assert!(project
        .apply_manuscript_chapter_create(&mut wrong_revision, &request, &plan.plan_digest)
        .is_err());
    apply(&mut project, &mut revision, &request);
    let baseline = project.content_baseline();
    assert_eq!(
        project
            .apply_manuscript_chapter_create(&mut revision, &request, &plan.plan_digest)
            .unwrap_err()
            .code,
        Code::StaleBaseline
    );
    assert_eq!(baseline, project.content_baseline());
    assert_eq!(project.manuscript_index("novel").unwrap().entries.len(), 1);
}

#[test]
fn duplicate_book_chapter_event_and_invalid_parent_are_classified() {
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    let mut revision = Revision::default();
    let request_one = request(&project, revision);
    apply(&mut project, &mut revision, &request_one);
    let mut next = request(&project, revision);
    assert_eq!(
        project
            .preview_manuscript_chapter_create(revision, &next)
            .unwrap_err()
            .code,
        Code::IdConflict
    );
    next.book = ManuscriptBookDestination::Existing { id: "novel".into() };
    assert_eq!(
        project
            .preview_manuscript_chapter_create(revision, &next)
            .unwrap_err()
            .code,
        Code::IdConflict
    );
    next.chapter.id = "chapter_two".into();
    next.chapter.parent_section_id = Some("chapter_one".into());
    assert_eq!(
        project
            .preview_manuscript_chapter_create(revision, &next)
            .unwrap_err()
            .code,
        Code::InvalidChapter
    );
    next.chapter.parent_section_id = None;
    new_event(&mut next, "world.wl", false);
    if let ManuscriptChapterSource::NewEvent { id, .. } = &mut next.source {
        *id = "start".into();
    }
    assert_eq!(
        project
            .preview_manuscript_chapter_create(revision, &next)
            .unwrap_err()
            .code,
        Code::IdConflict
    );
}

#[test]
fn destination_rejects_existing_case_alias_reserved_parent_and_unknown_source() {
    for path in [
        "world.wl",
        "WORLD.wl",
        "../escape.wl",
        "source.txt",
        ".world/.transactions/new.wl",
        "world.wl/child.wl",
        "C:/new.wl",
        "bad\\new.wl",
    ] {
        let work = Workspace::new();
        let project = Project::new(&work.0);
        let revision = Revision::default();
        let baseline = project.content_baseline();
        let mut request = request(&project, revision);
        new_event(&mut request, path, true);
        assert!(
            project
                .preview_manuscript_chapter_create(revision, &request)
                .is_err(),
            "{path}"
        );
        assert_eq!(project.content_baseline(), baseline);
        assert!(!work.0.exists());
    }
    let work = Workspace::new();
    let project = Project::new(&work.0);
    let mut request = request(&project, Revision::default());
    request.source = ManuscriptChapterSource::Existing {
        target: TargetRef::new("event", "missing"),
    };
    assert_eq!(
        project
            .preview_manuscript_chapter_create(Revision::default(), &request)
            .unwrap_err()
            .code,
        Code::SourceUnavailable
    );
}

#[test]
fn disk_inventory_external_edits_new_destination_and_pending_journal_are_zero_mutation() {
    for mode in ["source", "ordinary", "new_target", "journal", "new_source"] {
        let work = Workspace::new();
        let mut project = Project::new(&work.0);
        project.save().unwrap();
        fs::write(work.0.join("notes.txt"), "原资料").unwrap();
        let mut revision = Revision::default();
        let mut request = request(&project, revision);
        new_event(&mut request, "new.wl", true);
        let plan = project
            .preview_manuscript_chapter_create(revision, &request)
            .unwrap();
        let baseline = project.content_baseline();
        match mode {
            "source" => fs::write(&project.entry, "event start\n  外部改稿\n  -> END\n").unwrap(),
            "ordinary" => fs::write(work.0.join("notes.txt"), "外部资料变化").unwrap(),
            "new_target" => fs::write(work.0.join("new.wl"), "event outside\n  -> END\n").unwrap(),
            "new_source" => {
                fs::write(work.0.join("unloaded.wl"), "event outside\n  -> END\n").unwrap()
            }
            _ => {
                fs::create_dir_all(work.0.join(".world/.transactions/pending")).unwrap();
            }
        }
        assert!(
            project
                .apply_manuscript_chapter_create(&mut revision, &request, &plan.plan_digest)
                .is_err(),
            "{mode}"
        );
        assert_eq!(project.content_baseline(), baseline, "{mode}");
        assert!(project.manuscript_indices().is_empty());
        assert_eq!(revision, Revision::default());
    }
}

#[test]
fn new_book_never_takes_over_unknown_json_or_registered_other_document() {
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    project.save().unwrap();
    fs::create_dir_all(work.0.join(".world/manuscripts")).unwrap();
    let target = work.0.join(".world/manuscripts/novel.json");
    fs::write(&target, b"opaque ordinary bytes").unwrap();
    let request = request(&project, Revision::default());
    assert_eq!(
        project
            .preview_manuscript_chapter_create(Revision::default(), &request)
            .unwrap_err()
            .code,
        Code::InvalidDestination
    );
    assert_eq!(fs::read(&target).unwrap(), b"opaque ordinary bytes");
}

#[test]
fn request_json_is_strict_and_oversized_or_invalid_ids_are_bounded() {
    let work = Workspace::new();
    let project = Project::new(&work.0);
    let request = request(&project, Revision::default());
    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(
        parse_manuscript_chapter_create_request(&value.to_string()).unwrap(),
        request
    );
    for path in ["root", "revision", "target", "chapter", "book"] {
        let mut value = value.clone();
        let object = match path {
            "revision" => &mut value["expected_revision"],
            "target" => &mut value["source"]["target"],
            "chapter" => &mut value["chapter"],
            "book" => &mut value["book"],
            _ => &mut value,
        };
        object["unknown"] = serde_json::json!(true);
        assert!(
            parse_manuscript_chapter_create_request(&value.to_string()).is_err(),
            "{path}"
        );
    }
    assert!(
        parse_manuscript_chapter_create_request("{\"schema_version\":1,\"schema_version\":1}")
            .is_err()
    );
    let mut oversized = request;
    oversized.chapter.title = "大".repeat(2000);
    assert_eq!(
        project
            .preview_manuscript_chapter_create(Revision::default(), &oversized)
            .unwrap_err()
            .code,
        Code::BudgetExceeded
    );
}

#[test]
fn plan_is_bound_to_workspace_and_oversized_inventory_is_typed() {
    let one = Workspace::new();
    let two = Workspace::new();
    let first = Project::new(&one.0);
    let mut second = Project::new(&two.0);
    let mut revision = Revision::default();
    let request = request(&first, revision);
    assert_eq!(first.content_baseline(), second.content_baseline());
    let plan = first
        .preview_manuscript_chapter_create(revision, &request)
        .unwrap();
    assert_eq!(
        second
            .apply_manuscript_chapter_create(&mut revision, &request, &plan.plan_digest)
            .unwrap_err()
            .code,
        Code::StaleBaseline
    );
    second.save().unwrap();
    let oversized = fs::File::create(two.0.join("huge.bin")).unwrap();
    oversized.set_len(64 * 1024 * 1024 + 1).unwrap();
    assert_eq!(
        second
            .preview_manuscript_chapter_create(revision, &request)
            .unwrap_err()
            .code,
        Code::BudgetExceeded
    );
    assert!(second.manuscript_indices().is_empty());
}

#[cfg(unix)]
#[test]
fn source_path_permissions_and_links_cannot_be_bypassed_by_chapter_creation() {
    use std::os::unix::{fs::symlink, fs::PermissionsExt};
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    project.save().unwrap();
    let mut request = request(&project, Revision::default());
    new_event(&mut request, "world.wl", false);
    fs::set_permissions(&project.entry, fs::Permissions::from_mode(0o444)).unwrap();
    assert_eq!(
        project
            .preview_manuscript_chapter_create(Revision::default(), &request)
            .unwrap_err()
            .code,
        Code::ReadOnly
    );
    fs::set_permissions(&project.entry, fs::Permissions::from_mode(0o644)).unwrap();
    symlink(&project.entry, work.0.join("alias.wl")).unwrap();
    assert!(project
        .preview_manuscript_chapter_create(Revision::default(), &request)
        .is_err());
    assert_eq!(project.documents.len(), 1);
    assert!(project.manuscript_indices().is_empty());
}

#[test]
fn save_baseline_changes_invalidate_a_preview_even_without_content_changes() {
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    let mut revision = Revision::default();
    let request = request(&project, revision);
    let plan = project
        .preview_manuscript_chapter_create(revision, &request)
        .unwrap();
    let baseline = project.content_baseline();
    project.save().unwrap();
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(
        project
            .apply_manuscript_chapter_create(&mut revision, &request, &plan.plan_digest)
            .unwrap_err()
            .code,
        Code::StaleBaseline
    );
    assert!(project.manuscript_indices().is_empty());
    let fresh = project
        .preview_manuscript_chapter_create(revision, &request)
        .unwrap();
    project
        .apply_manuscript_chapter_create(&mut revision, &request, &fresh.plan_digest)
        .unwrap();
}

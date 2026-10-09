use super::*;
use crate::project::Project;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    root: PathBuf,
    project: Project,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn fixture() -> Fixture {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "worldline-localization-compile-count-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(
        root.join("world.wl"),
        "event start\n  First #wl-localization:first\n  Second #wl-localization:second\n  -> END\n",
    )
    .unwrap();
    fs::write(root.join(".world/project.json"), br#"{"schema_version":1,"language_version":"1.13","required_features":["content.localization.v1"]}"#).unwrap();
    let project = Project::open(&root).unwrap();
    Fixture { root, project }
}

#[test]
fn typed_edit_reuses_one_compilation_for_the_entire_explicit_batch() {
    let mut fixture = fixture();
    let page = fixture
        .project
        .query_localization_catalog(&LocalizationCatalogQuery::default())
        .unwrap();
    let draft = LocalizationEditDraft {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh".into(),
        source_baseline: page.source_baseline,
        edits: page
            .entries
            .iter()
            .map(|entry| LocalizationEdit {
                id: entry.id.clone().unwrap(),
                source_revision: entry.source_revision.clone().unwrap(),
                translation_parts: vec![LocalizationPart::Text {
                    text: "译文".into(),
                }],
            })
            .collect(),
    };
    source::take_compilation_count();
    let preview = fixture.project.preview_localization_edit(&draft).unwrap();
    assert!(preview.can_apply);
    assert_eq!(
        source::take_compilation_count(),
        1,
        "new typed preview must compile exactly once"
    );
    fixture
        .project
        .apply_localization_edit(&draft, &preview.plan_digest)
        .unwrap();
    assert_eq!(
        source::take_compilation_count(),
        1,
        "new typed apply must independently revalidate with one compilation"
    );
    let presentation = fixture
        .project
        .prepare_localization_presentation(&LocalizationPresentationRequest {
            schema_version: 1,
            target_locale: "zh".into(),
            policy: LocalizationPresentationPolicy::Strict,
        })
        .unwrap();
    assert_eq!(presentation.entries().len(), 2);
    assert_eq!(source::take_compilation_count(), 1);
}

#[test]
fn candidate_import_compiles_once_while_legacy_export_and_import_keep_independent_validation() {
    let fixture = fixture();
    let selection = LocalizationSelection {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh".into(),
        string_ids: vec!["first".into()],
    };
    source::take_compilation_count();
    let mut exchange = fixture
        .project
        .preview_localization_export(&selection)
        .unwrap()
        .exchange;
    assert_eq!(source::take_compilation_count(), 1);
    exchange.entries[0].translation_parts = Some(vec![LocalizationPart::Text {
        text: "中文".into(),
    }]);
    assert!(
        fixture
            .project
            .preview_localization_import_candidate(&selection, &exchange)
            .unwrap()
            .can_apply
    );
    assert_eq!(source::take_compilation_count(), 1);
    assert!(
        fixture
            .project
            .preview_localization_import(&selection, &exchange)
            .unwrap()
            .can_apply
    );
    assert_eq!(source::take_compilation_count(), 1);
}

#[test]
fn physical_line_count_is_exact_for_empty_eof_lf_crlf_and_blank_lines() {
    for (source, expected) in [
        ("", 0),
        ("a", 1),
        ("a\n", 1),
        ("a\r\n", 1),
        ("\n", 1),
        ("\r\n", 1),
        ("a\n\n", 2),
        ("a\r\nb", 2),
        ("\r", 1),
    ] {
        assert_eq!(limits::physical_lines(source), expected, "{source:?}");
    }
    let mut fixture = fixture();
    fixture
        .project
        .set_text(
            &fixture.root.join("world.wl"),
            "\n".repeat(MAX_LOCALIZATION_SOURCE_LINES + 1),
        )
        .unwrap();
    source::take_compilation_count();
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&LocalizationCatalogQuery::default())
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    assert_eq!(
        source::take_compilation_count(),
        0,
        "line overflow must fail before any compiler invocation"
    );
}

#[test]
fn clone_preflight_counts_saved_and_deleted_payload_before_any_compilation() {
    let root = std::env::temp_dir().join("worldline-localization-borrowed-preflight");
    let mut project = Project::new(&root);
    let original = project.document(&project.entry).unwrap().len();
    project.mark_saved();
    project
        .set_text(&project.entry.clone(), "X".into())
        .unwrap();
    let bytes = root.as_os_str().as_encoded_bytes().len()
        + project.entry.as_os_str().as_encoded_bytes().len() * 2
        + original
        + 1;
    source::take_compilation_count();
    assert_eq!(
        limits::clone_envelope_with_limit(&project, bytes).unwrap(),
        bytes
    );
    assert!(limits::clone_envelope_with_limit(&project, bytes - 1).is_err());
    project.delete_document(&project.entry.clone()).unwrap();
    assert_eq!(
        limits::clone_envelope_with_limit(&project, bytes).unwrap(),
        bytes,
        "tombstones keep their current and saved payloads"
    );
    assert_eq!(source::take_compilation_count(), 0);
}

#[test]
fn clone_preflight_rejects_tracked_document_overflow_without_compiling_or_cloning() {
    let root = std::env::temp_dir().join("worldline-localization-document-preflight");
    let mut project = Project::new(&root);
    let document = project.documents.values().next().unwrap().clone();
    for index in 1..MAX_LOCALIZATION_TRACKED_FILES {
        project
            .documents
            .insert(root.join(format!("file_{index}.wl")), document.clone());
    }
    source::take_compilation_count();
    project.check_localization_budget().unwrap();
    project.documents.insert(root.join("overflow.wl"), document);
    let baseline = project.content_baseline();
    assert_eq!(
        project.check_localization_budget().unwrap_err().code,
        "BUDGET_EXCEEDED"
    );
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(source::take_compilation_count(), 0);
}

#[test]
fn projected_manifest_and_sidecar_use_exact_current_saved_path_and_file_budget() {
    let fixture = fixture();
    let project = &fixture.project;
    let manifest_path = crate::workspace_documents::manifest_path(&project.root);
    let sidecar_path = project.root.join(".world/localization/zh.json");
    let manifest = project.authoring_document(&manifest_path).unwrap();
    let replacement = b"replacement manifest bytes";
    let sidecar = b"new locale bytes";
    let updates = [
        limits::AuthoringProjection {
            path: &manifest_path,
            bytes: replacement,
            create: false,
        },
        limits::AuthoringProjection {
            path: &sidecar_path,
            bytes: sidecar,
            create: true,
        },
    ];
    let baseline = project.content_baseline();
    let current = limits::clone_envelope_with_limit(project, usize::MAX).unwrap();
    let expected = current - manifest.bytes().len()
        + replacement.len()
        + sidecar_path.as_os_str().as_encoded_bytes().len()
        + sidecar.len();
    source::take_compilation_count();
    assert_eq!(
        limits::projected_envelope_with_limit(project, &updates, expected).unwrap(),
        expected
    );
    assert!(limits::projected_envelope_with_limit(project, &updates, expected - 1).is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(
        source::take_compilation_count(),
        0,
        "projection only borrows document sizes"
    );
    assert!(project.authoring_document(&sidecar_path).is_err());
}

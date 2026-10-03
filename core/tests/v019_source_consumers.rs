//! 诊断来源修复的非诊断消费者：原始字节重构、正文替换、结构编辑和时间来源。
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::project::Project;
use worldline_core::search_replace::{SearchFile, SearchOptions, SearchRequest, SearchScope};
use worldline_core::source_lifecycle::SourceLifecycleRequest;
use worldline_core::{CompileResult, TargetRef};

const SOURCE: &str = include_str!("../../runtime/tests/v019_fixtures/consumer.wl");
const MANIFEST: &str = r#"{"schema_version":1,"language_version":"1.13","required_features":["content.entities.v1","content.object_refs.v1","content.character_refs.v1"]}"#;
struct Workspace(PathBuf);
impl Workspace {
    fn new(source: &str) -> (Self, Project) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-v019-consumer-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(".world")).unwrap();
        std::fs::write(root.join(".world/project.json"), MANIFEST).unwrap();
        std::fs::write(root.join("world.wl"), source).unwrap();
        let project = Project::open(&root).unwrap();
        (Self(root), project)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn checked(project: &mut Project) -> CompileResult {
    let c = project.compile();
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    c
}
fn expected(kind: &str) -> String {
    match kind {
        "rule" => SOURCE
            .replace("rule ready(", "rule permitted(")
            .replace("{ready(", "{permitted(")
            .replace("if ready(", "if permitted("),
        "tag" => SOURCE
            .replace("tag key as", "tag token as")
            .replace("with key, log", "with token, log")
            .replace("has(inventory, key)", "has(inventory, token)")
            .replace("tag(key)", "tag(token)")
            .replace(r#"tag(\"key\")"#, r#"tag(\"token\")"#),
        "state" => SOURCE
            .replace("state inventory on", "state satchel on")
            .replace("has(inventory,", "has(satchel,")
            .replace("state(inventory)", "state(satchel)")
            .replace(r#"state(\"inventory\")"#, r#"state(\"satchel\")"#),
        "entity" => SOURCE
            .replace("entity ledger kind", "entity archive kind")
            .replace(r#"ref("entity", "ledger")"#, r#"ref("entity", "archive")"#)
            .replace("[[entity:ledger|", "[[entity:archive|"),
        "character" => SOURCE
            .replace("character keeper as", "character guardian as")
            .replace("say keeper ", "say guardian ")
            .replace(
                r#"ref("character", "keeper")"#,
                r#"ref("character", "guardian")"#,
            ),
        _ => unreachable!(),
    }
}

#[test]
fn quoted_rule_tag_state_and_static_ref_renames_touch_only_formal_identity_bytes() {
    for (kind, id, new_id) in [
        ("rule", "ready", "permitted"),
        ("tag", "key", "token"),
        ("state", "inventory", "satchel"),
        ("entity", "ledger", "archive"),
        ("character", "keeper", "guardian"),
    ] {
        let (_ws, mut project) = Workspace::new(SOURCE);
        let original = checked(&mut project);
        let baseline = project.content_baseline();
        let snapshot = project.clone();
        let plan = project
            .plan_rename_target(&TargetRef::new(kind, id), new_id)
            .unwrap();
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(project.document(&project.entry).unwrap(), SOURCE);
        let expected = expected(kind);
        for change in &plan.changes {
            let before = project.document(&change.path).unwrap();
            for occurrence in &change.occurrences {
                let a = &occurrence.before_range;
                let b = &occurrence.after_range;
                assert_eq!(&before[a.start..a.end], occurrence.before_token);
                assert_eq!(&expected[b.start..b.end], occurrence.after_token);
            }
        }
        project.apply_rename_plan(&plan).unwrap();
        assert_eq!(project.document(&project.entry).unwrap(), expected);
        assert_eq!(std::fs::read_to_string(&project.entry).unwrap(), SOURCE);
        let c = checked(&mut project);
        if kind == "entity" {
            assert_eq!(c.analysis.fingerprint, original.analysis.fingerprint);
        }
        let changed = project.content_baseline();
        assert!(project.apply_rename_plan(&plan).is_err());
        assert_eq!(project.content_baseline(), changed);
        assert!(project.restore(snapshot));
        assert_eq!(project.document(&project.entry).unwrap(), SOURCE);
    }
}

fn request(query: &str, replacement: &str) -> SearchRequest {
    SearchRequest {
        query: query.into(),
        replacement: replacement.into(),
        options: SearchOptions::default(),
        scope: SearchScope::Prose,
        files: vec![SearchFile {
            path: "world.wl".into(),
            range: None,
        }],
    }
}

#[test]
fn quoted_prose_replace_protects_expressions_static_refs_links_and_direction_bytes() {
    let (_ws, mut project) = Workspace::new(SOURCE);
    checked(&mut project);
    for protected in ["ready", "inventory", "keeper"] {
        assert!(
            project
                .search_drafts(&request(protected, "changed"), &[])
                .unwrap()
                .is_empty(),
            "{protected}"
        );
    }
    let before = project.clone();
    let baseline = project.content_baseline();
    let plan = project
        .preview_search_replace(&request("中文", "修订中文"), &[])
        .unwrap();
    assert_eq!(plan.hits.len(), 1);
    assert_eq!(project.content_baseline(), baseline);
    project.apply_search_replace(&plan, &[]).unwrap();
    assert_eq!(
        project.document(&project.entry).unwrap(),
        SOURCE.replacen("中文", "修订中文", 1)
    );
    assert_eq!(std::fs::read_to_string(&project.entry).unwrap(), SOURCE);
    checked(&mut project);
    let changed = project.content_baseline();
    assert!(project.apply_search_replace(&plan, &[]).is_err());
    assert_eq!(project.content_baseline(), changed);
    assert!(project.restore(before));
    assert_eq!(project.document(&project.entry).unwrap(), SOURCE);
}

#[test]
fn source_move_preserves_same_semantics_and_points_time_evidence_at_the_new_file() {
    let (_ws, mut project) = Workspace::new("include \"parts/old.wl\"\n");
    let old = project.root.join("parts/old.wl");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, SOURCE).unwrap();
    project = Project::open(&project.root).unwrap();
    let before = checked(&mut project);
    let baseline = project.content_baseline();
    let plan = project
        .preview_source_lifecycle(&SourceLifecycleRequest::Move {
            from: "parts/old.wl".into(),
            to: "章节/新稿.wl".into(),
        })
        .unwrap();
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(
        plan.runtime_fingerprint_before,
        plan.runtime_fingerprint_after
    );
    project.apply_source_lifecycle_plan(&plan).unwrap();
    let after = checked(&mut project);
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    assert_eq!(
        project.document(&project.entry).unwrap(),
        "include \"章节/新稿.wl\"\n"
    );
    let moved = project.root.join("章节/新稿.wl");
    assert_eq!(project.document(&moved).unwrap(), SOURCE);
    assert!(project.document(&old).is_err());
    let comparison = after.analysis.timeline.compare("start", "finish");
    assert_eq!(
        comparison.relation,
        worldline_core::timeline::TemporalRelation::Before
    );
    assert_eq!(comparison.evidence.len(), 1);
    let edge = &comparison.evidence[0];
    assert_eq!(PathBuf::from(&edge.file), moved);
    assert_eq!(
        SOURCE.lines().nth(edge.line as usize - 1).unwrap(),
        "event finish during tide follows start"
    );
    let changed = project.content_baseline();
    assert!(project.apply_source_lifecycle_plan(&plan).is_err());
    assert_eq!(project.content_baseline(), changed);
    assert_eq!(std::fs::read_to_string(old).unwrap(), SOURCE);
}

#[test]
fn structural_event_roundtrip_keeps_quoted_expressions_and_current_time_order() {
    let (_ws, mut project) = Workspace::new(SOURCE);
    let before = checked(&mut project);
    let (path, draft) = project.event_draft("finish").unwrap();
    let original_body = draft.body.clone();
    project
        .edit(|p| p.write_event(&path, Some("finish"), &draft))
        .unwrap();
    let after = checked(&mut project);
    assert_eq!(
        project.document(&path).unwrap(),
        include_str!("v019_fixtures/roundtrip.wl")
    );
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    assert_eq!(
        before.analysis.timeline.compare("start", "finish"),
        after.analysis.timeline.compare("start", "finish")
    );
    let (_, current) = project.event_draft("finish").unwrap();
    assert_eq!(current.body, original_body);
    assert_eq!(current.predecessors, ["start"]);
    let comparison = after.analysis.timeline.compare("start", "finish");
    assert_eq!(
        comparison.relation,
        worldline_core::timeline::TemporalRelation::Before
    );
    let text = project.document(&path).unwrap();
    let before_fragment = SOURCE.split("period tide").next().unwrap();
    assert!(text.starts_with(before_fragment));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SOURCE);
}

#[test]
fn explicit_summary_edit_keeps_the_frozen_v018_fingerprint_contract_and_body() {
    let (_ws, mut project) = Workspace::new(SOURCE);
    let before = checked(&mut project);
    let (path, draft) = project.event_draft("finish").unwrap();
    project
        .edit(|p| p.write_event(&path, Some("finish"), &draft))
        .unwrap();
    let (_, mut draft) = project.event_draft("finish").unwrap();
    let body = draft.body.clone();
    draft.summary = "修订后的结尾".into();
    project
        .edit(|p| p.write_event(&path, Some("finish"), &draft))
        .unwrap();
    let after = checked(&mut project);
    // 真实双core harness已证明：event.summary在0.18合同里参与指纹。
    assert_eq!(before.analysis.fingerprint, 13_563_902_629_068_965_109);
    assert_eq!(after.analysis.fingerprint, 9_011_183_548_374_066_215);
    assert_ne!(before.analysis.fingerprint, after.analysis.fingerprint);
    assert_eq!(
        project.document(&path).unwrap(),
        include_str!("v019_fixtures/summary-edited.wl")
    );
    let (_, current) = project.event_draft("finish").unwrap();
    assert_eq!(current.summary, "修订后的结尾");
    assert_eq!(current.body, body);
    assert_eq!(current.predecessors, ["start"]);
    assert_eq!(
        before.analysis.timeline.compare("start", "finish"),
        after.analysis.timeline.compare("start", "finish")
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SOURCE);
}

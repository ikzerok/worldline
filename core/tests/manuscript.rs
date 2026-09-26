use worldline_core::catalog::TargetRef;
use worldline_core::{
    build_manuscript_index, compile_source_with_options, CompileOptions, ManuscriptReferenceRole,
    ManuscriptReferenceStatus, MANUSCRIPT_REQUIRED_FEATURE,
};

const MANUSCRIPT: &str = r#"{
  "schema_version": 1,
  "id": "novel",
  "title": "雾港",
  "custom_document_field": {"keep": true},
  "entries": [
    {"id":"volume_one","kind":"section","title":"第一卷","custom_node_field":"keep"},
    {"id":"opening","kind":"chapter","parent_id":"volume_one","title":"抵达","target_ref":{"kind":"event","id":"arrival"},"summary":"抵达雾港","pov":{"kind":"character","id":"traveler"},"status":"draft","goal":"建立氛围"},
    {"id":"harbor_scene","kind":"chapter","parent_id":"volume_one","title":"港口","target_ref":{"kind":"scene","id":"arrival.harbor"}},
    {"id":"lighthouse_notes","kind":"chapter","title":"灯塔资料","target_ref":{"kind":"entity","id":"lighthouse"}}
  ]
}"#;

fn content() -> worldline_core::CompileResult {
    let source = concat!(
        "character traveler as \"旅人\"\n",
        "event arrival as \"不计入正文统计的摘要\"\n",
        "  甲乙 🌊 hello-42 [[character:traveler|林澈]]\n",
        "  if visits(\"arrival.harbor\") > 0\n",
        "    分支甲。\n",
        "  scene harbor\n",
        "    灯塔 beacon 7。\n",
        "    -> END\n",
        "  -> END\n",
        "entity lighthouse kind place as \"雾港灯塔\"\n",
        "  description \"雾港资料 alpha 17 🌊\"\n",
    );
    compile_source_with_options("world.wl", source, CompileOptions::v1_10())
}

fn build(
    bytes: &[u8],
    file: &str,
    content: &worldline_core::CompileResult,
) -> worldline_core::ManuscriptIndex {
    let features = vec![MANUSCRIPT_REQUIRED_FEATURE.to_string()];
    build_manuscript_index(bytes, file, "novel", &features, false, content)
}

#[test]
fn read_only_projection_pages_static_sources_and_preserves_unknown_fields() {
    let content = content();
    assert!(!content.has_errors(), "{:?}", content.diagnostics);
    let fingerprint = content.analysis.fingerprint;
    let index = build(
        MANUSCRIPT.as_bytes(),
        ".world/manuscripts/novel.json",
        &content,
    );

    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(index.id.as_deref(), Some("novel"));
    assert_eq!(index.source_bytes(), MANUSCRIPT.as_bytes());
    assert_eq!(
        index.source_document().unwrap()["custom_document_field"]["keep"],
        true
    );
    assert_eq!(
        index.source_document().unwrap()["entries"][0]["custom_node_field"],
        "keep"
    );

    let first = index.page(0, 2);
    assert_eq!(first.total, 3);
    assert_eq!(first.chapters.len(), 2);
    assert_eq!(first.chapters[0].id, "opening");
    assert_eq!(first.chapters[0].section_path, ["volume_one"]);
    assert_eq!(first.chapters[0].summary.as_deref(), Some("抵达雾港"));
    assert_eq!(first.chapters[0].status.as_deref(), Some("draft"));
    assert_eq!(
        first.chapters[0].source.as_ref().unwrap().status,
        ManuscriptReferenceStatus::Resolved
    );
    assert_eq!(
        first.chapters[0]
            .source
            .as_ref()
            .unwrap()
            .stats
            .unwrap()
            .han_characters,
        9
    );
    assert_eq!(
        first.chapters[0]
            .source
            .as_ref()
            .unwrap()
            .stats
            .unwrap()
            .words,
        13
    );
    assert_eq!(
        first.chapters[0]
            .source
            .as_ref()
            .unwrap()
            .location
            .as_ref()
            .unwrap()
            .file,
        "world.wl"
    );
    assert_eq!(first.chapters[1].id, "harbor_scene");
    assert_eq!(
        first.chapters[1]
            .source
            .as_ref()
            .unwrap()
            .stats
            .unwrap()
            .han_characters,
        2
    );
    assert_eq!(first.next_offset, Some(2));

    let second = index.page(first.next_offset.unwrap(), 2);
    assert_eq!(second.chapters.len(), 1);
    assert_eq!(second.chapters[0].id, "lighthouse_notes");
    assert_eq!(
        second.chapters[0]
            .source
            .as_ref()
            .unwrap()
            .stats
            .unwrap()
            .han_characters,
        4
    );
    assert_eq!(second.next_offset, None);

    let source_refs = index.references_to(&TargetRef::new("event", "arrival"));
    assert_eq!(source_refs.len(), 2);
    assert_eq!(source_refs[0].chapter_id, "opening");
    assert_eq!(source_refs[0].role, ManuscriptReferenceRole::Source);
    assert_eq!(source_refs[1].chapter_id, "harbor_scene");
    assert_eq!(
        source_refs[1].target,
        TargetRef::new("scene", "arrival.harbor")
    );
    let pov_refs = index.references_to(&TargetRef::new("character", "traveler"));
    assert_eq!(pov_refs.len(), 1);
    assert_eq!(pov_refs[0].role, ManuscriptReferenceRole::Perspective);

    let mut reordered_document: serde_json::Value =
        serde_json::from_str(MANUSCRIPT).expect("书稿夹具必须有效");
    reordered_document["entries"]
        .as_array_mut()
        .unwrap()
        .swap(1, 2);
    let reordered_bytes = serde_json::to_vec(&reordered_document).unwrap();
    let reordered = build(&reordered_bytes, ".world/manuscripts/novel.json", &content);
    assert_eq!(reordered.page(0, 1).chapters[0].id, "harbor_scene");
    assert_eq!(content.analysis.fingerprint, fingerprint);
}

#[test]
fn absent_targets_are_distinguished_from_unresolved_content_and_chapters_remain() {
    let good_content = content();
    let missing = r#"{"schema_version":1,"id":"novel","title":"雾港","entries":[{"id":"lost","kind":"chapter","title":"失落章节","target_ref":{"kind":"event","id":"not_here"}}]}"#;
    let index = build(missing.as_bytes(), "book.json", &good_content);
    assert_eq!(index.entries.len(), 1);
    assert_eq!(index.page(0, 10).chapters[0].id, "lost");
    assert_eq!(
        index.page(0, 10).chapters[0]
            .source
            .as_ref()
            .unwrap()
            .status,
        ManuscriptReferenceStatus::Missing
    );
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAN003"));

    let broken_content = compile_source_with_options(
        "world.wl",
        "event source\n  -> missing_event\n",
        CompileOptions::v1_9(),
    );
    assert!(broken_content.has_errors());
    let unresolved = build(missing.as_bytes(), "book.json", &broken_content);
    assert_eq!(
        unresolved.page(0, 10).chapters[0]
            .source
            .as_ref()
            .unwrap()
            .status,
        ManuscriptReferenceStatus::Unresolved
    );
    assert!(unresolved
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAN004"));
}

#[test]
fn invalid_display_titles_do_not_hide_identifiable_chapters() {
    let content = content();
    let bytes = r#"{"schema_version":1,"id":"novel","title":null,"entries":[
      {"id":"opening","kind":"chapter","title":42,"target_ref":{"kind":"event","id":"arrival"}}
    ]}"#;
    let index = build(bytes.as_bytes(), "book.json", &content);
    assert!(index.diagnostics.iter().any(|item| item.code == "MAN001"));
    assert_eq!(index.title.as_deref(), Some("novel"));
    assert_eq!(index.page(0, 10).chapters[0].id, "opening");
    assert_eq!(index.page(0, 10).chapters[0].title, "opening");
    assert_eq!(index.source_bytes(), bytes.as_bytes());
}

#[test]
fn deeply_nested_sections_project_without_recursive_stack_growth() {
    let content = content();
    let mut entries = Vec::new();
    for depth in 0..4096 {
        entries.push(serde_json::json!({
            "id": format!("section_{depth}"),
            "kind": "section",
            "title": format!("卷 {depth}"),
            "parent_id": (depth > 0).then(|| format!("section_{}", depth - 1)),
        }));
    }
    entries.push(serde_json::json!({
        "id": "deep_chapter",
        "kind": "chapter",
        "title": "最深章节",
        "parent_id": "section_4095",
        "target_ref": {"kind": "event", "id": "arrival"}
    }));
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1, "id": "novel", "title": "雾港", "entries": entries
    }))
    .unwrap();
    let index = build(&bytes, "book.json", &content);
    let page = index.page(0, 1);
    assert_eq!(page.total, 1);
    assert_eq!(page.chapters[0].section_path.len(), 4096);
}

#[test]
fn invalid_ids_cycles_and_source_kinds_are_diagnosed_without_dropping_chapters() {
    let content = content();
    let invalid = r#"{"schema_version":1,"id":"novel","title":"雾港","entries":[
      {"id":"loop_a","kind":"section","parent_id":"loop_b","title":"A"},
      {"id":"loop_b","kind":"section","parent_id":"loop_a","title":"B"},
      {"id":"same","kind":"chapter","title":"第一章","target_ref":{"kind":"event","id":"arrival"}},
      {"id":"same","kind":"chapter","title":"重复章","target_ref":{"kind":"file","id":"notes"}}
    ]}"#;
    let index = build(invalid.as_bytes(), "book.json", &content);
    assert_eq!(index.entries.len(), 4);
    assert_eq!(index.page(0, 10).chapters.len(), 2);
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAN005"));
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAN007"));
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAN008"));
    assert_eq!(
        index.page(0, 10).chapters[1]
            .source
            .as_ref()
            .unwrap()
            .status,
        ManuscriptReferenceStatus::Invalid
    );
}

#[test]
fn unsupported_document_version_is_read_only_and_keeps_original_bytes() {
    let content = content();
    let bytes = r#"{"schema_version":2,"id":"novel","title":"雾港","entries":[],"future":true}"#;
    let index = build(bytes.as_bytes(), "book.json", &content);
    assert!(index.read_only);
    assert_eq!(index.source_bytes(), bytes.as_bytes());
    assert_eq!(index.source_document().unwrap()["future"], true);
    assert!(index.entries.is_empty());
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAN002"));
}

#[test]
fn manuscript_feature_must_be_declared_before_projection() {
    let content = content();
    let missing_feature: Vec<String> = Vec::new();
    let index = build_manuscript_index(
        MANUSCRIPT.as_bytes(),
        "book.json",
        "novel",
        &missing_feature,
        false,
        &content,
    );
    assert!(index.read_only);
    assert_eq!(index.entries.len(), 0);
    assert_eq!(index.source_bytes(), MANUSCRIPT.as_bytes());
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAN002"));
}

#[test]
fn large_manuscripts_keep_projection_pages_within_the_declared_budget() {
    let entries = (0..157)
        .map(|index| {
            serde_json::json!({
                "id": format!("chapter_{index}"),
                "kind": "chapter",
                "title": format!("第 {index} 章"),
                "target_ref": {"kind":"event", "id":"arrival"}
            })
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "id": "novel",
        "title": "雾港",
        "entries": entries
    }))
    .unwrap();
    let content = content();
    let index = build(&bytes, "book.json", &content);

    let first = index.page(0, 1000);
    assert_eq!(first.limit, 100);
    assert_eq!(first.total, 157);
    assert_eq!(first.chapters.len(), 100);
    assert_eq!(first.next_offset, Some(100));
    let last = index.page(first.next_offset.unwrap(), 1000);
    assert_eq!(last.chapters.len(), 57);
    assert_eq!(last.chapters[0].id, "chapter_100");
    assert_eq!(last.next_offset, None);
}

#[test]
fn distinct_scene_references_project_independently() {
    let compiled = compile_source_with_options(
        "world.wl",
        concat!(
            "event arrival\n",
            "  scene harbor\n",
            "    潮汐🌊。\n",
            "  scene market\n",
            "    市集 goods-9。\n",
            "  -> END\n",
        ),
        CompileOptions::v1_10(),
    );
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let bytes = r#"{"schema_version":1,"id":"novel","title":"雾港","entries":[
      {"id":"harbor","kind":"chapter","title":"港口","target_ref":{"kind":"scene","id":"arrival.harbor"}},
      {"id":"market","kind":"chapter","title":"集市","target_ref":{"kind":"scene","id":"arrival.market"}}
    ]}"#;
    let index = build(bytes.as_bytes(), "book.json", &compiled);

    let page = index.page(0, 10);
    assert_eq!(page.chapters.len(), 2);
    assert_eq!(
        page.chapters[0].source.as_ref().unwrap().status,
        ManuscriptReferenceStatus::Resolved
    );
    assert_eq!(
        page.chapters[0]
            .source
            .as_ref()
            .unwrap()
            .stats
            .unwrap()
            .han_characters,
        2
    );
    assert_eq!(
        page.chapters[1].source.as_ref().unwrap().status,
        ManuscriptReferenceStatus::Resolved
    );
    assert_eq!(
        page.chapters[1]
            .source
            .as_ref()
            .unwrap()
            .stats
            .unwrap()
            .han_characters,
        2
    );
}

use super::*;
use crate::{compile_source, compile_source_with_options, CompileOptions};

fn compiled(text: &str) -> CompileResult {
    let result = compile_source_with_options(
        "故事.wl",
        text,
        CompileOptions::v1_13()
            .with_object_refs(true)
            .with_character_refs(true),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}
fn review(result: &CompileResult) -> ReviewProjection {
    review_projection(result, &TargetRef::new("event", "start")).unwrap()
}
fn flatten(nodes: &[ReviewNode]) -> Vec<&ReviewNode> {
    let mut output = Vec::new();
    for node in nodes {
        output.push(node);
        output.extend(flatten(&node.children));
    }
    output
}

#[test]
fn nested_alternatives_preserve_real_headers_boundaries_and_utf8() {
    let source = "let gate = true\nevent start\n  if gate\n    甲😀。~\n    乙。\n    if false\n      内层。\n    else\n      另一内层。\n  // 中文移行不猜位置\n  else if not gate\n    备选。\n  else\n    尾选。\n  汇合。\n  -> END\n";
    let result = compiled(source);
    let projection = review(&result);
    let condition = projection
        .nodes
        .iter()
        .find(|node| node.kind == ReviewKind::If)
        .unwrap();
    assert_eq!(condition.children.len(), 3);
    assert_eq!(condition.children[1].label, "否则如果（else if）");
    assert_eq!(condition.children[1].condition.as_deref(), Some("not gate"));
    assert_eq!(condition.children[1].source.as_ref().unwrap().line, 11);
    assert_eq!(condition.children[2].source.as_ref().unwrap().line, 13);
    assert_eq!(condition.children[0].children[0].kind, ReviewKind::Text);
    assert!(condition.children[0].children[0].glue);
    assert_eq!(condition.children[0].children[2].kind, ReviewKind::If);
    assert!(condition.end_label.is_some());
    for node in flatten(&projection.nodes) {
        if let Some(location) = &node.source {
            assert_eq!(
                source.get(location.byte_start..location.byte_end),
                Some(location.excerpt.as_str())
            );
            validate_review_source(&result, &projection, location).unwrap();
        }
    }
}

#[test]
fn choices_remain_one_group_across_extracted_effects_and_empty_choice() {
    let result = compiled("character lin as \"林\"\nevent start\n  choice once \"甲\" if true enable false disabled \"没钥匙\"\n    甲正文。\n  effect on enter\n    meet lin\n  choice \"空选项\"\n  继续。\n  -> END\n");
    let projection = review(&result);
    let groups = projection
        .nodes
        .iter()
        .filter(|node| node.kind == ReviewKind::ChoiceGroup)
        .collect::<Vec<_>>();
    assert_eq!(groups.len(), 1);
    let choices = &groups[0].children;
    assert_eq!(choices.len(), 2);
    assert!(choices[0].once);
    assert_eq!(choices[0].condition.as_deref(), Some("true"));
    assert_eq!(choices[0].enable.as_deref(), Some("false"));
    assert_eq!(choices[0].disabled_reason.as_deref(), Some("没钥匙"));
    assert!(choices[1].children.is_empty());
    assert!(projection
        .nodes
        .iter()
        .any(|node| node.label.contains("事件效果声明")));
}

#[test]
fn same_display_names_keep_identity_links_glue_and_unexpanded_calls() {
    let result = compiled("character first as \"同名\"\ncharacter second as \"同名\"\nfragment reply()\n  say first \"只在片段里\"\n  return\nevent start\n  say first \"你好😀 {rnd(1, 2)} [[character:second|朋友]]\"\n  say second \"另一人\"\n  call reply()\n  -> END\n");
    let projection = review(&result);
    let nodes = flatten(&projection.nodes);
    let says = nodes
        .iter()
        .filter(|node| node.kind == ReviewKind::Say)
        .collect::<Vec<_>>();
    assert_eq!(says.len(), 2);
    assert_eq!(says[0].speaker.as_ref().unwrap().display, "同名");
    assert_eq!(says[0].speaker.as_ref().unwrap().target.id, "first");
    assert_eq!(says[1].speaker.as_ref().unwrap().target.id, "second");
    assert!(says[0].parts.iter().any(|part| part.dynamic));
    assert!(says[0]
        .parts
        .iter()
        .any(|part| part.target == Some(TargetRef::new("character", "second"))));
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node.kind == ReviewKind::Call)
            .count(),
        1
    );
    assert!(!nodes
        .iter()
        .flat_map(|node| &node.parts)
        .any(|part| part.text.contains("只在片段")));
    let fragment = review_projection(&result, &TargetRef::new("fragment", "reply")).unwrap();
    assert!(flatten(&fragment.nodes)
        .iter()
        .any(|node| node.kind == ReviewKind::Return));
}

#[test]
fn divert_uses_resolved_local_and_cross_event_scene_identity() {
    let result = compiled("fragment jump()\n  -> other.room\nevent start\n  -> room\n  scene room\n    call jump()\n    -> END\nevent other\n  scene room\n    -> END\n");
    let projection = review(&result);
    let divert = flatten(&projection.nodes)
        .into_iter()
        .find(|node| node.kind == ReviewKind::Divert)
        .unwrap();
    assert_eq!(divert.target, Some(TargetRef::new("scene", "start.room")));
    let fragment = review_projection(&result, &TargetRef::new("fragment", "jump")).unwrap();
    let divert = flatten(&fragment.nodes)
        .into_iter()
        .find(|node| node.kind == ReviewKind::Divert)
        .unwrap();
    assert_eq!(divert.target, Some(TargetRef::new("scene", "other.room")));
}

#[test]
fn full_snapshot_revalidation_rejects_comments_bad_drafts_and_forged_locations() {
    let source = "event start\n  正文😀。\n  -> END\n";
    let result = compiled(source);
    let projection = review(&result);
    let location = projection.nodes[1].source.as_ref().unwrap();
    let changed = compiled(&format!("// 新行\n{source}"));
    assert_eq!(
        validate_review_source(&changed, &projection, location)
            .map(|_| ())
            .expect_err("应整体拒绝该审稿或来源")
            .code,
        "stale_review"
    );
    let bad = compile_source("故事.wl", "event start\n  if (\n");
    assert!(review_projection(&bad, &projection.target).is_err());
    assert!(validate_review_source(&bad, &projection, location).is_err());
    let mut forged = location.clone();
    forged.byte_start += 1;
    assert!(validate_review_source(&result, &projection, &forged).is_err());
}

#[test]
fn shared_snapshot_does_not_copy_source_for_each_chapter() {
    let result = compiled("event start\n  第一章。\n  -> END\nevent next\n  第二章。\n  -> END\n");
    let snapshot = ReviewSnapshot::new(&result).unwrap();
    let chapters = (0..100)
        .map(|index| {
            review_projection_with_snapshot(
                &result,
                &TargetRef::new("event", if index % 2 == 0 { "start" } else { "next" }),
                &snapshot,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(chapters
        .iter()
        .all(|review| std::sync::Arc::ptr_eq(&review.sources, &snapshot.sources)));
    assert_eq!(std::sync::Arc::strong_count(&snapshot.sources), 101);
    assert!(std::sync::Arc::ptr_eq(
        &chapters[0].sources,
        &chapters[0].clone().sources
    ));
}

#[test]
fn output_budgets_cover_metadata_escaping_and_node_count_without_truncation() {
    let huge_display = "名".repeat(200_000);
    let result = compiled(&format!(
        "character lin as \"{huge_display}\"\nevent start\n{}  -> END\n",
        "  say lin \"短句\"\n".repeat(40)
    ));
    assert_eq!(
        review_projection(&result, &TargetRef::new("event", "start"))
            .map(|_| ())
            .expect_err("应整体拒绝该审稿或来源")
            .code,
        "review_limit"
    );
    let source = format!("event start\n{}  -> END\n", "  文。\n".repeat(10_001));
    let result = compiled(&source);
    assert_eq!(
        review_projection(&result, &TargetRef::new("event", "start"))
            .map(|_| ())
            .expect_err("应整体拒绝该审稿或来源")
            .code,
        "review_limit"
    );
    let below = compiled(&format!(
        "event start\n  {}\n  -> END\n",
        "\\t".repeat(200_000)
    ));
    let actual_bytes = serde_json::to_vec(&review(&below)).unwrap().len();
    assert!(actual_bytes > 1_000_000 && actual_bytes <= MAX_REVIEW_JSON_BYTES);
    let escaped = compiled(&format!(
        "event start\n  {}\n  -> END\n",
        "\\t".repeat(250_000)
    ));
    assert_eq!(
        review_projection(&escaped, &TargetRef::new("event", "start"))
            .map(|_| ())
            .expect_err("应整体拒绝该审稿或来源")
            .code,
        "review_limit"
    );
}

#[test]
fn draft_projection_reads_unapplied_buffers_without_changing_project_or_fingerprint() {
    let mut project =
        crate::project::Project::new(&std::env::temp_dir().join("review-private-draft"));
    let path = project.entry.clone();
    project.documents.retain(|entry, _| entry == &path);
    project
        .set_text(&path, "event start\n  原文。\n  -> END\n".into())
        .unwrap();
    let baseline = project.content_baseline();
    let fingerprint = project.compile_current().analysis.fingerprint;
    let mut buffer = project.open_source_writing_buffer(&path).unwrap();
    buffer.replace_source(
        buffer
            .source()
            .replace("原文。", "未应用正文 {rnd(1, 2)}。"),
    );
    let result = project
        .compile_writing_drafts(std::slice::from_ref(&buffer))
        .unwrap();
    let projection = review(&result);
    assert!(flatten(&projection.nodes)
        .iter()
        .flat_map(|node| &node.parts)
        .any(|part| part.text.contains("未应用正文")));
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.compile_current().analysis.fingerprint, fingerprint);
    assert!(project.document(&path).unwrap().contains("原文。"));
    let unchanged = project.compile_current();
    assert!(validate_review_source(
        &unchanged,
        &projection,
        projection.nodes[1].source.as_ref().unwrap()
    )
    .is_err());
}

#[test]
fn event_entry_after_and_legacy_permission_keep_actual_declaration() {
    for source in [
        "let gate = true\nevent start after gate\n  -> END\n",
        "event start perm key\n  -> END\n",
        "event start perm key after false or perm(key)\n  -> END\n",
    ] {
        let result = compiled(source);
        let projection = review(&result);
        let header = &projection.nodes[0];
        assert_eq!(header.kind, ReviewKind::Structure);
        assert!(header.label.contains("入口约束未求值"));
        let expected = source
            .lines()
            .find(|line| line.starts_with("event "))
            .unwrap();
        assert_eq!(header.source.as_ref().unwrap().excerpt, expected);
    }
}

#[test]
fn cross_file_full_snapshot_and_current_memory_overrides_are_bound() {
    let root = std::env::temp_dir().join("review-cross-file-sources");
    let entry = root.join("world.wl");
    let people = root.join("people.wl");
    let mut sources = BTreeMap::from([
        (
            entry.clone(),
            "event start\n  say lin \"正文\"\n  -> END\n".into(),
        ),
        (people.clone(), "character lin as \"旧名字\"\n".into()),
    ]);
    let options = CompileOptions::v1_13();
    let original = crate::compile_sources_with_options(&entry, &sources, options);
    assert!(!original.has_errors(), "{:?}", original.diagnostics);
    let before = review(&original);
    sources.insert(
        people,
        "// 人物改稿\ncharacter lin as \"未应用名字\"\n".into(),
    );
    sources.insert(
        entry.clone(),
        "event start\n  say lin \"未应用正文\"\n  -> END\n".into(),
    );
    let current = crate::compile_sources_with_options(&entry, &sources, options);
    assert!(!current.has_errors(), "{:?}", current.diagnostics);
    let projection = review(&current);
    let say = flatten(&projection.nodes)
        .into_iter()
        .find(|node| node.kind == ReviewKind::Say)
        .unwrap();
    assert_eq!(say.speaker.as_ref().unwrap().display, "未应用名字");
    assert_eq!(say.parts[0].text, "未应用正文");
    assert_eq!(say.source.as_ref().unwrap().file, entry.to_string_lossy());
    assert!(
        validate_review_source(&current, &before, before.nodes[1].source.as_ref().unwrap())
            .is_err()
    );
    // 只改另一文件的注释，也必须让旧来源失效。
    let mut changed = current.sources.clone();
    let last = changed.keys().find(|path| **path != entry).unwrap().clone();
    changed
        .get_mut(&last)
        .unwrap()
        .push_str("// another snapshot\n");
    let new_result = crate::compile_sources_with_options(&entry, &changed, options);
    assert!(
        validate_review_source(&new_result, &projection, say.source.as_ref().unwrap()).is_err()
    );
}

#[test]
fn empty_invalid_branch_and_missing_provenance_never_claim_complete() {
    let result = compile_source(
        "draft.wl",
        "event start\n  if true\n  else\n    文。\n  -> END\n",
    );
    assert!(result.has_errors());
    assert!(review_projection(&result, &TargetRef::new("event", "start")).is_err());
    let mut result = compiled("event start\n  文。\n  -> END\n");
    result.program.source_provenance.statement_origins.clear();
    assert_eq!(
        review_projection(&result, &TargetRef::new("event", "start"))
            .map(|_| ())
            .expect_err("应整体拒绝该审稿或来源")
            .code,
        "source_unavailable"
    );
}

#[test]
fn depth_source_input_and_long_file_names_are_bounded() {
    let mut source = String::from("event start\n");
    for depth in 0..34 {
        source.push_str(&format!("{}if true\n", "  ".repeat(depth + 1)));
    }
    source.push_str(&format!("{}文。\n  -> END\n", "  ".repeat(35)));
    let result = compiled(&source);
    assert_eq!(
        review_projection(&result, &TargetRef::new("event", "start"))
            .map(|_| ())
            .expect_err("应整体拒绝该审稿或来源")
            .code,
        "review_limit"
    );
    let path = format!("{}.wl", "长".repeat(2000));
    let source = format!("event start\n{}  -> END\n", "  文。\n".repeat(200));
    let result = compile_source(&path, &source);
    assert!(!result.has_errors());
    assert_eq!(
        review_projection(&result, &TargetRef::new("event", "start"))
            .map(|_| ())
            .expect_err("应整体拒绝该审稿或来源")
            .code,
        "review_limit"
    );
    let mut result = compiled("event start\n  -> END\n");
    result
        .sources
        .insert(PathBuf::from("large.wl"), " ".repeat(MAX_SOURCE_BYTES + 1));
    assert_eq!(
        review_projection(&result, &TargetRef::new("event", "start"))
            .map(|_| ())
            .expect_err("应整体拒绝该审稿或来源")
            .code,
        "review_limit"
    );
}

#[test]
fn json_text_round_trip_preserves_quotes_newlines_unicode_without_markup_execution() {
    let result = compiled(
        "event start\n  <script>你好😀</script> \\\"引号\\\" \\n下一行\\t末尾\n  -> END\n",
    );
    let projection = review(&result);
    let encoded = serde_json::to_vec(&projection).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    let text = value["nodes"][1]["parts"][0]["text"].as_str().unwrap();
    assert_eq!(text, "<script>你好😀</script> \"引号\" \n下一行\t末尾");
    assert!(encoded.len() <= MAX_REVIEW_JSON_BYTES);
}

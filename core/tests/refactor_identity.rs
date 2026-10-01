//! v0.13：身份范围、真实逐处预览和不可拆分计划的最小正式回归。
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::catalog::TargetRef;
use worldline_core::project::Project;

const SOURCE: &str = r#"entity ledger kind item as "entity ledger 中文 \"原样\"" // entity ledger 注释
  property plain = "ref(\"entity\", \"ledger\") entity ledger [[entity:ledger|普通字符串]]"
entity ledger_copy kind item as "相似 ID"
entity other kind item as "另一个"
entity ref_owner kind item
  property held = (ref("entity", "ledger")) // ref("entity", "ledger")
  property edge = ref("relation", "edge")
relation_type owns as "所属"
relation_def edge type owns from entity ledger to entity ledger // relation_def edge entity ledger
  source_note "relation edge / entity ledger"
  scope_ref entity ledger
relation_def edge_copy type owns from entity other to entity ledger_copy
alias entity ledger as "entity ledger \"别名\"" // entity ledger
alias relation edge as "relation edge"
tag important as "标记"
mark entity ledger with important // entity ledger
mark relation edge with important // relation edge
anchor_def clue as "线索"
anchor_link clue entity ledger
/* 块注释 entity ledger
   alias entity ledger as "注释"
*/
alias /* 中文🙂 */ entity ledger as "ledger entity ledger"
event start
  中文 [[entity:ledger|entity ledger 中文]] 与 [[relation:edge|relation edge]]，ledger_copy
  choice "前缀\t [[entity:ledger|entity ledger 中文]] 后缀\"原样\""
    -> END
"#;

fn project() -> Project {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "wl-identity-spans-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.documents.retain(|path, _| path == &entry);
    project.set_text(&entry, SOURCE.into()).unwrap();
    project
        .create_authoring_document(
            &root.join(".world/project.json"),
            br#"{
        "schema_version":1,"language_version":"1.13",
        "required_features":["content.entities.v1","content.relations.v1","content.object_refs.v1"]
    }"#
            .to_vec(),
        )
        .unwrap();
    let compiled = project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    project
}

fn verify_real_projection(project: &Project, plan: &worldline_core::refactor::RenamePlan) {
    let mut candidate = project.clone();
    candidate.apply_rename_plan(plan).unwrap();
    for change in &plan.changes {
        let before = if change.kind == "source" {
            project.document(&change.path).unwrap().as_bytes().to_vec()
        } else {
            project
                .authoring_document(&change.path)
                .unwrap()
                .bytes()
                .to_vec()
        };
        let after = if change.kind == "source" {
            candidate
                .document(&change.path)
                .unwrap()
                .as_bytes()
                .to_vec()
        } else {
            candidate
                .authoring_document(&change.path)
                .unwrap()
                .bytes()
                .to_vec()
        };
        let mut reconstructed = before.clone();
        for occurrence in change.occurrences.iter().rev() {
            let old = &occurrence.before_range;
            let new = &occurrence.after_range;
            assert_eq!(
                &before[old.start..old.end],
                occurrence.before_token.as_bytes()
            );
            assert_eq!(
                &after[new.start..new.end],
                occurrence.after_token.as_bytes()
            );
            assert!(occurrence.before_context.contains(&occurrence.before_token));
            assert!(occurrence.after_context.contains(&occurrence.after_token));
            assert_eq!(
                occurrence.line,
                before[..old.start].iter().filter(|b| **b == b'\n').count() as u32 + 1
            );
            reconstructed.splice(old.start..old.end, occurrence.after_token.bytes());
        }
        assert_eq!(reconstructed, after, "预览应准确重建全部候选字节");
        if change
            .occurrences
            .iter()
            .all(|item| item.field.as_deref() != Some("source.syntax"))
        {
            assert_eq!(change.reference_count, change.occurrences.len());
        }
    }
}

#[test]
fn entity_tokens_preserve_every_nonidentity_byte_and_link_label() {
    let mut project = project();
    let baseline = project.content_baseline();
    let plan = project
        .plan_rename_target(&TargetRef::new("entity", "ledger"), "flood_ledger")
        .unwrap();
    assert_eq!(
        project.content_baseline(),
        baseline,
        "生成及取消预览不应写入"
    );
    assert_eq!(
        plan.runtime_fingerprint_before,
        plan.runtime_fingerprint_after
    );
    verify_real_projection(&project, &plan);
    let occurrences = &plan
        .changes
        .iter()
        .find(|c| c.kind == "source")
        .unwrap()
        .occurrences;
    assert!(occurrences
        .iter()
        .all(|o| o.before_token == "ledger" && o.after_token == "flood_ledger"));
    for field in [
        "declaration.id",
        "property.held.ref.id",
        "from.id",
        "to.id",
        "alias.target.id",
        "mark.target.id",
        "anchor_link.target.id",
        "scope_ref.id",
        "text.link.id",
        "choice.link.id",
    ] {
        assert!(
            occurrences
                .iter()
                .any(|o| o.field.as_deref() == Some(field)),
            "{field}"
        );
    }
    project.apply_rename_plan(&plan).unwrap();
    let after = project.document(&project.entry).unwrap();
    for protected in [
        r#"as "entity ledger 中文 \"原样\"" // entity ledger 注释"#,
        r#"property plain = "ref(\"entity\", \"ledger\") entity ledger [[entity:ledger|普通字符串]]""#,
        r#"// ref("entity", "ledger")"#,
        r#"source_note "relation edge / entity ledger""#,
        r#"as "entity ledger \"别名\"" // entity ledger"#,
        r#"/* 块注释 entity ledger
   alias entity ledger as "注释"
*/"#,
        r#"/* 中文🙂 */ entity flood_ledger as "ledger entity ledger""#,
        r#"[[entity:flood_ledger|entity ledger 中文]]"#,
        r#"后缀\"原样\""#,
        "entity ledger_copy kind item",
    ] {
        assert!(after.contains(protected), "被破坏的非身份字节：{protected}");
    }
}

#[test]
fn relation_tokens_preserve_alias_comment_and_ordinary_strings() {
    let mut project = project();
    let plan = project
        .plan_rename_target(&TargetRef::new("relation", "edge"), "edge_new")
        .unwrap();
    verify_real_projection(&project, &plan);
    assert!(plan
        .changes
        .iter()
        .flat_map(|c| &c.occurrences)
        .all(|o| o.before_token == "edge"));
    project.apply_rename_plan(&plan).unwrap();
    let after = project.document(&project.entry).unwrap();
    assert!(after.contains("relation_def edge_new type owns"));
    assert!(after.contains("// relation_def edge entity ledger"));
    assert!(after.contains(r#"alias relation edge_new as "relation edge""#));
    assert!(after.contains("mark relation edge_new with important // relation edge"));
    assert!(after.contains("[[relation:edge_new|relation edge]]"));
    assert!(after.contains("relation_def edge_copy type owns"));
    assert!(after.contains(r#"property edge = ref("relation", "edge_new")"#));
}

#[test]
fn state_owner_refusal_reports_exact_states_fingerprints_and_safe_alternative() {
    let mut project = project();
    let entry = project.entry.clone();
    project.set_text(&entry, format!("state condition on entity ledger with important as \"账册\"\nstate seal on entity \"ledger\" with []\n{SOURCE}")).unwrap();
    let before = project.compile();
    let baseline = project.content_baseline();
    let spans = worldline_core::lexer::identity_source_spans(
        "world.wl",
        project.document(&entry).unwrap(),
        worldline_core::CompileOptions::v1_13(),
    );
    assert_eq!(
        spans
            .iter()
            .filter(
                |s| s.field == "state.target.id" && s.target == TargetRef::new("entity", "ledger")
            )
            .count(),
        2
    );
    let error = project
        .plan_rename_target(&TargetRef::new("entity", "ledger"), "flood_ledger")
        .unwrap_err();
    for required in [
        "state condition",
        "state seal",
        "world.wl:1",
        "world.wl:2",
        "旧 fingerprint=",
        "候选 fingerprint=",
        "save",
        "replay",
        "as \"显示名\"",
    ] {
        assert!(error.contains(required), "{required}: {error}");
    }
    assert!(error.contains(&before.analysis.fingerprint.to_string()));
    let mut candidate = project.clone();
    let mut candidate_source = candidate.document(&entry).unwrap().to_string();
    for span in spans
        .into_iter()
        .rev()
        .filter(|span| span.target == TargetRef::new("entity", "ledger"))
    {
        candidate_source.replace_range(span.range, "flood_ledger");
    }
    candidate.set_text(&entry, candidate_source).unwrap();
    let candidate_fingerprint = candidate.compile().analysis.fingerprint;
    assert_ne!(candidate_fingerprint, before.analysis.fingerprint);
    assert!(error.contains(&format!("候选 fingerprint={candidate_fingerprint}")));

    assert_eq!(project.content_baseline(), baseline);
    let changed_display = project.document(&entry).unwrap().replacen(
        r#"as "entity ledger 中文 \"原样\"""#,
        r#"as "新的中文名称""#,
        1,
    );
    project.set_text(&entry, changed_display).unwrap();
    assert_eq!(
        project.compile().analysis.fingerprint,
        before.analysis.fingerprint
    );
}

#[test]
fn plan_public_fields_are_bound_and_legacy_machine_fields_remain_readable() {
    let mut project = project();
    let plan = project
        .plan_rename_target(&TargetRef::new("entity", "ledger"), "flood_ledger")
        .unwrap();
    let json = serde_json::to_value(&plan).unwrap();
    for key in [
        "target",
        "new_id",
        "content_baseline",
        "changes",
        "explicit_references",
    ] {
        assert!(json.get(key).is_some());
    }
    for key in ["path", "kind", "reference_count"] {
        assert!(json["changes"][0].get(key).is_some());
    }
    assert!(json["changes"][0].get("before").is_none());
    assert!(json["changes"][0].get("after").is_none());
    #[derive(serde::Deserialize)]
    struct Legacy {
        target: TargetRef,
        new_id: String,
        content_baseline: String,
        changes: Vec<LegacyChange>,
        explicit_references: usize,
    }
    #[derive(serde::Deserialize)]
    struct LegacyChange {
        path: std::path::PathBuf,
        kind: String,
        reference_count: usize,
    }
    let old: Legacy = serde_json::from_value(json).unwrap();
    assert_eq!(old.target, plan.target);
    assert_eq!(old.new_id, plan.new_id);
    assert_eq!(old.content_baseline, plan.content_baseline);
    assert_eq!(old.explicit_references, plan.explicit_references);
    assert_eq!(old.changes[0].path, plan.changes[0].path);
    assert_eq!(old.changes[0].kind, "source");
    assert_eq!(
        old.changes[0].reference_count,
        plan.changes[0].reference_count
    );
    let baseline = project.content_baseline();
    let mut variants = Vec::new();
    let mut altered = plan.clone();
    altered.changes.clear();
    variants.push(altered);
    let mut altered = plan.clone();
    altered.changes[0].occurrences[0]
        .before_context
        .push_str("伪造");
    variants.push(altered);
    let mut altered = plan.clone();
    altered.changes[0].occurrences[0].after_token = "other".into();
    variants.push(altered);
    let mut altered = plan.clone();
    altered.explicit_references += 1;
    variants.push(altered);
    let mut altered = plan.clone();
    altered.runtime_fingerprint_after += 1;
    variants.push(altered);
    let mut altered = plan.clone();
    altered.new_id = "other".into();
    variants.push(altered);
    for altered in variants {
        assert!(project.apply_rename_plan(&altered).is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
}

#[test]
fn registered_json_preserves_raw_bytes_and_reports_each_field_and_key() {
    let mut project = project();
    let manifest = project.root.join(".world/project.json");
    project.set_authoring_document(&manifest, br#"{
        "schema_version":1,"language_version":"1.13",
        "required_features":["content.entities.v1","content.relations.v1","content.object_refs.v1","presentation.graph_views.v1"],
        "graph_views":{"v":".world/graph-views/v.json"}
    }"#.to_vec()).unwrap();
    let path = project.root.join(".world/graph-views/v.json");
    let source = r#"{
  "schema_version": 1, "id":"v", "title":"中文 ledger",
  "focus" : { "kind":"entity", "id":"led\u0067er", "future":"ledger" },
  "filters":{"depth":1},
  "positions" : {"entity:ledger":[0,0],"entity:other":[1,1]},
  "hidden_relation_ids": [],
  "future":{"kind":"entity","id":"ledger"}
}
"#;
    project
        .create_authoring_document(&path, source.as_bytes().to_vec())
        .unwrap();
    let compiled = project.compile();
    let views = worldline_core::graph_views::build_graph_view_index(&project, &compiled);
    assert!(views.diagnostics.is_empty(), "{:?}", views.diagnostics);
    let plan = project
        .plan_rename_target(&TargetRef::new("entity", "ledger"), "flood_ledger")
        .unwrap();
    verify_real_projection(&project, &plan);
    let change = plan.changes.iter().find(|c| c.path == path).unwrap();
    assert_eq!(change.occurrences.len(), 2);
    assert_eq!(change.occurrences[0].field.as_deref(), Some("/focus/id"));
    assert_eq!(change.occurrences[0].before_token, r#"led\u0067er"#);
    assert_eq!(
        change.occurrences[1].field.as_deref(),
        Some("/positions/entity:ledger")
    );
    let expected = source
        .replace(r#""id":"led\u0067er""#, r#""id":"flood_ledger""#)
        .replace(r#""entity:ledger":["#, r#""entity:flood_ledger":["#);
    project.apply_rename_plan(&plan).unwrap();
    assert_eq!(
        project.authoring_document(&path).unwrap().bytes(),
        expected.as_bytes()
    );
}

#[test]
fn legacy_renames_accept_unchanged_new_ids_and_shared_prefixes() {
    for (old, new) in [("a", "alpha"), ("alpha", "a"), ("alphabet", "alpha")] {
        let mut project = project();
        let entry = project.entry.clone();
        let source = format!("// {new} {old} untouched\ncharacter {old} as \"{new} 原样\"\nevent start with {old}\n  {new} 普通正文 [[character:{old}|{new}]]\n  -> END\n");
        project.set_text(&entry, source).unwrap();
        let plan = project
            .plan_rename_target(&TargetRef::new("character", old), new)
            .unwrap();
        verify_real_projection(&project, &plan);
        project.apply_rename_plan(&plan).unwrap();
        let result = project.document(&entry).unwrap();
        assert!(result.contains(&format!("// {new} {old} untouched")));
        assert!(result.contains(&format!("as \"{new} 原样\"")));
        assert!(result.contains(&format!("{new} 普通正文 [[character:{new}|{new}]]")));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn external_disk_change_rejects_without_modifying_buffers_or_disk() {
    let mut project = project();
    project.save().unwrap();
    let plan = project
        .plan_rename_target(&TargetRef::new("entity", "ledger"), "flood_ledger")
        .unwrap();
    let baseline = project.content_baseline();
    let external = format!("{SOURCE}\n// 外部新字节\n");
    std::fs::write(&project.entry, &external).unwrap();
    assert!(project.apply_rename_plan(&plan).is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(std::fs::read_to_string(&project.entry).unwrap(), external);
    std::fs::remove_dir_all(&project.root).unwrap();
}

#[test]
fn cross_file_schema_binding_say_and_escaped_link_traps_share_core_ranges() {
    let mut project = project();
    let path = project
        .add_file(std::path::Path::new("chapters/more.wl"))
        .unwrap();
    let source = r#"schema record for entity
  field note_key note text
bind entity ledger to record // entity ledger
character narrator as "讲述者"
event next with narrator
  say narrator "前缀\t [[entity:ledger|entity ledger]] 尾声" direction "entity ledger [[entity:ledger|舞台纯文字]]"
  普通 \[[entity:ledger|转义链接]] 与 [[entity:ledger|真实链接]]
  -> END
"#;
    project.set_text(&path, source.into()).unwrap();
    assert!(
        !project.compile().has_errors(),
        "{:?}",
        project.compile().diagnostics
    );
    let plan = project
        .plan_rename_target(&TargetRef::new("entity", "ledger"), "flood_ledger")
        .unwrap();
    verify_real_projection(&project, &plan);
    let change = plan.changes.iter().find(|c| c.path == path).unwrap();
    assert_eq!(change.occurrences.len(), 3);
    assert!(change
        .occurrences
        .iter()
        .any(|o| o.field.as_deref() == Some("bind.target.id")));
    assert!(change
        .occurrences
        .iter()
        .any(|o| o.field.as_deref() == Some("say.link.id")));
    let expected = source
        .replace(
            "bind entity ledger to record",
            "bind entity flood_ledger to record",
        )
        .replace(
            "[[entity:ledger|entity ledger]] 尾声",
            "[[entity:flood_ledger|entity ledger]] 尾声",
        )
        .replace(
            "[[entity:ledger|真实链接]]",
            "[[entity:flood_ledger|真实链接]]",
        );
    project.apply_rename_plan(&plan).unwrap();
    assert_eq!(project.document(&path).unwrap(), expected);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn new_external_source_cannot_hide_references_from_an_old_plan() {
    let mut project = project();
    project.save().unwrap();
    let plan = project
        .plan_rename_target(&TargetRef::new("entity", "ledger"), "flood_ledger")
        .unwrap();
    let baseline = project.content_baseline();
    let path = project.root.join("new.wl");
    std::fs::write(&path, "alias entity ledger as \"外部新引用\"\n").unwrap();
    assert!(project
        .apply_rename_plan(&plan)
        .unwrap_err()
        .contains("新增源码"));
    assert_eq!(project.content_baseline(), baseline);
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("entity ledger"));
    std::fs::remove_dir_all(&project.root).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn quoted_catalog_target_ids_keep_quotes_and_display_bytes() {
    let mut project = project();
    std::fs::create_dir_all(&project.root).unwrap();
    std::fs::write(project.root.join("note.txt"), "附件原字节").unwrap();
    let entry = project.entry.clone();
    let prefix = r#"asset note file "note.txt" as "原文"
alias entity "ledger" as "entity ledger"
mark entity "ledger" with important
attach entity "ledger" with note
alias relation "edge" as "relation edge"
mark relation "edge" with important
attach relation "edge" with note
"#;
    project
        .set_text(&entry, format!("{prefix}{SOURCE}"))
        .unwrap();
    assert!(
        !project.compile().has_errors(),
        "{:?}",
        project.compile().diagnostics
    );
    for (kind, id, new) in [
        ("entity", "ledger", "ledger_new"),
        ("relation", "edge", "edge_new"),
    ] {
        let plan = project
            .plan_rename_target(&TargetRef::new(kind, id), new)
            .unwrap();
        verify_real_projection(&project, &plan);
        project.apply_rename_plan(&plan).unwrap();
        let source = project.document(&entry).unwrap();
        for keyword in ["alias", "mark", "attach"] {
            assert!(source.contains(&format!("{keyword} {kind} \"{new}\"")));
        }
        assert!(source.contains(&format!("as \"{kind} {id}\"")));
    }
    assert_eq!(
        std::fs::read_to_string(project.root.join("note.txt")).unwrap(),
        "附件原字节"
    );
    std::fs::remove_dir_all(&project.root).unwrap();
}

#[test]
fn compact_choice_headers_keep_formal_lexer_compatibility() {
    for header in ["choice", "choice once"] {
        let mut project = project();
        let entry = project.entry.clone();
        let source = format!(
            r#"entity ledger kind item as "名称"
event start
  {header}"前缀\t [[entity:ledger|entity ledger]]" // entity ledger
    -> END
"#
        );
        project.set_text(&entry, source.clone()).unwrap();
        assert!(
            !project.compile().has_errors(),
            "{:?}",
            project.compile().diagnostics
        );
        let plan = project
            .plan_rename_target(&TargetRef::new("entity", "ledger"), "flood_ledger")
            .unwrap();
        verify_real_projection(&project, &plan);
        project.apply_rename_plan(&plan).unwrap();
        let expected = source
            .replace("entity ledger kind", "entity flood_ledger kind")
            .replace(
                "[[entity:ledger|entity ledger]]",
                "[[entity:flood_ledger|entity ledger]]",
            );
        assert_eq!(project.document(&entry).unwrap(), expected);
    }
}

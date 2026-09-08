//! 权限迁移、状态唯一真相与旧存档的端到端回归。
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::ast::ChangeKind;
use worldline_core::project::Project;
use worldline_core::{compile_source, compile_sources, CompileResult};
use worldline_runtime::{AnchorKind, Story};

fn checked(result: CompileResult) -> CompileResult {
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    result
}
fn memory(source: &str) -> Project {
    let mut p = Project::new(&std::env::temp_dir().join("worldline-permission-memory"));
    p.documents.retain(|path, _| path == &p.entry);
    p.set_text(&p.entry.clone(), source.into()).unwrap();
    p
}
fn temp(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "worldline-permissions-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn add_remove_tags_preserve_unrelated_values_and_record_repeated_actions() {
    let result = checked(compile_source("story.wl", "tag a\ntag b\ntag c\nworld w\nstate identity on world w with a\nevent start\n  become identity add b, b\n  become identity add b\n  become identity remove c\n  become identity remove b\n  -> END\n"));
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    story.continue_story().unwrap();
    assert_eq!(story.states()["identity"], ["a"]);
    assert_eq!(story.state_history().len(), 4);
    assert_eq!(
        story.state_history()[1].before,
        story.state_history()[1].after
    );
    assert_eq!(
        story.state_history()[2].before,
        story.state_history()[2].after
    );
    assert_eq!(story.state_history()[0].kind, ChangeKind::AddTags);
    assert_eq!(
        result.analysis.catalog.states["identity"].changes[2].kind,
        ChangeKind::RemoveTags
    );
}

#[test]
fn normalized_compiler_and_migrated_sources_have_identical_behavior_and_ids() {
    let source = r#"world earth as "大地"
let literal = "perm(key) grant key revoke key"
let granted = perm(key)
event start
  effect on enter if not perm(key)
    grant key as "http://注释/*文本*/ perm(key)"
  if perm(key)
    中文 {perm(key)}，字符串 {"perm(key)"}，转义 \{perm(key)\}。
  choice "继续 {perm(key)}" if perm(key)
    -> gated
event gated perm key after false or perm(key)
  revoke key
  -> END
"#;
    let mut p = memory(source);
    let before = checked(p.compile());
    let m = before
        .program
        .permission_migration
        .as_ref()
        .unwrap()
        .clone();
    assert!(before.program.events.iter().all(|e| e.perm.is_none()));
    assert_eq!(before.program.worlds.len(), 1);
    assert_eq!(p.migrate_permissions().unwrap(), 1);
    assert_eq!(p.migrate_permissions().unwrap(), 0);
    let text = p.document(&p.entry).unwrap();
    assert!(text.contains("\"perm(key) grant key revoke key\""));
    assert!(text.contains("http://注释/*文本*/ perm(key)"));
    assert!(text.contains("转义 \\{perm(key)\\}"));
    assert!(text.contains("字符串 {\"perm(key)\"}"));
    let after = checked(p.compile());
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    assert_eq!(
        m.state,
        after.program.permission_migration.as_ref().unwrap().state
    );
    for result in [&before, &after] {
        let mut story = Story::new(&result.program, &result.analysis).unwrap();
        let output = story.continue_story().unwrap();
        assert_eq!(story.perm_list(), ["key"]);
        assert_eq!(story.state_view()["perms"], serde_json::json!(["key"]));
        assert!(format!("{output:?}").contains("中文 true"));
        story.choose(0).unwrap();
        story.continue_story().unwrap();
        assert!(story.perm_list().is_empty());
        assert!(story.states()[&m.state].is_empty());
        assert_eq!(story.anchors()[0].kind, AnchorKind::Grant);
        assert_eq!(story.anchors()[1].kind, AnchorKind::Revoke);
    }
}

#[test]
fn preserves_multiline_unicode_comments_and_inline_comments_in_expressions() {
    let source = "/* 中文注释 grant key\nperm(key) */\r\nevent start\r\n  grant /* 授予 */ key // grant key\r\n  if perm(/* 内部 */ key) // perm(key)\r\n    正文 {perm(/* 嵌入 */ key)}\r\n  -> gate\r\nevent gate perm /* 准入 */ key after false /* 或 */ or perm(key) // 尾注\r\n  -> END\r\n";
    let mut p = memory(source);
    let before = checked(p.compile());
    p.migrate_permissions().unwrap();
    let text = p.document(&p.entry).unwrap();
    for comment in [
        "/* 中文注释 grant key\nperm(key) */",
        "/* 授予 */",
        "// grant key",
        "/* 内部 */",
        "// perm(key)",
        "/* 嵌入 */",
        "/* 准入 */",
        "/* 或 */",
        "// 尾注",
    ] {
        assert!(text.contains(comment), "{comment}\n{text}");
    }
    assert_eq!(text.matches("\r\n").count(), source.matches("\r\n").count());
    let after = checked(p.compile());
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
}

#[test]
fn cross_file_collisions_are_deterministic_and_migration_is_idempotent() {
    let mut p = memory("include \"part.wl\"\ntag __narrative_identity\nworld existing\nevent start\n  grant key\n  -> other\n");
    let part = p.add_file(Path::new("part.wl")).unwrap();
    p.set_text(&part, "character __permission_6b6579\nevent other perm key after perm(key)\n  revoke key\n  -> END\n".into()).unwrap();
    let old = checked(p.compile());
    let m = old.program.permission_migration.as_ref().unwrap();
    assert_eq!(m.state, "__narrative_identity_2");
    assert_eq!(m.tags["key"], "__permission_6b6579_2");
    assert_eq!(p.migrate_permissions().unwrap(), 2);
    let sources = p.sources();
    assert_eq!(p.migrate_permissions().unwrap(), 0);
    assert_eq!(p.sources(), sources);
    let new = checked(p.compile());
    assert_eq!(old.analysis.fingerprint, new.analysis.fingerprint);
    assert_eq!(new.program.worlds.len(), 1);
    // 文件映射插入顺序不改变生成 ID。
    let reversed: BTreeMap<_, _> = old
        .sources
        .iter()
        .rev()
        .map(|(p, s)| (p.clone(), s.clone()))
        .collect();
    assert_eq!(
        checked(compile_sources(&p.entry, &reversed))
            .analysis
            .fingerprint,
        old.analysis.fingerprint
    );
}

#[test]
fn open_migrates_dirty_buffers_export_and_save_are_real_and_keep_disk_baseline() {
    let root = temp("disk");
    std::fs::create_dir_all(&root).unwrap();
    let entry = root.join("world.wl");
    let part = root.join("part.wl");
    let source = "include \"part.wl\"\nevent start\n  grant key\n  choice \"继续\"\n    -> gate\n";
    let tail = "event gate perm key\n  -> END\n";
    std::fs::write(&entry, source).unwrap();
    std::fs::write(&part, tail).unwrap();
    let mut project = Project::open(&entry).unwrap();
    assert!(project.is_dirty());
    assert_eq!(std::fs::read_to_string(&entry).unwrap(), source);
    assert_eq!(std::fs::read_to_string(&part).unwrap(), tail);
    let before = checked(project.compile());
    let destination = temp("export");
    project.export(&destination).unwrap();
    let mut reopened = Project::open(&destination).unwrap();
    assert!(!reopened.is_dirty());
    assert_eq!(
        checked(reopened.compile()).analysis.fingerprint,
        before.analysis.fingerprint
    );
    assert!(std::fs::read_to_string(destination.join("world.wl"))
        .unwrap()
        .contains("become "));
    assert_eq!(std::fs::read_to_string(&entry).unwrap(), source);
    project.save().unwrap();
    assert!(!project.is_dirty());
    assert!(std::fs::read_to_string(&entry).unwrap().contains("become "));
    assert!(!Project::open(&entry).unwrap().is_dirty());
    // 外部修改仍触发原保存冲突检测，迁移不覆盖磁盘基线。
    std::fs::write(&entry, source).unwrap();
    std::fs::write(&part, tail).unwrap();
    let mut conflicted = Project::open(&entry).unwrap();
    std::fs::write(&part, format!("{tail}// 合作者\n")).unwrap();
    assert!(conflicted.save().unwrap_err().contains("外部修改"));
}

const OLD_STORY: &str = "event start\n  grant key\n  choice \"继续\"\n    -> gate\nevent gate perm key after perm(key)\n  revoke key\n  -> END\n";
fn old_save(result: &CompileResult) -> String {
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    story.continue_story().unwrap();
    let mut save: serde_json::Value = serde_json::from_str(&story.save().unwrap()).unwrap();
    let m = result.program.permission_migration.as_ref().unwrap();
    // 旧运行时在相同暂停点的载荷：原始 AST 指纹、权限集合、不含身份状态。
    save["fingerprint"] = serde_json::json!(m.legacy_fingerprint);
    save["perms"] = serde_json::json!(["key"]);
    save["states"].as_object_mut().unwrap().remove(&m.state);
    save["state_history"] = serde_json::json!([]);
    save.to_string()
}

#[test]
fn legacy_archive_resumes_before_and_after_source_migration_and_only_new_states_are_saved() {
    let mut p = memory(OLD_STORY);
    let old = checked(p.compile());
    let archive = old_save(&old);
    // 验证兼容指纹来自原始未归一 AST，而非任意白名单。
    let mut diags = Vec::new();
    let lines = worldline_core::lexer::lex_source("old.wl", OLD_STORY, &mut diags);
    let mut raw = worldline_core::parser::Parser::new(&lines, &mut diags).parse_program();
    raw.entry = "start".into();
    assert_eq!(
        worldline_core::fingerprint_program(&raw),
        old.program
            .permission_migration
            .as_ref()
            .unwrap()
            .legacy_fingerprint
    );
    p.migrate_permissions().unwrap();
    let new = checked(p.compile());
    for result in [&old, &new] {
        let mut restored = Story::load(&result.program, &result.analysis, &archive).unwrap();
        assert_eq!(restored.perm_list(), ["key"]);
        let saved: serde_json::Value = serde_json::from_str(&restored.save().unwrap()).unwrap();
        assert!(saved.get("perms").is_none());
        assert_eq!(
            saved["states"][&result.program.permission_migration.as_ref().unwrap().state]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        restored.continue_story().unwrap();
        restored.choose(0).unwrap();
        restored.continue_story().unwrap();
        assert!(restored.perm_list().is_empty());
    }
    // 转存和重新打开后仍保有旧指纹绑定。
    let target = temp("save-as");
    p.save_as(&target).unwrap();
    let result = checked(Project::open(&target).unwrap().compile());
    assert!(Story::load(&result.program, &result.analysis, &archive).is_ok());
}

#[test]
fn rejects_changed_program_unknown_old_permissions_and_conflicting_state_truth() {
    let mut p = memory(OLD_STORY);
    let old = checked(p.compile());
    let archive = old_save(&old);
    p.migrate_permissions().unwrap();
    let text = p.document(&p.entry).unwrap().replace("继续", "改稿");
    p.set_text(&p.entry.clone(), text).unwrap();
    let changed = checked(p.compile());
    assert!(Story::load(&changed.program, &changed.analysis, &archive).is_err());
    let mut bad: serde_json::Value = serde_json::from_str(&archive).unwrap();
    bad["perms"] = serde_json::json!(["unknown"]);
    assert!(Story::load(&old.program, &old.analysis, &bad.to_string()).is_err());
    bad["perms"] = serde_json::json!(["key"]);
    bad["fingerprint"] = serde_json::json!(old.analysis.fingerprint);
    bad["states"][&old.program.permission_migration.as_ref().unwrap().state] =
        serde_json::json!([]);
    assert!(Story::load(&old.program, &old.analysis, &bad.to_string()).is_err());
    bad["perms"] = serde_json::json!([]);
    bad["states"][&old.program.permission_migration.as_ref().unwrap().state] =
        serde_json::json!([old.program.permission_migration.as_ref().unwrap().tags["key"]]);
    assert!(Story::load(&old.program, &old.analysis, &bad.to_string()).is_err());
}

#[test]
fn no_legacy_syntax_leaves_sources_and_fingerprint_untouched() {
    let mut p = memory(
        "world w\nevent start\n  正文grant不是指令。\n  字面 perm(key)。 // grant key\n  -> END\n",
    );
    let before = p.sources();
    assert_eq!(p.migrate_permissions().unwrap(), 0);
    assert_eq!(before, p.sources());
    assert!(checked(p.compile()).program.permission_migration.is_none());
}

#[test]
fn migrated_admission_preserves_old_gate_order_without_hidden_requirements_after_edit() {
    let mut p = memory("event start\n  -> gated\nevent gated perm key after rnd(1, 2) > 0 and 1 / 0 == 0\n  -> END\n");
    let before = checked(p.compile());
    p.migrate_permissions().unwrap();
    let after = checked(p.compile());
    for result in [&before, &after] {
        let mut story = Story::new(&result.program, &result.analysis).unwrap();
        let rng = story.state_view();
        let save_before: serde_json::Value = serde_json::from_str(&story.save().unwrap()).unwrap();
        let err = story.continue_story().unwrap_err();
        assert!(err.message.contains("权限"), "{err}");
        let save_after: serde_json::Value = serde_json::from_str(&story.save().unwrap()).unwrap();
        assert_eq!(save_before["rng"], save_after["rng"]);
        assert_eq!(rng["perms"], serde_json::json!([]));
    }
    let text = p
        .document(&p.entry)
        .unwrap()
        .lines()
        .map(|line| {
            if line.starts_with("event gated ") {
                "event gated".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    p.set_text(&p.entry.clone(), text).unwrap();
    let edited = checked(p.compile());
    let mut story = Story::new(&edited.program, &edited.analysis).unwrap();
    story.continue_story().unwrap();
    assert!(story.is_ended());
}

#[test]
fn quoted_choice_interpolations_preserve_literals_and_state_replacement_controls_permissions() {
    let mut p = memory(
        r#"event start
  grant key
  choice "引号 \"perm(key)\" {perm(\"key\")}" if perm(key)
    -> gate
event gate perm key
  -> END
"#,
    );
    let before = checked(p.compile());
    p.migrate_permissions().unwrap();
    let m = before.program.permission_migration.as_ref().unwrap();
    let text = p.document(&p.entry).unwrap().replace(
        &format!("become {} add {}", m.state, m.tags["key"]),
        &format!("become {} with {}", m.state, m.tags["key"]),
    );
    assert!(text.contains(r#"引号 \"perm(key)\""#));
    p.set_text(&p.entry.clone(), text).unwrap();
    let result = checked(p.compile());
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    story.continue_story().unwrap();
    assert_eq!(story.perm_list(), ["key"]);
    assert_eq!(story.state_history()[0].kind, ChangeKind::Become);
    assert!(story.anchors().is_empty());
    assert_eq!(story.choices()[0].label, "引号 \"perm(key)\" true");
}

#[test]
fn migration_failure_is_atomic_and_missing_new_state_is_not_silently_reset() {
    let mut p = memory("event start\n  grant key\n  if perm(key) +\n    正文\n  -> END\n");
    let before = p.sources();
    assert!(p.migrate_permissions().is_err());
    assert_eq!(before, p.sources());
    let result = checked(compile_source(
        "state.wl",
        "world w\ntag a\nstate s on world w with a\nevent start\n  -> END\n",
    ));
    let story = Story::new(&result.program, &result.analysis).unwrap();
    let mut save: serde_json::Value = serde_json::from_str(&story.save().unwrap()).unwrap();
    save.as_object_mut().unwrap().remove("states");
    assert!(Story::load(&result.program, &result.analysis, &save.to_string()).is_err());
}

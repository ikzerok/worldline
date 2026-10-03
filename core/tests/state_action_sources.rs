//! 状态动作回源只承认实际 AST 身份、正式 parser 来源与原稿动作头。
use std::{collections::BTreeMap, path::PathBuf};
use worldline_core::ast::ChangeKind;
use worldline_core::evidence_source::{
    resolve_evidence_source, state_action_source, EvidenceSource, EvidenceSourceOwner,
    EvidenceSourcePrecision,
};
use worldline_core::{
    compile_source_with_options, compile_sources_with_options, CompileOptions, CompileResult,
};

const DECLARATIONS: &str =
    "character actor\ntag red\ntag blue\nstate mood on character actor with red\n";

fn compile(body: &str) -> CompileResult {
    let result = compile_source_with_options(
        "actions.wl",
        &format!("{DECLARATIONS}{body}"),
        CompileOptions::v1_13(),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}

fn compile_files(main: &str, included: &[(&str, &str)]) -> CompileResult {
    let root = std::env::temp_dir().join(format!("wl-action-sources-{}", std::process::id()));
    let entry = root.join("world.wl");
    let mut sources = BTreeMap::from([(entry.clone(), format!("{DECLARATIONS}{main}"))]);
    sources.extend(
        included
            .iter()
            .map(|(file, text)| (root.join(file), (*text).into())),
    );
    let result = compile_sources_with_options(&entry, &sources, CompileOptions::v1_13());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}

fn during(node: &str, action: ChangeKind) -> EvidenceSourceOwner {
    EvidenceSourceOwner::StateAction {
        node: node.into(),
        action,
        timing: "during".into(),
        effect_index: None,
        action_index: None,
    }
}

fn effect(
    node: &str,
    action: ChangeKind,
    timing: &str,
    block: usize,
    index: usize,
) -> EvidenceSourceOwner {
    EvidenceSourceOwner::StateAction {
        node: node.into(),
        action,
        timing: timing.into(),
        effect_index: Some(block),
        action_index: Some(index),
    }
}

fn line_of(compiled: &CompileResult, header: &str) -> u32 {
    compiled.sources[&PathBuf::from("actions.wl")]
        .lines()
        .position(|line| line.trim() == header)
        .unwrap() as u32
        + 1
}

fn assert_source(
    compiled: &CompileResult,
    owner: EvidenceSourceOwner,
    header: &str,
) -> EvidenceSource {
    let source = state_action_source(&compiled.program, line_of(compiled, header), &owner).unwrap();
    assert_eq!(source.owner, owner);
    let target = resolve_evidence_source(compiled, &source).unwrap();
    assert_eq!(target.precision, EvidenceSourcePrecision::StatementHeader);
    assert_eq!(&compiled.sources[&target.path][target.range], header);
    source
}

#[test]
fn inline_choice_condition_scene_and_dynamic_fragment_actions_have_exact_owners() {
    let compiled = compile(
        "fragment alter(selected: tag)\n  become state(mood) with from tags(selected)\n  if true\n    become state(mood) add from tags(tag(red))\n  choice \"返回\"\n    become state(mood) remove from tags(tag(blue))\n    return\nevent start\n  call alter(tag(blue))\n  become mood with blue\n  if true\n    become mood add red\n  choice \"进入\"\n    become mood remove blue\n    -> start.room\n  scene room\n    become mood with []\n    -> END\n",
    );
    for (node, action, header) in [
        ("start", ChangeKind::Become, "become mood with blue"),
        ("start", ChangeKind::AddTags, "become mood add red"),
        ("start", ChangeKind::RemoveTags, "become mood remove blue"),
        ("start.room", ChangeKind::Become, "become mood with []"),
        (
            "fragment:alter",
            ChangeKind::Become,
            "become state(mood) with from tags(selected)",
        ),
        (
            "fragment:alter",
            ChangeKind::AddTags,
            "become state(mood) add from tags(tag(red))",
        ),
        (
            "fragment:alter",
            ChangeKind::RemoveTags,
            "become state(mood) remove from tags(tag(blue))",
        ),
    ] {
        let source = assert_source(&compiled, during(node, action), header);
        let json = serde_json::to_value(&source).unwrap();
        assert_eq!(json["kind"], "state_action");
        assert_eq!(json["node"], node);
        assert_eq!(
            serde_json::from_value::<EvidenceSource>(json).unwrap(),
            source
        );
    }
    let scene_line = line_of(&compiled, "become mood with []");
    assert!(state_action_source(
        &compiled.program,
        scene_line,
        &during("start", ChangeKind::Become)
    )
    .is_none());
    let fragment_line = line_of(&compiled, "become state(mood) with from tags(selected)");
    assert!(state_action_source(
        &compiled.program,
        fragment_line,
        &during("start", ChangeKind::Become)
    )
    .is_none());
}

#[test]
fn effect_block_and_action_indices_bind_to_the_defining_event() {
    let compiled = compile(
        "event start\n  effect on enter\n    become mood with blue\n    become mood add red\n  effect on exit\n    become mood remove blue\n  effect on done\n    become mood with []\n  scene room\n    -> END\n",
    );
    for (kind, timing, block, index, header) in [
        (ChangeKind::Become, "enter", 0, 0, "become mood with blue"),
        (ChangeKind::AddTags, "enter", 0, 1, "become mood add red"),
        (
            ChangeKind::RemoveTags,
            "exit",
            1,
            0,
            "become mood remove blue",
        ),
        (ChangeKind::Become, "done", 2, 0, "become mood with []"),
    ] {
        assert_source(
            &compiled,
            effect("start", kind, timing, block, index),
            header,
        );
        assert!(state_action_source(
            &compiled.program,
            line_of(&compiled, header),
            &effect("start.room", kind, timing, block, index),
        )
        .is_none());
    }
    let source = assert_source(
        &compiled,
        effect("start", ChangeKind::Become, "enter", 0, 0),
        "become mood with blue",
    );
    for owner in [
        during("start", ChangeKind::Become),
        effect("start", ChangeKind::AddTags, "enter", 0, 0),
        effect("start", ChangeKind::Become, "exit", 0, 0),
        effect("start", ChangeKind::Become, "enter", 2, 0),
        effect("start", ChangeKind::Become, "enter", 0, 1),
        effect("start", ChangeKind::Become, "enter", 99, 0),
        effect("start", ChangeKind::Become, "unknown", 0, 0),
        effect("missing", ChangeKind::Become, "enter", 0, 0),
    ] {
        let forged = EvidenceSource {
            owner,
            ..source.clone()
        };
        assert!(state_action_source(&compiled.program, forged.line, &forged.owner).is_none());
        assert!(resolve_evidence_source(&compiled, &forged).is_err());
    }
    for (block, action) in [(None, Some(0)), (Some(0), None)] {
        let mut forged = source.clone();
        if let EvidenceSourceOwner::StateAction {
            effect_index,
            action_index,
            ..
        } = &mut forged.owner
        {
            *effect_index = block;
            *action_index = action;
        }
        assert!(resolve_evidence_source(&compiled, &forged).is_err());
    }
}

#[test]
fn inline_sources_reject_wrong_line_operation_indices_and_missing_provenance() {
    let mut compiled = compile("event start\n  become mood add blue\n  -> END\n");
    let source = assert_source(
        &compiled,
        during("start", ChangeKind::AddTags),
        "become mood add blue",
    );
    for line in [0, 1, source.line - 1, source.line + 1, u32::MAX] {
        let forged = EvidenceSource {
            line,
            ..source.clone()
        };
        assert!(resolve_evidence_source(&compiled, &forged).is_err());
    }
    for owner in [
        during("start", ChangeKind::Become),
        during("start", ChangeKind::Grant),
        effect("start", ChangeKind::AddTags, "during", 0, 0),
        EvidenceSourceOwner::Choice {
            node: "start".into(),
        },
        EvidenceSourceOwner::Rule {
            name: "start".into(),
        },
    ] {
        assert!(state_action_source(&compiled.program, source.line, &owner).is_none());
    }
    compiled.program.source_provenance = Default::default();
    assert!(state_action_source(&compiled.program, source.line, &source.owner).is_none());
    assert!(resolve_evidence_source(&compiled, &source).is_err());
}

#[test]
fn lexer_must_confirm_the_same_static_or_dynamic_operation_and_physical_location() {
    for (header, replacement, kind) in [
        ("become mood add blue", "普通正文", ChangeKind::AddTags),
        (
            "become mood add blue",
            "become mood remove blue",
            ChangeKind::AddTags,
        ),
        (
            "become mood add blue",
            "become state(mood) add from tags(tag(blue))",
            ChangeKind::AddTags,
        ),
        (
            "become state(mood) add from tags(tag(blue))",
            "become mood add blue",
            ChangeKind::AddTags,
        ),
        (
            "become state(mood) remove from tags(tag(blue))",
            "become state(mood) with from tags(tag(blue))",
            ChangeKind::RemoveTags,
        ),
        (
            "become mood add blue",
            "  become mood add blue",
            ChangeKind::AddTags,
        ),
    ] {
        let mut compiled = compile(&format!("event start\n  {header}\n  -> END\n"));
        let source = assert_source(&compiled, during("start", kind), header);
        let text = compiled
            .sources
            .get_mut(&PathBuf::from("actions.wl"))
            .unwrap();
        *text = text.replace(header, replacement);
        assert!(
            resolve_evidence_source(&compiled, &source).is_err(),
            "{replacement}"
        );
    }
}

#[test]
fn same_physical_locations_in_distinct_fragments_and_event_effects_stay_separate() {
    let compiled = compile_files(
        "include \"a.wl\"\ninclude \"b.wl\"\nevent start\n  call first()\n  call second()\n  -> left\n",
        &[
            ("a.wl", "fragment first()\n  become mood add blue\n  return\nevent left\n  effect on enter\n    become mood add blue\n  -> END\n"),
            ("b.wl", "fragment second()\n  become mood add blue\n  return\nevent right\n  effect on enter\n    become mood add blue\n  -> END\n"),
        ],
    );
    for (first, second, line) in [
        (
            during("fragment:first", ChangeKind::AddTags),
            during("fragment:second", ChangeKind::AddTags),
            2,
        ),
        (
            effect("left", ChangeKind::AddTags, "enter", 0, 0),
            effect("right", ChangeKind::AddTags, "enter", 0, 0),
            6,
        ),
    ] {
        let a = state_action_source(&compiled.program, line, &first).unwrap();
        let b = state_action_source(&compiled.program, line, &second).unwrap();
        assert!(a.file.ends_with("a.wl"));
        assert!(b.file.ends_with("b.wl"));
        assert!(resolve_evidence_source(&compiled, &a).is_ok());
        assert!(resolve_evidence_source(&compiled, &b).is_ok());
        let forged = EvidenceSource { file: b.file, ..a };
        assert!(resolve_evidence_source(&compiled, &forged).is_err());
    }
}

#[test]
fn included_action_source_comes_from_parser_instead_of_event_file() {
    let compiled = compile_files(
        "event start\ninclude \"body.wl\"\n",
        &[(
            "body.wl",
            "  choice \"继续\"\n    if true\n      become mood add blue\n    -> END\n",
        )],
    );
    let source =
        state_action_source(&compiled.program, 3, &during("start", ChangeKind::AddTags)).unwrap();
    assert!(source.file.ends_with("body.wl"));
    let target = resolve_evidence_source(&compiled, &source).unwrap();
    assert_eq!(
        &compiled.sources[&target.path][target.range],
        "become mood add blue"
    );
    assert_eq!(target.column, 7);
}

#[test]
fn indistinguishable_same_root_included_effect_locations_are_unavailable() {
    let compiled = compile_files(
        "event start\ninclude \"a.wl\"\ninclude \"b.wl\"\n  -> END\n",
        &[
            ("a.wl", "  effect on enter\n    become mood add blue\n"),
            ("b.wl", "  effect on exit\n    become mood add blue\n"),
        ],
    );
    for (index, timing) in [(0, "enter"), (1, "exit")] {
        let owner = effect("start", ChangeKind::AddTags, timing, index, 0);
        assert!(state_action_source(&compiled.program, 2, &owner).is_none());
        for file in compiled.sources.keys() {
            let forged = EvidenceSource {
                file: file.to_string_lossy().into(),
                line: 2,
                owner: owner.clone(),
            };
            assert!(resolve_evidence_source(&compiled, &forged).is_err());
        }
    }
}

#[test]
fn comments_moving_sources_and_crlf_preserve_only_current_snapshot_positions() {
    let original = compile("event start\n  become mood add blue\n  -> END\n");
    let old = assert_source(
        &original,
        during("start", ChangeKind::AddTags),
        "become mood add blue",
    );
    let source = format!("{DECLARATIONS}// 移行\nevent start\n  become mood add blue\n  -> END\n")
        .replace('\n', "\r\n");
    let moved = compile_source_with_options("moved.wl", &source, CompileOptions::v1_13());
    assert!(!moved.has_errors(), "{:?}", moved.diagnostics);
    assert_eq!(original.analysis.fingerprint, moved.analysis.fingerprint);
    assert!(resolve_evidence_source(&moved, &old).is_err());
    let current = state_action_source(&moved.program, old.line + 1, &old.owner).unwrap();
    assert_eq!(current.file, "moved.wl");
    let target = resolve_evidence_source(&moved, &current).unwrap();
    assert_eq!(target.column, 3);
    assert_eq!(&source[target.range], "become mood add blue");
}

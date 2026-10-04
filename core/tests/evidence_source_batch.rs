//! 批量定位保持逐项身份/错误/物理范围，只在一次调用内复用同源词法。
use std::{collections::BTreeMap, path::PathBuf};
use worldline_core::ast::ChangeKind;
use worldline_core::evidence_source::{
    resolve_evidence_source, resolve_evidence_sources, state_action_source, EvidenceSource,
    EvidenceSourceOwner, EvidenceSourceTarget, MAX_EVIDENCE_SOURCE_BATCH,
    MAX_EVIDENCE_SOURCE_BATCH_BYTES,
};
use worldline_core::{compile_sources_with_options, CompileOptions, CompileResult};

fn fixture() -> CompileResult {
    let root = std::env::temp_dir().join(format!("worldline-batch-sources-{}", std::process::id()));
    let sources=BTreeMap::from([
        (root.join("world.wl"), "tag one\r\ntag two\r\nworld setting\r\nstate fate on world setting with []\r\nrule allowed() -> bool = true\r\ninclude \"a.wl\"\r\ninclude \"b.wl\"\r\nevent start\r\n  effect on enter\r\n    become fate with one\r\n  choice \"中文选择\" if allowed()\r\n    call first()\r\n    call second()\r\n    -> END\r\n".into()),
        (root.join("a.wl"), "fragment first()\n  become state(fate) add from tags(tag(two))\n  return\n".into()),
        (root.join("b.wl"), "fragment second()\n  become fate remove one\n  return\n".into()),
    ]);
    let compiled =
        compile_sources_with_options(&root.join("world.wl"), &sources, CompileOptions::v1_13());
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    compiled
}
fn action(
    snapshot: &CompileResult,
    node: &str,
    kind: ChangeKind,
    line: u32,
    timing: &str,
    effect_index: Option<usize>,
    action_index: Option<usize>,
) -> EvidenceSource {
    state_action_source(
        &snapshot.program,
        line,
        &EvidenceSourceOwner::StateAction {
            node: node.into(),
            action: kind,
            timing: timing.into(),
            effect_index,
            action_index,
        },
    )
    .unwrap()
}
fn sources(snapshot: &CompileResult) -> Vec<EvidenceSource> {
    let file = snapshot.program.event_files[0].clone();
    vec![
        EvidenceSource {
            file: file.clone(),
            line: 5,
            owner: EvidenceSourceOwner::Rule {
                name: "allowed".into(),
            },
        },
        EvidenceSource {
            file,
            line: 11,
            owner: EvidenceSourceOwner::Choice {
                node: "start".into(),
            },
        },
        action(
            snapshot,
            "start",
            ChangeKind::Become,
            10,
            "enter",
            Some(0),
            Some(0),
        ),
        action(
            snapshot,
            "fragment:first",
            ChangeKind::AddTags,
            2,
            "during",
            None,
            None,
        ),
        action(
            snapshot,
            "fragment:second",
            ChangeKind::RemoveTags,
            2,
            "during",
            None,
            None,
        ),
    ]
}
fn equivalent(
    snapshot: &CompileResult,
    sources: &[EvidenceSource],
) -> Vec<Result<EvidenceSourceTarget, String>> {
    let expected = sources
        .iter()
        .map(|source| resolve_evidence_source(snapshot, source))
        .collect::<Vec<_>>();
    let actual = resolve_evidence_sources(snapshot, &sources.iter().collect::<Vec<_>>()).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), sources.len());
    actual
}
#[test]
fn mixed_choices_rules_dynamic_effects_and_same_line_in_different_files_are_equivalent() {
    let snapshot = fixture();
    let mut requests = sources(&snapshot);
    requests.push(requests[3].clone());
    requests.reverse();
    let results = equivalent(&snapshot, &requests);
    assert!(results.iter().all(Result::is_ok));
    for (source, result) in requests.iter().zip(results) {
        let target = result.unwrap();
        assert_eq!(target.line, source.line);
        let header = &snapshot.sources[&target.path][target.range];
        assert!(!header.contains(['\r', '\n']));
        assert!(
            header.starts_with("rule ")
                || header.starts_with("choice ")
                || header.starts_with("become ")
        );
    }
    let first = resolve_evidence_source(&snapshot, &sources(&snapshot)[3]).unwrap();
    let second = resolve_evidence_source(&snapshot, &sources(&snapshot)[4]).unwrap();
    assert_eq!(first.line, second.line);
    assert_ne!(first.path, second.path);
}
#[test]
fn wrong_owners_operations_indices_paths_and_lines_keep_their_own_errors() {
    let snapshot = fixture();
    let original = sources(&snapshot);
    let mut requests = original.clone();
    for line in [0, 1, u32::MAX] {
        let mut wrong = original[3].clone();
        wrong.line = line;
        requests.push(wrong);
    }
    let mut wrong = original[3].clone();
    wrong.file = original[4].file.clone();
    requests.push(wrong);
    let mut wrong = original[3].clone();
    wrong.file = "../outside.wl".into();
    requests.push(wrong);
    let mut wrong = original[3].clone();
    if let EvidenceSourceOwner::StateAction { action, .. } = &mut wrong.owner {
        *action = ChangeKind::RemoveTags;
    }
    requests.push(wrong);
    let mut wrong = original[2].clone();
    if let EvidenceSourceOwner::StateAction { effect_index, .. } = &mut wrong.owner {
        *effect_index = Some(99);
    }
    requests.push(wrong);
    let mut wrong = original[1].clone();
    wrong.owner = EvidenceSourceOwner::Choice {
        node: "missing".into(),
    };
    requests.push(wrong);
    let mut wrong = original[0].clone();
    wrong.owner = EvidenceSourceOwner::Rule {
        name: "missing".into(),
    };
    requests.push(wrong);
    let results = equivalent(&snapshot, &requests);
    assert!(results[..original.len()].iter().all(Result::is_ok));
    assert!(results[original.len()..].iter().all(Result::is_err));
}
#[test]
fn physical_headers_still_require_current_lexer_classification_and_no_cache_survives_calls() {
    let mut snapshot = fixture();
    let requests = sources(&snapshot);
    assert!(equivalent(&snapshot, &requests).iter().all(Result::is_ok));
    let path = PathBuf::from(&requests[3].file);
    let original = snapshot.sources[&path].clone();
    snapshot
        .sources
        .get_mut(&path)
        .unwrap()
        .replace_range(.., "fragment first()\n  -> END\n");
    let changed = equivalent(&snapshot, &requests);
    assert!(changed[3].is_err());
    assert!(changed[4].is_ok());
    snapshot.sources.insert(path.clone(), original.clone());
    assert!(equivalent(&snapshot, &requests)[3].is_ok());
    snapshot.sources.insert(path, "fragment first()\n".into());
    assert!(equivalent(&snapshot, &requests)[3].is_err());
    snapshot.sources.insert(
        PathBuf::from(&requests[2].file),
        "// truncated current source\n".into(),
    );
    let truncated = equivalent(&snapshot, &requests);
    assert!(truncated[0].is_err() && truncated[1].is_err() && truncated[2].is_err());
}
#[test]
fn limits_reject_the_whole_batch_without_silently_truncating_duplicates() {
    let snapshot = fixture();
    let source = sources(&snapshot).pop().unwrap();
    let at_limit = vec![&source; MAX_EVIDENCE_SOURCE_BATCH];
    let results = resolve_evidence_sources(&snapshot, &at_limit).unwrap();
    assert_eq!(results.len(), MAX_EVIDENCE_SOURCE_BATCH);
    assert!(results.iter().all(Result::is_ok));
    assert!(
        resolve_evidence_sources(&snapshot, &vec![&source; MAX_EVIDENCE_SOURCE_BATCH + 1]).is_err()
    );
    assert!(resolve_evidence_sources(&snapshot, &[]).unwrap().is_empty());
    let mut large = source.clone();
    if let EvidenceSourceOwner::StateAction { node, timing, .. } = &mut large.owner {
        *node = "x".repeat(MAX_EVIDENCE_SOURCE_BATCH_BYTES - large.file.len() - timing.len());
    }
    // 额度内的无效身份是逐项错误；越额则整个请求失败，不混同两种错误域。
    assert!(resolve_evidence_sources(&snapshot, &[&large]).unwrap()[0].is_err());
    if let EvidenceSourceOwner::StateAction { node, .. } = &mut large.owner {
        node.push('测');
    }
    assert!(resolve_evidence_sources(&snapshot, &[&large]).is_err());
}

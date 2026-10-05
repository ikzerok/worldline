//! 变量动作回源仅信任正式节点、变量身份、parser侧表与物理词法头。
use std::{collections::BTreeMap, path::PathBuf};
use worldline_core::evidence_source::{
    resolve_evidence_source, resolve_evidence_sources, variable_write_source,
    variable_write_source_file, EvidenceSource, EvidenceSourceOwner, EvidenceSourcePrecision,
    VariableWriteOperation as Operation, MAX_EVIDENCE_SOURCE_BATCH,
    MAX_EVIDENCE_SOURCE_BATCH_BYTES,
};
use worldline_core::{
    compile_source_with_options, compile_sources_with_options, CompileOptions, CompileResult,
};

fn owner(node: &str, variable: &str, operation: Operation) -> EvidenceSourceOwner {
    EvidenceSourceOwner::VariableWrite {
        node: node.into(),
        variable: variable.into(),
        operation,
    }
}

fn compile(text: &str) -> CompileResult {
    let result = compile_source_with_options("writes.wl", text, CompileOptions::v1_13());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}

fn compile_files(main: &str, included: &[(&str, &str)]) -> CompileResult {
    let root = std::env::temp_dir().join(format!("wl-write-sources-{}", std::process::id()));
    let entry = root.join("world.wl");
    let mut sources = BTreeMap::from([(entry.clone(), main.into())]);
    sources.extend(
        included
            .iter()
            .map(|(file, text)| (root.join(file), (*text).into())),
    );
    let result = compile_sources_with_options(&entry, &sources, CompileOptions::v1_13());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}

fn line_of(compiled: &CompileResult, header: &str) -> u32 {
    compiled.sources[&PathBuf::from("writes.wl")]
        .lines()
        .position(|line| line.trim() == header)
        .unwrap() as u32
        + 1
}

fn assert_source(
    compiled: &CompileResult,
    owner: EvidenceSourceOwner,
    line: u32,
    header: &str,
) -> EvidenceSource {
    let file = variable_write_source_file(&compiled.program, line, &owner).unwrap();
    let source = variable_write_source(&compiled.program, line, &owner).unwrap();
    assert_eq!(source.file, file);
    assert_eq!(source.owner, owner);
    let target = resolve_evidence_source(compiled, &source).unwrap();
    assert_eq!(target.precision, EvidenceSourcePrecision::StatementHeader);
    assert_eq!(
        &compiled.sources[&target.path][target.range.clone()],
        header
    );
    assert_eq!(
        resolve_evidence_sources(compiled, &[&source]).unwrap(),
        vec![Ok(target)]
    );
    source
}

#[test]
fn let_const_set_condition_choice_fragment_and_nested_scene_use_exact_nodes() {
    let compiled = compile(
        "let score = 0\nfragment update(amount: num)\n  local temporary: num = amount\n  if true\n    set score = score + amount\n  const locked = 9\n  let initialized = 4\n  return\nevent start\n  set score = 1\n  if true\n    let branch = 2\n  choice \"进入\"\n    set score = 3\n    -> start.room.inner\n  scene room\n    scene inner\n      if true\n        set score = 4\n      -> END\n",
    );
    for (node, variable, operation, header) in [
        ("start", "score", Operation::Set, "set score = 1"),
        ("start", "branch", Operation::Let, "let branch = 2"),
        ("start", "score", Operation::Set, "set score = 3"),
        ("start.room.inner", "score", Operation::Set, "set score = 4"),
        (
            "fragment:update",
            "score",
            Operation::Set,
            "set score = score + amount",
        ),
        (
            "fragment:update",
            "locked",
            Operation::Const,
            "const locked = 9",
        ),
        (
            "fragment:update",
            "initialized",
            Operation::Let,
            "let initialized = 4",
        ),
    ] {
        let source = assert_source(
            &compiled,
            owner(node, variable, operation),
            line_of(&compiled, header),
            header,
        );
        let json = serde_json::to_value(&source).unwrap();
        assert_eq!(json["kind"], "variable_write");
        assert_eq!(json["node"], node);
        assert_eq!(json["variable"], variable);
        assert_eq!(
            json["operation"],
            match operation {
                Operation::Let => "let",
                Operation::Const => "const",
                Operation::Set => "set",
            }
        );
        assert_eq!(
            serde_json::from_value::<EvidenceSource>(json).unwrap(),
            source
        );
    }
    for (node, variable, operation, header) in [
        ("start", "score", Operation::Let, "let score = 0"),
        ("start", "score", Operation::Set, "set score = 4"),
        ("start.room", "score", Operation::Set, "set score = 4"),
        (
            "fragment:update",
            "temporary",
            Operation::Let,
            "local temporary: num = amount",
        ),
        ("start", "locked", Operation::Const, "const locked = 9"),
    ] {
        assert!(variable_write_source(
            &compiled.program,
            line_of(&compiled, header),
            &owner(node, variable, operation),
        )
        .is_none());
    }
}

#[test]
fn same_lines_in_included_fragments_and_events_do_not_share_source_identity() {
    let compiled = compile_files(
        "let score = 0\ninclude \"a.wl\"\ninclude \"b.wl\"\nevent start\n  call first()\n  call second()\n  -> left\n",
        &[
            ("a.wl", "fragment first()\n  set score = 1\n  return\nevent left\n  set score = 2\n  -> END\n"),
            ("b.wl", "fragment second()\n  set score = 1\n  return\nevent right\n  set score = 2\n  -> END\n"),
        ],
    );
    for (first, second, line, header) in [
        ("fragment:first", "fragment:second", 2, "set score = 1"),
        ("left", "right", 5, "set score = 2"),
    ] {
        let a = assert_source(
            &compiled,
            owner(first, "score", Operation::Set),
            line,
            header,
        );
        let b = assert_source(
            &compiled,
            owner(second, "score", Operation::Set),
            line,
            header,
        );
        assert!(a.file.ends_with("a.wl"));
        assert!(b.file.ends_with("b.wl"));
        assert!(resolve_evidence_source(&compiled, &EvidenceSource { file: b.file, ..a }).is_err());
    }
}

#[test]
fn included_statement_uses_its_physical_file_and_distinguishes_variable_names() {
    let compiled = compile_files(
        "let first = 0\nlet second = 0\nevent start\ninclude \"a.wl\"\ninclude \"b.wl\"\n  -> END\n",
        &[("a.wl", "  set first = 1\n"), ("b.wl", "  set second = 2\n")],
    );
    for (variable, file, header) in [
        ("first", "a.wl", "set first = 1"),
        ("second", "b.wl", "set second = 2"),
    ] {
        let source = assert_source(
            &compiled,
            owner("start", variable, Operation::Set),
            1,
            header,
        );
        assert!(source.file.ends_with(file));
    }
    let nested = compile_files(
        "let score = 0\nevent start\ninclude \"body.wl\"\n",
        &[(
            "body.wl",
            "  choice \"继续\"\n    if true\n      set score = 3\n    -> END\n",
        )],
    );
    let source = assert_source(
        &nested,
        owner("start", "score", Operation::Set),
        3,
        "set score = 3",
    );
    assert!(source.file.ends_with("body.wl"));
    assert_eq!(resolve_evidence_source(&nested, &source).unwrap().column, 7);
}

#[test]
fn same_variable_line_and_column_in_distinct_operations_keep_physical_origins() {
    let compiled = compile_files(
        "event start\ninclude \"a.wl\"\ninclude \"b.wl\"\n  -> END\n",
        &[("a.wl", "  let score = 1\n"), ("b.wl", "  set score = 2\n")],
    );
    for (operation, file, header) in [
        (Operation::Let, "a.wl", "let score = 1"),
        (Operation::Set, "b.wl", "set score = 2"),
    ] {
        let source = assert_source(&compiled, owner("start", "score", operation), 1, header);
        assert!(source.file.ends_with(file));
    }
}

#[test]
fn same_root_scenes_distinguish_same_variable_and_physical_location() {
    let compiled = compile_files(
        "let score = 0\nevent start\n  -> start.left\ninclude \"a.wl\"\ninclude \"b.wl\"\n",
        &[
            ("a.wl", "  scene left\n    set score = 1\n    -> END\n"),
            ("b.wl", "  scene right\n    set score = 2\n    -> END\n"),
        ],
    );
    for (node, file, header) in [
        ("start.left", "a.wl", "set score = 1"),
        ("start.right", "b.wl", "set score = 2"),
    ] {
        let source = assert_source(&compiled, owner(node, "score", Operation::Set), 2, header);
        assert!(source.file.ends_with(file));
    }
}

#[test]
fn indistinguishable_same_node_included_writes_are_unavailable() {
    let compiled = compile_files(
        "let score = 0\nevent start\ninclude \"a.wl\"\ninclude \"b.wl\"\n  -> END\n",
        &[("a.wl", "  set score = 1\n"), ("b.wl", "  set score = 2\n")],
    );
    let owner = owner("start", "score", Operation::Set);
    assert!(variable_write_source(&compiled.program, 1, &owner).is_none());
    for file in compiled.sources.keys() {
        assert!(resolve_evidence_source(
            &compiled,
            &EvidenceSource {
                file: file.to_string_lossy().into(),
                line: 1,
                owner: owner.clone(),
            },
        )
        .is_err());
    }
}

#[test]
fn wrong_names_operations_nodes_paths_lines_and_missing_provenance_are_rejected() {
    let mut compiled = compile("let score = 0\nevent start\n  set score = 1\n  -> END\n");
    let source = assert_source(
        &compiled,
        owner("start", "score", Operation::Set),
        3,
        "set score = 1",
    );
    for owner in [
        owner("start", "other", Operation::Set),
        owner("missing", "score", Operation::Set),
        owner("start.room", "score", Operation::Set),
        owner("fragment:start", "score", Operation::Set),
        owner("start", "score", Operation::Let),
        owner("start", "score", Operation::Const),
        EvidenceSourceOwner::Choice {
            node: "start".into(),
        },
    ] {
        assert!(variable_write_source(&compiled.program, source.line, &owner).is_none());
        assert!(resolve_evidence_source(
            &compiled,
            &EvidenceSource {
                owner,
                ..source.clone()
            }
        )
        .is_err());
    }
    for line in [0, 1, 2, 4, u32::MAX] {
        assert!(resolve_evidence_source(
            &compiled,
            &EvidenceSource {
                line,
                ..source.clone()
            }
        )
        .is_err());
    }
    for file in ["", "../outside.wl", "other.wl"] {
        assert!(resolve_evidence_source(
            &compiled,
            &EvidenceSource {
                file: file.into(),
                ..source.clone()
            }
        )
        .is_err());
    }
    compiled.program.source_provenance = Default::default();
    assert!(variable_write_source(&compiled.program, source.line, &source.owner).is_none());
    assert!(resolve_evidence_source(&compiled, &source).is_err());
}

#[test]
fn formal_lexer_must_confirm_variable_name_operation_and_physical_column() {
    for replacement in [
        "普通正文",
        "let score = 1",
        "const score = 1",
        "set other = 1",
        "  set score = 1",
    ] {
        let mut compiled = compile("let score = 0\nevent start\n  set score = 1\n  -> END\n");
        let source = variable_write_source(
            &compiled.program,
            3,
            &owner("start", "score", Operation::Set),
        )
        .unwrap();
        let text = compiled
            .sources
            .get_mut(&PathBuf::from("writes.wl"))
            .unwrap();
        *text = text.replace("set score = 1", replacement);
        assert!(
            resolve_evidence_source(&compiled, &source).is_err(),
            "{replacement}"
        );
    }
    let mut compiled = compile("event start\n  const locked = 1\n  -> END\n");
    let source = variable_write_source(
        &compiled.program,
        2,
        &owner("start", "locked", Operation::Const),
    )
    .unwrap();
    *compiled
        .sources
        .get_mut(&PathBuf::from("writes.wl"))
        .unwrap() = "event start\n  let locked = 1\n  -> END\n".into();
    assert!(resolve_evidence_source(&compiled, &source).is_err());
}

#[test]
fn mutated_or_duplicated_ast_identity_does_not_reuse_parser_provenance() {
    let mut compiled = compile("let score = 0\nevent start\n  set score = 1\n  -> END\n");
    let source = variable_write_source(
        &compiled.program,
        3,
        &owner("start", "score", Operation::Set),
    )
    .unwrap();
    if let worldline_core::ast::Stmt::Set(statement) = &mut compiled.program.events[0].body[0] {
        statement.name = "other".into();
    }
    assert!(resolve_evidence_source(&compiled, &source).is_err());
    assert!(variable_write_source(
        &compiled.program,
        3,
        &owner("start", "other", Operation::Set)
    )
    .is_none());
    let mut compiled = compile("let score = 0\nevent start\n  set score = 1\n  -> END\n");
    let duplicate = compiled.program.events[0].body[0].clone();
    compiled.program.events[0].body.push(duplicate);
    assert!(variable_write_source(&compiled.program, 3, &source.owner).is_none());
    let mut compiled = compile("event start\n  let score = 1\n  -> END\n");
    let identity = owner("start", "score", Operation::Let);
    if let worldline_core::ast::Stmt::Let(statement) = &mut compiled.program.events[0].body[0] {
        statement.file = "forged.wl".into();
    }
    assert!(variable_write_source(&compiled.program, 2, &identity).is_none());
}

#[test]
fn borrowed_source_allows_runtime_to_reject_long_paths_before_copying_a_record() {
    let file = format!("{}.wl", "x".repeat(4096));
    let compiled = compile_source_with_options(
        &file,
        "event start\n  let score = 1\n  -> END\n",
        CompileOptions::v1_13(),
    );
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let file_ref = variable_write_source_file(
        &compiled.program,
        2,
        &owner("start", "score", Operation::Let),
    )
    .unwrap();
    assert_eq!(file_ref, file);
    assert!(file_ref.len() > 2048);
}

#[test]
fn current_utf8_headers_survive_comments_crlf_and_reject_equal_fingerprint_old_positions() {
    let text = "let score = 0\nevent start\n  set score /* 中文 */ = 1 // 保留\n  -> END\n";
    let original = compile(text);
    let old = assert_source(
        &original,
        owner("start", "score", Operation::Set),
        3,
        "set score /* 中文 */ = 1 // 保留",
    );
    let moved_text = format!("// 移行\n{text}").replace('\n', "\r\n");
    let moved = compile_source_with_options("moved.wl", &moved_text, CompileOptions::v1_13());
    assert!(!moved.has_errors(), "{:?}", moved.diagnostics);
    assert_eq!(original.analysis.fingerprint, moved.analysis.fingerprint);
    assert!(resolve_evidence_source(&moved, &old).is_err());
    let current = variable_write_source(&moved.program, 4, &old.owner).unwrap();
    let target = resolve_evidence_source(&moved, &current).unwrap();
    assert_eq!(target.column, 3);
    assert_eq!(
        &moved_text[target.range],
        "set score /* 中文 */ = 1 // 保留"
    );
}

#[test]
fn variable_identity_is_included_in_existing_batch_byte_and_count_guards() {
    let compiled = compile("let score = 0\nevent start\n  set score = 1\n  -> END\n");
    let source = variable_write_source(
        &compiled.program,
        3,
        &owner("start", "score", Operation::Set),
    )
    .unwrap();
    let results =
        resolve_evidence_sources(&compiled, &vec![&source; MAX_EVIDENCE_SOURCE_BATCH]).unwrap();
    assert_eq!(results.len(), MAX_EVIDENCE_SOURCE_BATCH);
    assert!(results.iter().all(Result::is_ok));
    assert!(
        resolve_evidence_sources(&compiled, &vec![&source; MAX_EVIDENCE_SOURCE_BATCH + 1]).is_err()
    );
    let mut huge = source.clone();
    if let EvidenceSourceOwner::VariableWrite { node, variable, .. } = &mut huge.owner {
        *variable = "x".repeat(MAX_EVIDENCE_SOURCE_BATCH_BYTES - huge.file.len() - node.len());
    }
    assert!(resolve_evidence_sources(&compiled, &[&huge]).unwrap()[0].is_err());
    if let EvidenceSourceOwner::VariableWrite { variable, .. } = &mut huge.owner {
        variable.push('测');
    }
    assert!(resolve_evidence_sources(&compiled, &[&huge]).is_err());
}

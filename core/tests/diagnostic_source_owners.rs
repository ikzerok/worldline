//! Include 合并后执行语句保留实际 file；来源修正不改变旧 parser 接受集合。
use std::{collections::BTreeMap, path::PathBuf};
use worldline_core::diagnostic::DiagnosticSourceRole as Role;
use worldline_core::project::Project;
use worldline_core::{
    compile_sources_with_options, CompileOptions, CompileResult, Diagnostic, Span,
};

fn fixture_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("wl-statement-owner-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    // 与 core 使用相同完整路径身份，不能把 Windows 8.3 别名作为 source map key。
    Project::new(&root).root
}

fn compile(main: &str, included: &[(&str, &str)]) -> (CompileResult, BTreeMap<PathBuf, String>) {
    let root = fixture_root();
    let entry = root.join("world.wl");
    let mut sources = BTreeMap::from([(entry.clone(), main.into())]);
    sources.extend(
        included
            .iter()
            .map(|(file, source)| (root.join(file), (*source).into())),
    );
    (
        compile_sources_with_options(
            &entry,
            &sources,
            CompileOptions::v1_13().with_object_refs(true),
        ),
        sources,
    )
}
fn selected(sources: &BTreeMap<PathBuf, String>, diagnostic: &Diagnostic) -> String {
    slice(&sources[&PathBuf::from(&diagnostic.file)], diagnostic.span)
}
fn slice(text: &str, span: Span) -> String {
    text.lines()
        .nth(span.line as usize - 1)
        .unwrap()
        .chars()
        .skip(span.column as usize - 1)
        .take(span.length as usize)
        .collect()
}
fn assert_origin(
    result: &CompileResult,
    sources: &BTreeMap<PathBuf, String>,
    code: &str,
    file: &str,
    expected: &str,
    role: Role,
) {
    let found: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == code)
        .collect();
    assert!(!found.is_empty(), "{:#?}", result.diagnostics);
    for diagnostic in found {
        assert_eq!(
            diagnostic.file,
            fixture_root().join(file).to_string_lossy(),
            "{diagnostic:?}"
        );
        assert_eq!(diagnostic.source_role(), Some(role));
        assert_eq!(selected(sources, diagnostic), expected);
    }
}

#[test]
fn included_expression_retains_its_actual_file_and_same_single_error_fact() {
    let (result, sources) = compile(
        "event start\ninclude \"actor.wl\"\n",
        &[("actor.wl", "  {missing}\n  -> END\n")],
    );
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "A102");
    assert_eq!(
        result.diagnostics[0].severity,
        worldline_core::Severity::Error
    );
    assert_eq!(
        result.diagnostics[0].message,
        "变量 `missing` 未声明(需要先 let)"
    );
    assert_origin(
        &result,
        &sources,
        "A102",
        "actor.wl",
        "missing",
        Role::Target,
    );
}

#[test]
fn included_divert_set_link_and_effect_sources_share_the_statement_owner_contract() {
    for (body, code, expected, role) in [
        ("  -> missing\n", "A101", "missing", Role::Target),
        (
            "  set missing = true\n  -> END\n",
            "A102",
            "missing",
            Role::Target,
        ),
        (
            "  [[entity:missing|幽灵]]\n  -> END\n",
            "A218",
            "[[entity:missing|幽灵]]",
            Role::Target,
        ),
        (
            "  effect on enter if 7\n    meet missing\n  -> END\n",
            "A103",
            "7",
            Role::Expression,
        ),
        (
            "  effect on enter if 7\n    meet missing\n  -> END\n",
            "A208",
            "meet missing",
            Role::Statement,
        ),
        (
            "  become missing with absent\n  -> END\n",
            "A216",
            "become missing with absent",
            Role::Statement,
        ),
    ] {
        let (result, sources) =
            compile("event start\ninclude \"actor.wl\"\n", &[("actor.wl", body)]);
        assert_origin(&result, &sources, code, "actor.wl", expected, role);
    }
}

#[test]
fn same_numbered_unrelated_variable_and_event_declarations_remain_precise() {
    let (result, sources) = compile(
        "let tide = 1\nevent start\ninclude \"actor.wl\"\n",
        &[("actor.wl", "  {missing}\n")],
    );
    assert_origin(
        &result,
        &sources,
        "A102",
        "actor.wl",
        "missing",
        Role::Target,
    );
    assert_origin(&result, &sources, "A107", "world.wl", "tide", Role::Target);
    assert_origin(
        &result,
        &sources,
        "A202",
        "world.wl",
        "event start",
        Role::Declaration,
    );
}

#[test]
fn same_root_loc_kind_collision_is_unavailable_only_for_affected_statements() {
    let (result, sources) = compile(
        "let tide = 1\nevent start\ninclude \"a.wl\"\ninclude \"b.wl\"\n",
        &[
            ("a.wl", "  {missing}\n"),
            ("b.wl", "  {missing}\n  -> END\n"),
        ],
    );
    let unknown: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "A102")
        .collect();
    assert_eq!(unknown.len(), 2);
    assert!(unknown
        .iter()
        .all(|d| d.source_role() == Some(Role::Unavailable)));
    for diagnostic in unknown {
        assert_eq!(diagnostic.span, Span::new(0, 1, 0));
        let wire = serde_json::to_value(diagnostic).unwrap();
        assert_eq!(wire.as_object().unwrap().len(), 8);
        assert_eq!(wire["span"]["line"], 0);
        assert_eq!(wire["span"]["length"], 0);
        assert!(wire.get("source_role").is_none());
    }
    assert_origin(&result, &sources, "A107", "world.wl", "tide", Role::Target);
}

#[test]
fn fragment_body_and_cross_file_else_if_keep_their_actual_expression_file() {
    let (result, sources) = compile(
        "fragment act()\ninclude \"actor.wl\"\nevent start\n  call act()\n  -> END\n",
        &[("actor.wl", "  {missing}\n  return\n")],
    );
    assert_origin(
        &result,
        &sources,
        "A102",
        "actor.wl",
        "missing",
        Role::Target,
    );
    let (result, sources) = compile(
        "event start\ninclude \"a.wl\"\ninclude \"b.wl\"\n",
        &[
            ("a.wl", "  if true\n    正文\n"),
            ("b.wl", "  else if 9\n    正文\n  -> END\n"),
        ],
    );
    assert_origin(&result, &sources, "A103", "b.wl", "9", Role::Expression);
}

#[test]
fn flow_producers_use_statement_origins_without_changing_flow_facts() {
    let (result, sources) = compile(
        "event start\ninclude \"actor.wl\"\n",
        &[("actor.wl", "  -> start\n")],
    );
    assert_origin(&result, &sources, "A206", "actor.wl", "start", Role::Target);
    let (result, sources) = compile(
        "event start\ninclude \"actor.wl\"\n",
        &[("actor.wl", "  choice once \"离开\"\n    -> END\n")],
    );
    assert_origin(
        &result,
        &sources,
        "A205",
        "actor.wl",
        "choice once \"离开\"",
        Role::Statement,
    );
}

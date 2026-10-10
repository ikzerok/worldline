use super::*;
#[test]
fn production_source_hit_marks_only_its_own_file_as_draft() {
    let root = path();
    let files = BTreeMap::from([
        (
            PathBuf::from("world.wl"),
            b"character a\nevent start\n  say a \"Main\"\n  call detached()\n  -> END\n".to_vec(),
        ),
        (
            PathBuf::from("fragment.wl"),
            b"fragment detached()\n  say a \"Unchanged fragment\"\n  return\n".to_vec(),
        ),
        (
            PathBuf::from(".world/project.json"),
            br#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#.to_vec(),
        ),
    ]);
    let project = on_disk(&root, &files);
    let mut buffer = project
        .open_source_writing_buffer(Path::new("world.wl"))
        .unwrap();
    buffer.replace_source(buffer.source().replace("Main", "Unapplied main"));
    let buffers = [buffer];
    let result = project
        .production_script_snapshot(&buffers, &[], &request())
        .unwrap();
    let fragment = result
        .rows
        .iter()
        .find(|row| row.source.file == "fragment.wl")
        .unwrap();
    let main = result
        .rows
        .iter()
        .find(|row| row.source.file == "world.wl")
        .unwrap();
    assert!(
        !project
            .production_script_source_hit(&buffers, &[], &result, &fragment.row_key)
            .unwrap()
            .draft
    );
    assert!(
        project
            .production_script_source_hit(&buffers, &[], &result, &main.row_key)
            .unwrap()
            .draft
    );
}
#[test]
fn production_ast_envelope_rejects_deep_or_large_trees_without_recursive_walk() {
    let mut compiled = crate::compile_source("world.wl", "event start\n  -> END\n");
    let mut body = Vec::new();
    for _ in 0..66 {
        body = vec![crate::ast::Stmt::Scene(crate::ast::SceneStmt {
            name: "nested".into(),
            body,
            loc: crate::ast::Loc::new(1, 1),
        })];
    }
    compiled.program.events[0].body = body;
    assert_eq!(
        super::super::guards::ast_envelope(&compiled)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    compiled.program.events[0].body =
        vec![crate::ast::Stmt::Return(crate::ast::Loc::new(1, 1)); 200_001];
    assert_eq!(
        super::super::guards::ast_envelope(&compiled)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
}
#[test]
fn production_choice_label_and_direct_scene_keep_their_own_guards() {
    let source = "let n = 1\ncharacter a\ncharacter b\nevent start after n > 0\n  scene inside\n    choice once \"Take\" if n > 0 enable false disabled \"PRIVATE_REASON\"\n      -> END\n";
    let project = fixture(source);
    let mut input = request();
    input.include_choices = true;
    let event = snapshot(&project, &input);
    input.scope = ProductionScope::CurrentTarget {
        target: TargetRef::new("scene", "start.inside"),
    };
    let scene = snapshot(&project, &input);
    assert_eq!(event.rows.len(), 1);
    assert_eq!(scene.rows.len(), 1);
    assert_eq!(
        event.rows[0].control_ancestry,
        scene.rows[0].control_ancestry
    );
    let choice = scene.rows[0]
        .control_ancestry
        .iter()
        .find(|control| control.kind == "choice")
        .unwrap();
    assert_eq!(choice.condition.as_deref(), Some("n > 0"));
    assert_eq!(choice.enable.as_deref(), Some("false"));
    assert!(choice.once);
    assert!(!choice.evaluated);
    assert!(scene.rows[0]
        .control_ancestry
        .iter()
        .any(|control| control.kind == "event_after"));
    let artifact = scene.export(&options(ProductionFormat::Json)).unwrap();
    assert!(!String::from_utf8_lossy(artifact.bytes()).contains("PRIVATE_REASON"));
}

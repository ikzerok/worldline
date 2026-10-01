//! 1.13 静态人物强引用完整生命周期；不引入运行时人物值。
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::project::Project;
use worldline_core::{compile_source_with_options, CompileOptions, CompileResult, TargetRef};
#[path = "character_refs/lifecycle.rs"]
mod lifecycle;
#[path = "character_refs/localization.rs"]
mod localization;
#[path = "character_refs/syntax.rs"]
mod syntax;
#[path = "character_refs/templates.rs"]
mod templates;

const PEOPLE: &str = r#"character lin as "林舟"
  property self = ref("character", "lin")
character mei as "梅"
"#;
const FACTS: &str = r#"schema ship for entity entity_type ship
  field captain_id captain ref character required
entity boat kind ship as "渡船"
  property captain = ref("character", "lin") // ref("character", "lin") 注释不变
  property plain = "lin"
  property relation_value = ref("relation", "command")
  property entity_value = ref("entity", "boat")
bind entity boat to ship
relation_type commands
  from entity
  to character
relation_def command type commands from entity boat to character lin
  property witness = ref("character", "lin")
alias character lin as "lin"
event start with lin
  [[character:lin|lin]] 与普通 lin 文字。
  say lin "你好"
  -> END
"#;
fn options() -> CompileOptions {
    CompileOptions::v1_13()
        .with_object_refs(true)
        .with_character_refs(true)
}
fn compile(source: &str) -> CompileResult {
    compile_source_with_options("world.wl", source, options())
}
fn full_source() -> String {
    format!("{PEOPLE}{FACTS}")
}
fn codes(result: &CompileResult) -> Vec<&str> {
    result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}
struct Fixture {
    root: PathBuf,
    project: Project,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn fixture(name: &str) -> Fixture {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "character-refs-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), FACTS).unwrap();
    fs::write(root.join("people.wl"), PEOPLE).unwrap();
    fs::write(root.join(".world/project.json"), br#"{"schema_version":1,"language_version":"1.13","required_features":["content.object_refs.v1","content.character_refs.v1","content.templates.v1"],"templates":{}}"#).unwrap();
    let mut project = Project::open(&root).unwrap();
    let result = project.compile();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    Fixture { root, project }
}

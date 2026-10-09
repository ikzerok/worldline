//! 正式执行语句的来源；不由渲染文字或暂停后节点反推。
use crate::ast::{Program, Stmt};
use crate::source_provenance::{SourceOwner, StatementKind};

/// 必须传入该 program 内真实执行的正文/说话/选择语句；缺失、歧义或合成 AST 无来源时返回 None。
pub fn runtime_output_source_file<'a>(
    program: &'a Program,
    node: &str,
    statement: &Stmt,
) -> Option<&'a str> {
    let loc = match statement {
        Stmt::Text(text) => text.loc,
        Stmt::Say(say) => say.loc,
        Stmt::Choice(choice) => choice.loc,
        _ => return None,
    };
    let body = super::state_actions::node_body(program, node)?;
    if !contains(body.body, statement) {
        return None;
    }
    program.source_provenance.statement_file(
        &SourceOwner::new(body.file, body.line),
        loc,
        StatementKind::of(statement),
    )
}

fn contains(body: &[Stmt], target: &Stmt) -> bool {
    body.iter().any(|statement| {
        std::ptr::eq(statement, target)
            || match statement {
                Stmt::Choice(choice) => contains(&choice.body, target),
                Stmt::If(branches) => branches
                    .branches
                    .iter()
                    .any(|(_, body)| contains(body, target)),
                // 场景归属自己的完整节点身份。
                _ => false,
            }
    })
}

/// 单个不可变 Program 的进程内索引；键不是持久身份，不能序列化或用于另一份 AST。
/// 文件借用正式 parser 的来源侧表，不为每条语句复制路径。
pub struct RuntimeOutputSourceIndex<'p> {
    files: std::collections::BTreeMap<usize, &'p str>,
    unresolved_file_links: bool,
}

impl<'p> RuntimeOutputSourceIndex<'p> {
    pub fn new(program: &'p Program) -> Self {
        let mut index = Self {
            files: std::collections::BTreeMap::new(),
            unresolved_file_links: false,
        };
        for (position, event) in program.events.iter().enumerate() {
            let file = program.event_files.get(position).map_or("", String::as_str);
            index.walk(
                program,
                &event.body,
                &SourceOwner::new(file, event.loc.line),
            );
        }
        for fragment in &program.fragments {
            index.walk(
                program,
                &fragment.body,
                &SourceOwner::new(&fragment.file, fragment.loc.line),
            );
        }
        index
    }

    pub fn get(&self, statement: &Stmt) -> Option<&'p str> {
        self.files
            .get(&(statement as *const Stmt as usize))
            .copied()
    }

    pub fn has_unresolved_file_links(&self) -> bool {
        self.unresolved_file_links
    }

    fn walk(&mut self, program: &'p Program, body: &[Stmt], owner: &SourceOwner) {
        for statement in body {
            let text = match statement {
                Stmt::Text(text) => Some((text.loc, text.parts.as_slice())),
                Stmt::Say(say) => Some((say.loc, say.text.parts.as_slice())),
                Stmt::Choice(choice) => Some((choice.loc, choice.label.as_slice())),
                _ => None,
            };
            if let Some((loc, parts)) = text {
                if let Some(file) = program.source_provenance.statement_file(owner, loc, StatementKind::of(statement)) {
                    self.files.insert(statement as *const Stmt as usize, file);
                } else if parts.iter().any(|part| matches!(part, crate::ast::TextPart::Link(link) if link.target.kind == "file")) {
                    self.unresolved_file_links = true;
                }
            }
            match statement {
                Stmt::Choice(choice) => self.walk(program, &choice.body, owner),
                Stmt::If(branches) => {
                    for (_, body) in &branches.branches {
                        self.walk(program, body, owner);
                    }
                }
                Stmt::Scene(scene) => self.walk(program, &scene.body, owner),
                _ => {}
            }
        }
    }
}

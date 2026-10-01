//! 调用不是跃迁；仅把片段中真实的尾部跃迁投影为调用者的可能后续连接。
use super::{Ctx, NodeCtx};
use crate::{
    ast::*,
    graph::EdgeKind,
    relation_context::{expression, TransitionContext},
};
impl Ctx<'_> {
    pub(super) fn collect_fragment_transitions(&mut self) {
        for (event, definition) in self.program.events.clone().into_iter().enumerate() {
            let file = self
                .program
                .event_files
                .get(event)
                .cloned()
                .unwrap_or_default();
            self.fragment_transitions(
                &definition.body,
                &NodeCtx {
                    event,
                    node_name: definition.name,
                },
                &file,
                &[],
                &TransitionContext::default(),
            );
        }
    }
    fn fragment_transitions(
        &mut self,
        body: &[Stmt],
        node: &NodeCtx,
        file: &str,
        chain: &[String],
        context: &TransitionContext,
    ) {
        for stmt in body {
            match stmt {
                Stmt::Call(call) => {
                    if chain.len() >= 128 || chain.contains(&call.name) {
                        continue;
                    }
                    if let Some(fragment) = self
                        .program
                        .fragments
                        .iter()
                        .find(|f| f.name == call.name)
                        .cloned()
                    {
                        let mut nested = chain.to_vec();
                        nested.push(call.name.clone());
                        self.fragment_transitions(
                            &fragment.body,
                            node,
                            &fragment.file,
                            &nested,
                            context,
                        );
                    }
                }
                Stmt::Divert(divert) if !chain.is_empty() => {
                    let DivertTarget::Node(target) = &divert.target else {
                        continue;
                    };
                    let Some(path) = self.symbols.resolve_target(target, None) else {
                        continue;
                    };
                    let full = path.full_name(&self.program.events[path.event].name);
                    let before = self.graph_edges.len();
                    self.add_edge(
                        &node.node_name,
                        &full,
                        if divert.drift {
                            EdgeKind::Drift
                        } else {
                            EdgeKind::Divert
                        },
                        Some(format!("经片段 {}", chain.join(" → "))),
                        file,
                        divert.loc.line,
                    );
                    if self.graph_edges.len() > before {
                        self.graph_edges
                            .last_mut()
                            .unwrap()
                            .contexts
                            .push(context.clone());
                    }
                }
                Stmt::If(branches) => {
                    let mut previous = Vec::new();
                    for (condition, body) in &branches.branches {
                        let mut inner = context.clone();
                        inner
                            .conditions
                            .extend(previous.iter().map(|e| format!("not ({e})")));
                        if let Some(condition) = condition {
                            let value = expression(condition);
                            inner.conditions.push(value.clone());
                            previous.push(value);
                        }
                        self.fragment_transitions(body, node, file, chain, &inner);
                    }
                }
                Stmt::Choice(choice) => {
                    let mut inner = context.clone();
                    inner.choices.push(choice.label_raw.clone());
                    if let Some(condition) = &choice.cond {
                        inner.conditions.push(expression(condition));
                    }
                    if let Some(condition) = &choice.enable {
                        inner.conditions.push(expression(condition));
                    }
                    if choice.once {
                        inner.conditions.push("此选择尚未选取".into());
                    }
                    self.fragment_transitions(&choice.body, node, file, chain, &inner);
                }
                Stmt::Scene(scene) => self.fragment_transitions(
                    &scene.body,
                    &NodeCtx {
                        event: node.event,
                        node_name: format!("{}.{}", node.node_name, scene.name),
                    },
                    file,
                    chain,
                    context,
                ),
                _ => {}
            }
        }
    }
}

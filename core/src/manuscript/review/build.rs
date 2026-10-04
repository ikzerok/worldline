use super::*;
use crate::ast::{DivertTarget, EffectBlock, Loc, Stmt, TextPart};
use crate::source_provenance::{ExpressionSlot, SourceOwner, StatementKind};

pub(super) struct Builder<'a> {
    result: &'a CompileResult,
    target: &'a TargetRef,
    sources: source::Sources<'a>,
    pub count: usize,
    pub locations: Vec<ReviewSource>,
    bytes: usize,
}
impl<'a> Builder<'a> {
    pub fn new(result: &'a CompileResult, target: &'a TargetRef) -> Self {
        Self {
            result,
            target,
            sources: source::Sources::new(result),
            count: 0,
            bytes: 0,
            locations: Vec::new(),
        }
    }
    pub fn target(&mut self) -> Result<Vec<ReviewNode>, ReviewError> {
        let result = self.result;
        let object =
            result.analysis.catalog.object(self.target).ok_or_else(|| {
                ReviewError::new("target_unavailable", "审稿目标不存在或无法确认")
            })?;
        match self.target.kind.as_str() {
            "event" => {
                let index = result
                    .program
                    .event_index(&self.target.id)
                    .ok_or_else(ReviewError::source)?;
                let event = &result.program.events[index];
                let file = result
                    .program
                    .event_files
                    .get(index)
                    .ok_or_else(ReviewError::source)?;
                let owner = SourceOwner::new(file, event.loc.line);
                let source = self.location(file, event.loc.line)?;
                let header = ReviewNode::new(
                    ReviewKind::Structure,
                    &format!("{} · 事件入口约束未求值", source.excerpt),
                    Some(source.clone()),
                );
                self.finish(&header, 0)?;
                let mut output = vec![header];
                if !event.effects.is_empty() {
                    let mut effects = ReviewNode::new(
                        ReviewKind::Structure,
                        "事件效果声明 · 与正文分开展示，不拆分选择组",
                        None,
                    );
                    self.finish(&effects, 0)?;
                    for effect in &event.effects {
                        effects.children.push(self.effect(effect, &owner, 1)?);
                    }
                    output.push(effects);
                }
                output.extend(self.body(&event.body, &owner, 0)?);
                Ok(output)
            }
            "fragment" => {
                let fragment = result
                    .program
                    .fragments
                    .iter()
                    .find(|item| item.name == self.target.id)
                    .ok_or_else(ReviewError::source)?;
                let source = self.location(&fragment.file, fragment.loc.line)?;
                let header = ReviewNode::new(
                    ReviewKind::Structure,
                    &format!("{} · 片段声明，未调用", source.excerpt),
                    Some(source.clone()),
                );
                self.finish(&header, 0)?;
                let mut output = vec![header];
                output.extend(self.body(
                    &fragment.body,
                    &SourceOwner::new(&fragment.file, fragment.loc.line),
                    0,
                )?);
                Ok(output)
            }
            "scene" => {
                let path = result
                    .analysis
                    .symbols
                    .scenes
                    .get(&self.target.id)
                    .ok_or_else(ReviewError::source)?;
                if path.scenes.len() > MAX_DEPTH {
                    return Err(ReviewError::limit());
                }
                let event = result
                    .program
                    .events
                    .get(path.event)
                    .ok_or_else(ReviewError::source)?;
                let file = result
                    .program
                    .event_files
                    .get(path.event)
                    .ok_or_else(ReviewError::source)?;
                let mut body = event.body.as_slice();
                for name in &path.scenes {
                    body = body
                        .iter()
                        .find_map(|stmt| match stmt {
                            Stmt::Scene(scene) if &scene.name == name => {
                                Some(scene.body.as_slice())
                            }
                            _ => None,
                        })
                        .ok_or_else(ReviewError::source)?;
                }
                self.body(body, &SourceOwner::new(file, event.loc.line), 0)
            }
            "entity" => {
                let entity = result
                    .program
                    .entities
                    .iter()
                    .find(|entity| entity.name == self.target.id)
                    .ok_or_else(ReviewError::source)?;
                let source = self.location(&object.file, object.line)?;
                let mut node = ReviewNode::new(
                    ReviewKind::Description,
                    "实体描述（来源定位到声明）",
                    Some(source),
                );
                node.parts.push(ReviewPart {
                    text: entity.description.clone(),
                    target: None,
                    dynamic: false,
                });
                self.finish(&node, 0)?;
                Ok(vec![node])
            }
            _ => Err(ReviewError::new(
                "unsupported_target",
                "审稿仅支持 event、scene、fragment、entity",
            )),
        }
    }
    fn location(&self, file: &str, line: u32) -> Result<ReviewSource, ReviewError> {
        let span = self
            .result
            .program
            .source_provenance
            .statements
            .get(&(file.into(), line))
            .ok_or_else(ReviewError::source)?
            .span;
        self.sources.location(self.target, file, span)
    }
    fn statement_source(
        &self,
        owner: &SourceOwner,
        loc: Loc,
        kind: StatementKind,
    ) -> Result<ReviewSource, ReviewError> {
        let file = self
            .result
            .program
            .source_provenance
            .statement_file(owner, loc, kind)
            .ok_or_else(ReviewError::source)?;
        self.location(file, loc.line)
    }
    fn expression(
        &self,
        file: &str,
        line: u32,
        slot: ExpressionSlot,
    ) -> Result<String, ReviewError> {
        let expression = self
            .result
            .program
            .source_provenance
            .expression(file, line, slot)
            .ok_or_else(ReviewError::source)?;
        self.sources.text(
            &expression.file,
            expression.span.ok_or_else(ReviewError::source)?,
        )
    }
    fn body(
        &mut self,
        body: &[Stmt],
        owner: &SourceOwner,
        depth: usize,
    ) -> Result<Vec<ReviewNode>, ReviewError> {
        let items = body.iter().map(Item::Statement).collect::<Vec<_>>();
        self.items(&items, owner, depth)
    }
    fn items(
        &mut self,
        items: &[Item<'_>],
        owner: &SourceOwner,
        depth: usize,
    ) -> Result<Vec<ReviewNode>, ReviewError> {
        if depth > MAX_DEPTH {
            return Err(ReviewError::limit());
        }
        let mut output = Vec::new();
        let mut index = 0;
        while let Some(item) = items.get(index) {
            if matches!(item, Item::Statement(Stmt::Choice(_))) {
                let mut group =
                    ReviewNode::new(ReviewKind::ChoiceGroup, "选择组 · 各选项互为备选", None);
                group.end_label = Some("选择组结束 · 仅控制流继续时汇合至下文".into());
                self.finish(&group, depth)?;
                while let Some(Item::Statement(statement @ Stmt::Choice(_))) = items.get(index) {
                    group
                        .children
                        .push(self.statement(statement, owner, depth + 1)?);
                    index += 1;
                }
                output.push(group);
            } else {
                output.push(match item {
                    Item::Statement(statement) => self.statement(statement, owner, depth)?,
                });
                index += 1;
            }
        }
        Ok(output)
    }
    fn statement(
        &mut self,
        statement: &Stmt,
        owner: &SourceOwner,
        depth: usize,
    ) -> Result<ReviewNode, ReviewError> {
        if depth > MAX_DEPTH {
            return Err(ReviewError::limit());
        }
        let loc = crate::language::statement_loc(statement);
        let source = self.statement_source(owner, loc, StatementKind::of(statement))?;
        let file = source.file.clone();
        let mut node =
            ReviewNode::new(ReviewKind::Structure, &source.excerpt, Some(source.clone()));
        match statement {
            Stmt::Text(text) => {
                node.kind = ReviewKind::Text;
                node.label.clear();
                node.parts = parts(&text.parts);
                node.glue = text.glue;
            }
            Stmt::Say(say) => {
                node.kind = ReviewKind::Say;
                node.label = say.direction.clone().unwrap_or_default();
                let target = TargetRef::new("character", &say.speaker);
                let speaker = self
                    .result
                    .analysis
                    .catalog
                    .object(&target)
                    .ok_or_else(ReviewError::source)?;
                if speaker.display.len() > MAX_REVIEW_JSON_BYTES.saturating_sub(self.bytes) {
                    return Err(ReviewError::limit());
                }
                node.speaker = Some(ReviewSpeaker {
                    target,
                    display: speaker.display.clone(),
                });
                node.parts = parts(&say.text.parts);
                node.glue = say.text.glue;
            }
            Stmt::If(branches) => {
                node.kind = ReviewKind::If;
                node.label = "条件分组 · 互斥分支".into();
                node.end_label = Some("条件组结束 · 仅控制流继续时汇合至下文".into());
                self.finish(&node, depth)?;
                for (index, (condition, body)) in branches.branches.iter().enumerate() {
                    let (physical, span) = self
                        .result
                        .program
                        .source_provenance
                        .branch_headers
                        .get(&(file.clone(), loc.line, index))
                        .ok_or_else(ReviewError::source)?;
                    let source = self.sources.location(self.target, physical, *span)?;
                    let label = if index == 0 {
                        "if"
                    } else if condition.is_some() {
                        "else if"
                    } else {
                        "else"
                    };
                    let mut branch = ReviewNode::new(ReviewKind::Branch, label, Some(source));
                    if condition.is_some() {
                        branch.condition = Some(self.expression(
                            &file,
                            loc.line,
                            ExpressionSlot::Condition(index as u32),
                        )?);
                    }
                    self.finish(&branch, depth + 1)?;
                    branch.children = self.body(body, owner, depth + 2)?;
                    node.children.push(branch);
                }
                return Ok(node);
            }
            Stmt::Choice(choice) => {
                node.kind = ReviewKind::Choice;
                node.label = "选项".into();
                node.parts = parts(&choice.label);
                node.once = choice.once;
                node.disabled_reason = choice.disabled_reason.clone();
                if choice.cond.is_some() {
                    node.condition =
                        Some(self.expression(&file, loc.line, ExpressionSlot::Condition(0))?);
                }
                if choice.enable.is_some() {
                    node.enable = Some(self.expression(&file, loc.line, ExpressionSlot::Enable)?);
                }
                self.finish(&node, depth)?;
                node.children = self.body(&choice.body, owner, depth + 1)?;
                return Ok(node);
            }
            Stmt::Scene(scene) => {
                node.kind = ReviewKind::Scene;
                node.label = format!("场景 {}", scene.name);
                node.target = self
                    .result
                    .analysis
                    .catalog
                    .objects
                    .iter()
                    .find(|object| {
                        object.target.kind == "scene"
                            && object.file == file
                            && object.line == loc.line
                    })
                    .map(|object| object.target.clone());
                node.end_label = Some("场景块结束 · 仅控制流继续时进入下文".into());
                self.finish(&node, depth)?;
                node.children = self.body(&scene.body, owner, depth + 1)?;
                return Ok(node);
            }
            Stmt::Call(call) => {
                node.kind = ReviewKind::Call;
                node.label = format!("{} · 静态调用，不展开、不执行参数", source.excerpt);
                node.target = Some(TargetRef::new("fragment", &call.name));
            }
            Stmt::Return(_) => {
                node.kind = ReviewKind::Return;
                node.label = "return · 返回调用处".into();
            }
            Stmt::Divert(divert) => {
                node.kind = ReviewKind::Divert;
                node.label = format!("{} · 控制流离开此处", source.excerpt);
                if let DivertTarget::Node(id) = &divert.target {
                    let current_event = self
                        .result
                        .program
                        .events
                        .iter()
                        .zip(&self.result.program.event_files)
                        .find(|(event, file)| event.loc.line == owner.line && **file == owner.file)
                        .map(|(event, _)| event.name.as_str());
                    let path = self
                        .result
                        .analysis
                        .symbols
                        .resolve_target(id, current_event)
                        .ok_or_else(ReviewError::source)?;
                    let event = self
                        .result
                        .program
                        .events
                        .get(path.event)
                        .ok_or_else(ReviewError::source)?;
                    node.target = Some(TargetRef::new(
                        if path.scenes.is_empty() {
                            "event"
                        } else {
                            "scene"
                        },
                        &path.full_name(&event.name),
                    ));
                }
            }
            Stmt::Effect(effect) => return self.effect(effect, owner, depth),
            _ => {}
        }
        self.finish(&node, depth)?;
        Ok(node)
    }
    fn effect(
        &mut self,
        effect: &EffectBlock,
        owner: &SourceOwner,
        depth: usize,
    ) -> Result<ReviewNode, ReviewError> {
        let source = self.statement_source(owner, effect.loc, StatementKind::Effect)?;
        let mut node = ReviewNode::new(
            ReviewKind::Structure,
            &format!(
                "{} · 按 enter/exit/done 时机生效，此处未执行",
                source.excerpt
            ),
            Some(source.clone()),
        );
        self.finish(&node, depth)?;
        for action in &effect.actions {
            let source = self.statement_source(owner, action.loc, StatementKind::Change)?;
            let child =
                ReviewNode::new(ReviewKind::Structure, &source.excerpt, Some(source.clone()));
            self.finish(&child, depth + 1)?;
            node.children.push(child);
        }
        Ok(node)
    }
    fn finish(&mut self, node: &ReviewNode, depth: usize) -> Result<(), ReviewError> {
        self.count += 1;
        if self.count > MAX_NODES || depth > MAX_DEPTH {
            return Err(ReviewError::limit());
        }
        // 在保留节点或克隆下一次长显示名之前，计量全部字段及 JSON 转义。
        // 所有调用点均在填充 children 前调用，因此每个节点只计一次。
        let mut budget = ByteBudget(MAX_REVIEW_JSON_BYTES.saturating_sub(self.bytes));
        serde_json::to_writer(&mut budget, node).map_err(|_| ReviewError::limit())?;
        self.bytes = MAX_REVIEW_JSON_BYTES - budget.0;
        if let Some(source) = &node.source {
            self.locations.push(source.clone());
        }
        Ok(())
    }
}
fn parts(source: &[TextPart]) -> Vec<ReviewPart> {
    source
        .iter()
        .map(|part| match part {
            TextPart::Str(text) => ReviewPart {
                text: text.clone(),
                target: None,
                dynamic: false,
            },
            TextPart::Link(link) => ReviewPart {
                text: link.label.clone(),
                target: Some(link.target.clone()),
                dynamic: false,
            },
            TextPart::Expr(_) => ReviewPart {
                text: "〔动态内容 · 未求值〕".into(),
                target: None,
                dynamic: true,
            },
        })
        .collect()
}
enum Item<'a> {
    Statement(&'a Stmt),
}

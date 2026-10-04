//! 可暂停片段和类型化动作，复用原有执行帧与状态历史。
use super::{model::FrameSrc, Frame, Output, RunError, Story, Value};
use std::collections::{BTreeMap, HashSet};
use worldline_core::ast::{Change, Stmt};

impl<'p> Story<'p> {
    pub(super) fn active_scope(&self) -> (BTreeMap<String, Value>, HashSet<String>) {
        let Some(frame) = self.frames.iter().rev().find(|f| f.fragment.is_some()) else {
            return (BTreeMap::new(), HashSet::new());
        };
        let Some(fragment) = self
            .program
            .fragments
            .iter()
            .find(|f| Some(&f.name) == frame.fragment.as_ref())
        else {
            return (BTreeMap::new(), HashSet::new());
        };
        let declared = fragment
            .parameters
            .iter()
            .map(|p| p.name.clone())
            .chain(
                worldline_core::language::locals(&fragment.body)
                    .into_iter()
                    .map(|l| l.name.clone()),
            )
            .collect();
        (frame.locals.clone(), declared)
    }
    pub(super) fn execute_language(
        &mut self,
        fi: usize,
        out: &mut Vec<Output>,
    ) -> Result<(), RunError> {
        let stmt = &self.frames[fi].stmts[self.frames[fi].idx];
        match stmt {
            Stmt::Local(l) => {
                let v = self.eval(&l.expr)?;
                if v.kind() != l.kind {
                    return Err(RunError::new("local值类型与声明不符"));
                }
                let frame = self
                    .frames
                    .iter_mut()
                    .rev()
                    .find(|f| f.fragment.is_some())
                    .ok_or_else(|| RunError::new("local不在片段调用内"))?;
                frame.locals.insert(l.name.clone(), v);
                self.frames[fi].idx += 1;
            }
            Stmt::Call(c) => {
                if self.frames.iter().filter(|f| f.fragment.is_some()).count() >= 128 {
                    return Err(RunError::new("片段调用深度超过128层"));
                }
                let index = self
                    .program
                    .fragments
                    .iter()
                    .position(|f| f.name == c.name)
                    .ok_or_else(|| RunError::new(format!("未知片段 `{}`", c.name)))?;
                let fragment = &self.program.fragments[index];
                if c.args.len() != fragment.parameters.len() {
                    return Err(RunError::new("片段参数数量不符"));
                }
                let mut locals = BTreeMap::new();
                for (p, e) in fragment.parameters.iter().zip(&c.args) {
                    let v = self.eval(e)?;
                    if v.kind() != p.kind {
                        return Err(RunError::new("片段参数类型不符"));
                    }
                    locals.insert(p.name.clone(), v);
                }
                let stmt = self.frames[fi].idx;
                self.frames[fi].idx += 1;
                self.frames.push(Frame {
                    stmts: &fragment.body,
                    idx: 0,
                    node: None,
                    src: Some(FrameSrc::FragmentCall {
                        stmt,
                        fragment: index,
                    }),
                    fragment: Some(fragment.name.clone()),
                    locals,
                });
            }
            Stmt::Return(_) => {
                let index = self
                    .frames
                    .iter()
                    .rposition(|f| f.fragment.is_some())
                    .ok_or_else(|| RunError::new("return不在片段调用内"))?;
                self.frames.truncate(index);
            }
            Stmt::Say(s) => {
                let (content, links) = self.render_parts(&s.text.parts)?;
                out.push(Output::Text {
                    speaker: Some(worldline_core::catalog::TargetRef {
                        kind: "character".into(),
                        id: s.speaker.clone(),
                    }),
                    content,
                    new_line: !self.glue_pending,
                    tags: s.text.tags.clone(),
                    links,
                });
                self.glue_pending = s.text.glue;
                self.frames[fi].idx += 1;
            }
            Stmt::DynamicChange(c) => {
                let state = self.eval(&c.state)?;
                let tags = self.eval(&c.tags)?;
                let (Value::StateRef(id), Value::TagSet(tags)) = (state, tags) else {
                    return Err(RunError::new("动态状态操作需要状态身份和标签集合"));
                };
                if !self.states.contains_key(&id)
                    || tags.iter().any(|id| {
                        !self.program.catalog.iter().any(
                        |d| matches!(d,worldline_core::catalog::CatalogDecl::Tag(t) if &t.name==id),
                    )
                    })
                {
                    return Err(RunError::new("动态状态操作包含未知身份"));
                }
                let source = self.action_source(c.kind, c.loc.line);
                self.apply_change(
                    &Change {
                        id,
                        tags,
                        kind: c.kind,
                        note: None,
                        to_storyline: None,
                        loc: c.loc,
                    },
                    source,
                )?;
                self.frames[fi].idx += 1;
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    pub(super) fn fragment_choice_id(
        &self,
        depth: usize,
        start: usize,
        offset: usize,
    ) -> Option<String> {
        let root = self.frames[..=depth]
            .iter()
            .rposition(|f| f.fragment.is_some())?;
        let mut id = format!("fragment:{}", self.frames[root].fragment.as_deref()?);
        for frame in &self.frames[root + 1..=depth] {
            match frame.src {
                Some(FrameSrc::IfBranch { stmt, branch }) => {
                    id.push_str(&format!("/if:{stmt}:{branch}"))
                }
                Some(FrameSrc::ChoiceBody { stmt }) => id.push_str(&format!("/choice:{stmt}")),
                _ => {}
            }
        }
        Some(format!("{id}:{start}:{offset}"))
    }
    pub(super) fn current_source_file(&self) -> Option<&str> {
        if let Some(name) = self.frames.iter().rev().find_map(|f| f.fragment.as_deref()) {
            return self
                .program
                .fragments
                .iter()
                .find(|f| f.name == name)
                .map(|f| f.file.as_str());
        }
        let event = self.current_event_name()?;
        let path = self.symbols.events.get(&event)?;
        self.program.event_files.get(path.event).map(String::as_str)
    }
    pub(super) fn call_view(&self) -> Vec<serde_json::Value> {
        self.frames.iter().enumerate().filter_map(|(i,f)| {
            let name = f.fragment.as_ref()?;
            let definition = self.program.fragments.iter().find(|d| &d.name == name)?;
            let caller = self.frames[..i].iter().rev().find_map(|f|f.fragment.as_ref().map(|n|format!("fragment:{n}")).or_else(||f.node.clone()));
            let call_statement = match f.src { Some(FrameSrc::FragmentCall{stmt,..}) => Some(stmt), _ => None };
            Some(serde_json::json!({"fragment":name,"statement":f.idx,"line":f.stmts.get(f.idx).map(super::util::stmt_line),"file":definition.file,"caller":caller,"call_statement":call_statement,"locals":f.locals}))
        }).collect()
    }
}

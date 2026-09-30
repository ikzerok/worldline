use super::{
    model::{FrameSave, FrameSrc},
    Frame, RunError, Story,
};
use worldline_core::{ast::Stmt, Analysis};
impl<'p> Story<'p> {
    pub(super) fn rebuild_fragment_frame(
        &self,
        save: &FrameSave,
        frames: &[Frame<'p>],
        analysis: &Analysis,
    ) -> Result<Option<Frame<'p>>, RunError> {
        let Some(name) = &save.fragment else {
            return Ok(None);
        };
        if save.node.is_some() {
            return Err(RunError::new("片段帧不能同时是事件帧"));
        }
        if frames.iter().filter(|f| f.fragment.is_some()).count() >= 128 {
            return Err(RunError::new("存档片段调用超过128层"));
        }
        let Some(FrameSrc::FragmentCall { stmt, fragment }) = save.src else {
            return Err(RunError::new("片段帧缺少调用来源"));
        };
        let definition = self
            .program
            .fragments
            .get(fragment)
            .filter(|f| &f.name == name)
            .ok_or_else(|| RunError::new("存档片段身份与索引不符"))?;
        let parent = frames
            .last()
            .ok_or_else(|| RunError::new("片段不能是存档根帧"))?;
        if parent.idx != stmt + 1
            || !matches!(parent.stmts.get(stmt),Some(Stmt::Call(call)) if &call.name==name)
        {
            return Err(RunError::new("片段返回点与调用位置不符"));
        }
        if save.idx > definition.body.len() {
            return Err(RunError::new("片段语句位置越界"));
        }
        for p in &definition.parameters {
            if !save.locals.contains_key(&p.name) {
                return Err(RunError::new(format!("存档缺少片段参数 `{}`", p.name)));
            }
        }
        let locals = worldline_core::language::locals(&definition.body);
        for (name, value) in &save.locals {
            let kind = definition
                .parameters
                .iter()
                .find(|p| &p.name == name)
                .map(|p| p.kind)
                .or_else(|| locals.iter().find(|l| &l.name == name).map(|l| l.kind))
                .ok_or_else(|| RunError::new(format!("存档包含未知局部 `{name}`")))?;
            super::variable_validation::validate_value(analysis, value, Some(kind))
                .map_err(|message| RunError::new(format!("存档局部 `{name}` 无效：{message}")))?;
        }
        Ok(Some(Frame {
            stmts: &definition.body,
            idx: save.idx,
            node: None,
            src: save.src,
            fragment: Some(name.clone()),
            locals: save.locals.clone(),
        }))
    }
}

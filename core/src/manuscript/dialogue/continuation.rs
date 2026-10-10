use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Witness {
    path: PathBuf,
    after_workspace_guard: String,
    source: String,
    original: String,
    target: TargetRef,
    line: u32,
    before_baseline: String,
    after_baseline: String,
    staged_generation: u64,
    observation: String,
}

pub(super) fn witness(
    project: &Project,
    buffer: &WritingBuffer,
    edit: &writer::Edit,
    candidate: &Project,
    plan: &DialogueEditPlan,
) -> Result<Option<Witness>> {
    if !plan.can_apply || edit.expected.is_none() {
        return Ok(None);
    }
    Ok(Some(Witness {
        path: buffer.path().to_owned(),
        after_workspace_guard: if edit.no_change {
            plan.workspace_guard.clone()
        } else {
            prepare::workspace(candidate)?
        },
        source: if edit.no_change {
            buffer.source()
        } else {
            candidate
                .document(buffer.path())
                .map_err(|e| DialogueError::new("SOURCE_UNAVAILABLE", e))?
        }
        .into(),
        original: buffer.original().into(),
        target: plan.request.target.clone(),
        line: edit.line,
        before_baseline: project.content_baseline(),
        after_baseline: candidate.content_baseline(),
        staged_generation: if edit.no_change {
            buffer.generation()
        } else {
            buffer.generation().wrapping_add(1)
        },
        observation: project.manuscript_observation_key(),
    }))
}

impl Project {
    /// 仅从已验证计划的私有完整结果见证定位下一句，不信任调用方修改的范围或 ID。
    pub fn dialogue_continuation(
        &self,
        buffer: &WritingBuffer,
        plan: &DialogueEditPlan,
    ) -> Result<DialogueInsertionAnchor> {
        prepare::guard(self, buffer)?;
        let unavailable = || {
            DialogueError::new(
                "STALE_DRAFT",
                "无法确认刚修改的正式语句，请保留输入并重新选择下一句位置",
            )
        };
        let witness = plan.continuation.as_ref().ok_or_else(unavailable)?;
        let baseline = self.content_baseline();
        let workspace_guard = prepare::workspace(self)?;
        let staged = baseline == witness.before_baseline
            && buffer.original() == witness.original
            && buffer.generation() == witness.staged_generation
            && workspace_guard == plan.workspace_guard;
        let applied = baseline == witness.after_baseline
            && buffer.original() == witness.source
            && buffer.generation() == 0
            && workspace_guard == witness.after_workspace_guard;
        if buffer.path() != witness.path
            || self.root != plan.root
            || self.search_refresh_generation() != plan.refresh_generation
            || self.manuscript_observation_key() != witness.observation
            || buffer.source() != witness.source
            || !(staged || applied)
        {
            return Err(unavailable());
        }
        let projection = self.project_dialogue_buffer(buffer, &witness.target)?;
        let id = projection
            .statements
            .iter()
            .find(|statement| statement.source.line == witness.line)
            .and_then(|statement| statement.after_anchor_id.as_ref())
            .ok_or_else(unavailable)?;
        projection
            .anchors
            .iter()
            .find(|anchor| &anchor.id == id)
            .cloned()
            .ok_or_else(unavailable)
    }
}

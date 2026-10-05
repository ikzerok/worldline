use super::*;
use crate::project::Project;
use std::path::Path;

impl Project {
    /// 当前精确编辑正文的单文件结构；不会应用/保存该正文。
    pub fn source_outline(&self, path: &Path, source: &str) -> SourceOutline {
        let path = crate::compiler::source_path(path);
        let mut result = SourceOutline {
            path: path.clone(),
            status: SourceOutlineStatus::Ready,
            message: None,
            entries: Vec::new(),
            stamp: Stamp {
                source: signature(source),
                baseline: self.content_baseline(),
                generation: self.search_refresh_generation(),
                options: self.compile_options(),
            },
            statements: Vec::new(),
            comments: Vec::new(),
            non_boundaries: Vec::new(),
        };
        let outcome = self
            .outline_guard(&path)
            .and_then(|()| ranges::build(&path.to_string_lossy(), source, result.stamp.options));
        match outcome {
            Ok((entries, statements, comments)) => {
                result.entries = entries;
                result.statements = statements;
                result.comments = comments;
                result.non_boundaries = source
                    .char_indices()
                    .flat_map(|(offset, ch)| (offset + 1)..(offset + ch.len_utf8()))
                    .collect();
            }
            Err((status, message)) => {
                result.status = status;
                result.message = Some(message);
            }
        }
        if !output_within_budget(&result, MAX_SOURCE_OUTLINE_OUTPUT_BYTES) {
            result.status = SourceOutlineStatus::BudgetExceeded;
            result.message = Some("结构投影序列化输出超过 2 MiB 预算".into());
            result.entries.clear();
            result.statements.clear();
            result.comments.clear();
            result.non_boundaries.clear();
        }
        result
    }

    /// 跳转前重新确认当前源与 Project / 磁盘身份，只返回当前声明头范围。
    pub fn source_outline_range(
        &self,
        outline: &SourceOutline,
        source: &str,
        occurrence: usize,
    ) -> Result<Range<usize>, String> {
        if outline.status != SourceOutlineStatus::Ready {
            return Err(outline
                .message
                .clone()
                .unwrap_or_else(|| "本文件结构暂不可用".into()));
        }
        if !outline.matches_source(source)
            || outline.stamp.baseline != self.content_baseline()
            || outline.stamp.generation != self.search_refresh_generation()
            || outline.stamp.options != self.compile_options()
        {
            return Err("源码结构已过期，请更新本文件结构后定位".into());
        }
        let fresh = self.source_outline(&outline.path, source);
        if fresh.status != SourceOutlineStatus::Ready {
            return Err(fresh.message.unwrap_or_else(|| "来源暂不可用".into()));
        }
        if &fresh != outline {
            return Err("源码结构与当前来源不一致，请重新查询".into());
        }
        fresh
            .entries
            .get(occurrence)
            .map(|entry| entry.header.clone())
            .ok_or_else(|| "声明来源不存在，请重新查询".into())
    }

    fn outline_guard(&self, path: &Path) -> Result<(), (SourceOutlineStatus, String)> {
        let unavailable = |message: String| (SourceOutlineStatus::Unavailable, message);
        if !self.authoring_diagnostics().is_empty() {
            return Err(unavailable(
                "工作区清单或能力尚未确认，本文件结构暂不可用".into(),
            ));
        }
        crate::file_access::within(&self.root, path).map_err(unavailable)?;
        self.document(path).map_err(unavailable)?;
        if self
            .source_selection()
            .is_some_and(|selection| !selection.is_active(path))
        {
            return Err((
                SourceOutlineStatus::Inactive,
                "非活动源码不提供可导航结构".into(),
            ));
        }
        // One bounded inventory validates all baselines, newly added manifests and transactions.
        self.verify_review_navigation().map_err(|message| {
            unavailable(
                message
                    .replace("审稿", "源码结构")
                    .replace("组织源码", "确认源码结构")
                    .replace("源码组织未提交", "源码结构暂不可用"),
            )
        })
    }
}

use super::*;
use crate::project::Project;
use std::path::Path;

impl Project {
    /// 当前精确编辑正文的只读定位预览，不要求先应用或修复语法。
    pub fn preview_source_jump(
        &self,
        path: &Path,
        source: &str,
        request: &str,
    ) -> Result<SourceJumpPreview, String> {
        // 在工作区 IO 前先拒绝不合法请求与超预算正文。
        let request = Request::parse(request)?;
        let coordinates = SourceCoordinates::new(source)?;
        let position = coordinates.locate_request(request)?;
        let path = crate::file_access::within(&self.root, path)?;
        self.source_jump_guard(&path)?;
        let context = coordinates.context(position);
        Ok(SourceJumpPreview {
            path: path.clone(),
            position,
            line_count: coordinates.line_count(),
            max_column: coordinates.lines[position.line - 1].characters + 1,
            context,
            stamp: Stamp {
                root: self.root.clone(),
                path,
                source: coordinates.source,
                baseline: self.content_baseline(),
                generation: self.search_refresh_generation(),
                options: self.compile_options(),
                request,
            },
        })
    }

    /// 重新验证精确正文、工程身份、全部公开预览与磁盘保护，返回零宽字节插入点。
    pub fn resolve_source_jump(
        &self,
        preview: &SourceJumpPreview,
        source: &str,
    ) -> Result<Range<usize>, String> {
        if preview.stamp.root != self.root
            || preview.stamp.path != preview.path
            || preview.stamp.source != source
            || preview.stamp.baseline != self.content_baseline()
            || preview.stamp.generation != self.search_refresh_generation()
            || preview.stamp.options != self.compile_options()
        {
            return Err("源码定位预览已过期，请使用当前正文重新预览".into());
        }
        let request = format!(
            "{}:{}",
            preview.stamp.request.line, preview.stamp.request.column
        );
        let fresh = self.preview_source_jump(&preview.stamp.path, source, &request)?;
        if &fresh != preview {
            return Err("源码定位预览与当前来源不一致，请重新预览".into());
        }
        Ok(fresh.position.byte_offset..fresh.position.byte_offset)
    }

    fn source_jump_guard(&self, path: &Path) -> Result<(), String> {
        if path.extension().is_none_or(|extension| extension != "wl") {
            return Err("源码定位只接受工作区内已载入的 .wl 文件".into());
        }
        self.document(path)?;
        // 包含有界库存、未知清单/能力、未载入新文件、事务和保存基线检查；
        // 不检查活动集或 DSL 诊断，不编译当前或其它文件。
        self.verify_review_navigation().map_err(|message| {
            message
                .replace("审稿", "源码定位")
                .replace("组织源码", "确认源码定位")
                .replace("源码组织未提交", "源码定位暂不可用")
        })
    }
}

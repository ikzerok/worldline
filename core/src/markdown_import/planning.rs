use super::{
    mapping, markdown, source, InputFile, MarkdownImportOptions, ParsedMarkdownSource,
    PreparedPreview, Project, MAX_MARKDOWN_BYTES_PER_PAGE, MAX_MARKDOWN_BYTES_TOTAL,
};
use std::path::{Path, PathBuf};

pub(super) fn prepare_preview(
    project: &Project,
    source_root: PathBuf,
    inputs: Vec<InputFile<'_>>,
    options: &MarkdownImportOptions,
) -> Result<PreparedPreview, String> {
    let markdown = inputs
        .iter()
        .filter(|file| {
            Path::new(&file.relative)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        })
        .cloned()
        .collect::<Vec<_>>();
    if markdown.is_empty() {
        return Err("来源目录中没有 Markdown 页面".into());
    }

    let mut text_total = 0usize;
    let mut pages = Vec::with_capacity(markdown.len());
    for input in &markdown {
        if input.length > MAX_MARKDOWN_BYTES_PER_PAGE as u64 {
            return Err(format!("Markdown 页面超过单页预算：{}", input.relative));
        }
        text_total = text_total
            .checked_add(input.length as usize)
            .ok_or("Markdown 文本大小溢出")?;
        if text_total > MAX_MARKDOWN_BYTES_TOTAL {
            return Err("Markdown 文本超过单次迁移预算".into());
        }
        let bytes = source::read_input_file(input)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| format!("Markdown 页面不是有效 UTF-8：{}", input.relative))?;
        let parsed = markdown::parse_page(&input.relative, &bytes, text)?;
        pages.push(parsed);
    }
    pages.sort_by(|left, right| left.relative.cmp(&right.relative));
    mapping::prepare_preview_from_pages(
        project,
        options,
        ParsedMarkdownSource {
            source_root,
            inputs,
            markdown,
            pages,
        },
    )
}

pub(super) fn validate_project_for_import(
    project: &Project,
    options: &MarkdownImportOptions,
) -> Result<(), String> {
    if !project.authoring_diagnostics().is_empty() {
        return Err("工作区存在只读诊断，不能预览迁移写入".into());
    }
    if !project.recovery_conflicts().is_empty() {
        return Err("工程存在未解决的保存事务冲突".into());
    }
    let baseline = project.content_baseline();
    if options.expected_baseline != baseline {
        return Err(format!(
            "工程基线已过期，拒绝迁移预览；当前基线为 {baseline}"
        ));
    }
    source::check_tracked_disk_baselines(project)?;

    Ok(())
}

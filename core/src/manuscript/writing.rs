//! 一个源文件一个缓冲；正文、结构和源码是其带代次范围投影。
use crate::catalog::TargetRef;
use crate::lexer::{lex_source_with_options, LineKind};
use crate::project::Project;
use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritingBlockKind {
    Prose,
    Structure,
}

#[derive(Debug, Clone)]
pub struct WritingBlock {
    pub kind: WritingBlockKind,
    pub label: String,
    pub line: u32,
    pub range: Range<usize>,
    pub text: String,
    pub source: String,
    pub indent: String,
}

#[derive(Debug, Clone)]
pub struct WritingProjection {
    pub target: TargetRef,
    pub generation: u64,
    pub range: Range<usize>,
    pub source: String,
    pub blocks: Vec<WritingBlock>,
}

#[derive(Debug, Clone)]
pub struct WritingBuffer {
    path: PathBuf,
    original: String,
    text: String,
    baseline: String,
    generation: u64,
}

impl WritingBuffer {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn source(&self) -> &str {
        &self.text
    }
    pub fn baseline(&self) -> &str {
        &self.baseline
    }
    pub fn is_changed(&self) -> bool {
        self.text != self.original
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 源码输入不要求立即可编译；无效输入仍留在唯一缓冲中。
    pub fn replace_source(&mut self, text: String) {
        if self.text != text {
            self.text = text;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    pub fn replace_range(
        &mut self,
        generation: u64,
        range: Range<usize>,
        expected: &str,
        replacement: &str,
    ) -> Result<(), String> {
        if generation != self.generation || self.text.get(range.clone()) != Some(expected) {
            return Err("正文范围已过期，输入未写入；请重新查看当前草稿".into());
        }
        self.text.replace_range(range, replacement);
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }

    /// 正文块只替换原行的文字范围；新行继承原缩进，其他字节不重建。
    pub fn replace_prose(
        &mut self,
        generation: u64,
        block: &WritingBlock,
        text: &str,
    ) -> Result<(), String> {
        if block.kind != WritingBlockKind::Prose {
            return Err("结构块请在结构或源码视图编辑".into());
        }
        let newline = if self.text.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let replacement = text
            .replace("\r\n", "\n")
            .split('\n')
            .collect::<Vec<_>>()
            .join(&format!("{newline}{}", block.indent));
        self.replace_range(generation, block.range.clone(), &block.source, &replacement)
    }

    /// 仅当已提交工程中的该文件仍是原文时，允许同步非源码编排的基线。
    pub fn rebase_unchanged_source(&mut self, project: &Project) -> Result<(), String> {
        if project.document(&self.path)? != self.original {
            return Err("正文文件已变化，不能自动更新草稿基线".into());
        }
        self.baseline = project.content_baseline();
        Ok(())
    }
}

impl Project {
    pub fn open_writing_buffer(&self, target: &TargetRef) -> Result<WritingBuffer, String> {
        let result = self.compile_current();
        let object = result
            .analysis
            .catalog
            .object(target)
            .ok_or("正文目标未解析")?;
        let path = source_path(self, &object.file);
        if !matches!(
            target.kind.as_str(),
            "event" | "scene" | "entity" | "fragment"
        ) {
            return Err("仅事件、场景、实体和片段可以成为书稿正文来源".into());
        }
        self.open_source_writing_buffer(&path)
    }

    /// 完整源码入口不要求目标或当前语法有效；仅接受已加载的工作区文件。
    pub fn open_source_writing_buffer(&self, path: &Path) -> Result<WritingBuffer, String> {
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            self.root.join(path)
        };
        crate::file_access::within(&self.root, &path)?;
        let text = self.document(&path)?.to_owned();
        Ok(WritingBuffer {
            path,
            original: text.clone(),
            text,
            baseline: self.content_baseline(),
            generation: 0,
        })
    }

    pub fn project_writing_buffer(
        &self,
        buffer: &WritingBuffer,
        target: &TargetRef,
    ) -> Result<WritingProjection, String> {
        let mut candidate = self.clone();
        candidate.set_text(&buffer.path, buffer.text.clone())?;
        let result = candidate.compile_current();
        if result.has_errors() {
            return Err("草稿暂不能解析；全部输入已保留，请在源码视图继续修复".into());
        }
        let object = result
            .analysis
            .catalog
            .object(target)
            .ok_or("正文目标已变化或未解析")?;
        if source_path(self, &object.file) != buffer.path {
            return Err("来源文件已变化，不能在旧文件草稿中定位章节".into());
        }
        let lines =
            lex_source_with_options(&object.file, &buffer.text, &mut Vec::new(), result.options);
        let header = lines
            .iter()
            .position(|line| line.no == object.line)
            .ok_or("正文声明范围无法确认")?;
        if !matches!(
            lines[header].kind,
            LineKind::Event { .. } | LineKind::Scene { .. } | LineKind::Entity { .. }
        ) && !matches!(&lines[header].kind, LineKind::Language111 { keyword, .. } if keyword == "fragment")
        {
            return Err("该对象暂不支持安全正文范围编辑".into());
        }
        let offsets = line_offsets(&buffer.text);
        let start = *offsets
            .get(object.line as usize)
            .unwrap_or(&buffer.text.len());
        let end_line = lines
            .iter()
            .skip(header + 1)
            .find(|line| line.indent <= lines[header].indent)
            .map(|line| line.no);
        let end = end_line
            .and_then(|line| offsets.get(line as usize - 1).copied())
            .unwrap_or(buffer.text.len());
        let range = start..end;
        let mut blocks: Vec<WritingBlock> = Vec::new();
        for (index, raw) in buffer.text.split_inclusive('\n').enumerate() {
            let offset = offsets[index];
            if offset < start || offset >= end {
                continue;
            }
            let raw = raw.trim_end_matches(['\n', '\r']);
            let indent_len = raw.len() - raw.trim_start_matches([' ', '\t']).len();
            let line = (index + 1) as u32;
            let kind = if lines
                .iter()
                .any(|entry| entry.no == line && matches!(entry.kind, LineKind::Text { .. }))
            {
                WritingBlockKind::Prose
            } else {
                WritingBlockKind::Structure
            };
            if let Some(previous) = blocks.last_mut().filter(|previous| {
                previous.kind == WritingBlockKind::Prose
                    && ((kind == WritingBlockKind::Prose && previous.indent == raw[..indent_len])
                        || raw.trim().is_empty())
            }) {
                previous.range.end = offset + raw.len();
                previous.source = buffer.text[previous.range.clone()].into();
                previous.text = deindent(&previous.source, &previous.indent);
                continue;
            }
            let label = lines
                .iter()
                .find(|entry| entry.no == line)
                .map(|entry| structure_label(&entry.kind))
                .unwrap_or("注释 / 留白");
            blocks.push(WritingBlock {
                kind,
                label: label.into(),
                line,
                range: offset + indent_len..offset + raw.len(),
                text: raw[indent_len..].into(),
                source: raw[indent_len..].into(),
                indent: raw[..indent_len].into(),
            });
        }
        Ok(WritingProjection {
            target: target.clone(),
            generation: buffer.generation,
            source: buffer.text[range.clone()].into(),
            range,
            blocks,
        })
    }

    pub fn preview_writing_buffer(&self, buffer: &WritingBuffer) -> Result<(), String> {
        self.writing_candidate(buffer, false).map(|_| ())
    }

    /// 预览与应用共用候选验证；失败不改变工程或缓冲。
    pub fn apply_writing_buffer(&mut self, buffer: &WritingBuffer) -> Result<(), String> {
        *self = self.writing_candidate(buffer, false)?;
        Ok(())
    }

    /// 明确完整源码模式：诊断作为结果返回，不以编译失败抛弃作者原稿。
    pub fn preview_source_writing_buffer(
        &self,
        buffer: &WritingBuffer,
    ) -> Result<Vec<crate::Diagnostic>, String> {
        Ok(self
            .writing_candidate(buffer, true)?
            .compile_current()
            .diagnostics)
    }

    pub fn apply_source_writing_buffer(
        &mut self,
        buffer: &WritingBuffer,
    ) -> Result<Vec<crate::Diagnostic>, String> {
        let candidate = self.writing_candidate(buffer, true)?;
        let diagnostics = candidate.compile_current().diagnostics;
        *self = candidate;
        Ok(diagnostics)
    }

    fn writing_candidate(
        &self,
        buffer: &WritingBuffer,
        allow_invalid_source: bool,
    ) -> Result<Project, String> {
        if buffer.baseline != self.content_baseline()
            || self.document(&buffer.path)? != buffer.original
        {
            return Err("正文草稿基线已过期；输入已保留，请核对工程变化".into());
        }
        self.ensure_workspace_writable()?;
        if !self.recovery_conflicts().is_empty() {
            return Err("工程有未解决的保存事务冲突".into());
        }
        super::commands::ensure_disk_matches_saved_baselines(self)?;
        let mut candidate = self.clone();
        if allow_invalid_source {
            candidate.set_text(&buffer.path, buffer.text.clone())?;
        } else {
            candidate.edit(|project| project.set_text(&buffer.path, buffer.text.clone()))?;
        }
        Ok(candidate)
    }
}

fn source_path(project: &Project, file: &str) -> PathBuf {
    let path = Path::new(file);
    if path.is_absolute() {
        path.to_owned()
    } else {
        project.root.join(path)
    }
}

fn line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for line in text.split_inclusive('\n') {
        offsets.push(offsets.last().copied().unwrap_or(0) + line.len());
    }
    offsets
}

fn structure_label(kind: &LineKind) -> &'static str {
    match kind {
        LineKind::Text { .. } => "正文",
        LineKind::Choice { .. } => "选项",
        LineKind::If { .. } | LineKind::ElseIf { .. } | LineKind::Else { .. } => "条件",
        LineKind::Divert { .. } => "去向",
        LineKind::Scene { .. } => "场景",
        LineKind::Let { .. } | LineKind::Const { .. } | LineKind::Set { .. } => "变量",
        LineKind::Language111 { keyword, .. } => match keyword.as_str() {
            "say" => "角色台词",
            "call" => "调用片段",
            "return" => "返回调用处",
            "local" => "局部绑定",
            "become" => "集合状态",
            _ => "语言结构",
        },
        LineKind::Description { .. } => "资料正文",
        _ => "结构",
    }
}

fn deindent(source: &str, indent: &str) -> String {
    let normalized = source.replace("\r\n", "\n");
    normalized
        .split('\n')
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                line
            } else {
                line.strip_prefix(indent).unwrap_or(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

//! 当前精确源码的物理行与 Unicode 标量列；契约见 spec/source-coordinates.md。
use serde::Serialize;
use std::{ops::Range, path::PathBuf};
mod project;

pub const MAX_SOURCE_COORDINATE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SOURCE_COORDINATE_LINES: usize = 65_536;
pub const MAX_SOURCE_JUMP_REQUEST_BYTES: usize = 128;
pub const MAX_SOURCE_JUMP_CONTEXT_CHARACTERS: usize = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SourcePosition {
    pub line: usize,
    pub column: usize,
    pub byte_offset: usize,
    pub character_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLineContext {
    pub text: String,
    pub byte_range: Range<usize>,
    pub start_column: usize,
    pub truncated_start: bool,
    pub truncated_end: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceJumpPreview {
    pub path: PathBuf,
    pub position: SourcePosition,
    pub line_count: usize,
    pub max_column: usize,
    pub context: SourceLineContext,
    #[serde(skip)]
    stamp: Stamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    root: PathBuf,
    path: PathBuf,
    source: String,
    baseline: String,
    generation: u64,
    options: crate::CompileOptions,
    request: Request,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Request {
    line: usize,
    column: usize,
}

#[derive(Debug, Clone)]
struct Line {
    start: usize,
    end: usize,
    start_character: usize,
    characters: usize,
}

/// 可缓存的精确正文索引；不会编译源码或推断语法归属。
#[derive(Debug, Clone)]
pub struct SourceCoordinates {
    source: String,
    lines: Vec<Line>,
}

impl SourceCoordinates {
    pub fn new(source: &str) -> Result<Self, String> {
        if source.len() > MAX_SOURCE_COORDINATE_BYTES {
            return Err("源码坐标超过 2 MiB 正文预算".into());
        }
        let mut lines = Vec::new();
        let (mut start, mut start_character) = (0, 0);
        for segment in source.split_inclusive('\n') {
            let content = match segment.strip_suffix('\n') {
                Some(line) => line.strip_suffix('\r').unwrap_or(line),
                None => segment,
            };
            let characters = content.chars().count();
            push_line(
                &mut lines,
                Line {
                    start,
                    end: start + content.len(),
                    start_character,
                    characters,
                },
            )?;
            start += segment.len();
            // LF/CRLF 各一个物理换行，但原文 Unicode 标量索引不归一化。
            start_character += characters + segment.len() - content.len();
        }
        if source.is_empty() || source.ends_with('\n') {
            push_line(
                &mut lines,
                Line {
                    start,
                    end: start,
                    start_character,
                    characters: 0,
                },
            )?;
        }
        Ok(Self {
            source: source.to_owned(),
            lines,
        })
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// 0 基整文 Unicode 标量光标转换为 1 基物理行列；CRLF 中间拒绝。
    pub fn position_at_character(
        &self,
        source: &str,
        index: usize,
    ) -> Result<SourcePosition, String> {
        self.verify_source(source)?;
        let line_index = self
            .lines
            .partition_point(|line| line.start_character <= index)
            .saturating_sub(1);
        let line = &self.lines[line_index];
        let column_offset = index - line.start_character;
        if column_offset > line.characters {
            return Err("光标字符索引超出正文或位于 CRLF 换行中间".into());
        }
        Ok(self.position(line_index, column_offset))
    }

    /// 0 基 UTF-8 字节光标转换为 1 基物理行列；UTF-8 与 CRLF 中间拒绝。
    pub fn position_at_byte(&self, source: &str, index: usize) -> Result<SourcePosition, String> {
        self.verify_source(source)?;
        if !source.is_char_boundary(index) {
            return Err("光标字节索引超出正文或不在 UTF-8 边界".into());
        }
        let line_index = self
            .lines
            .partition_point(|line| line.start <= index)
            .saturating_sub(1);
        let line = &self.lines[line_index];
        if index > line.end {
            return Err("光标不能位于 CRLF 换行中间".into());
        }
        let column_offset = source[line.start..index].chars().count();
        Ok(SourcePosition {
            line: line_index + 1,
            column: column_offset + 1,
            byte_offset: index,
            character_offset: line.start_character + column_offset,
        })
    }

    /// 解析整个输入 trim 后的 ASCII 十进制“行:列”或“行”。
    pub fn locate(&self, source: &str, request: &str) -> Result<SourcePosition, String> {
        self.verify_source(source)?;
        self.locate_request(Request::parse(request)?)
    }

    fn verify_source(&self, source: &str) -> Result<(), String> {
        if self.source != source {
            return Err("源码坐标已过期，请使用当前完整正文重新查询".into());
        }
        Ok(())
    }

    fn locate_request(&self, request: Request) -> Result<SourcePosition, String> {
        let line = self
            .lines
            .get(request.line - 1)
            .ok_or_else(|| format!("目标行超出范围，当前共有 {} 行", self.line_count()))?;
        if request.column > line.characters + 1 {
            return Err(format!(
                "目标列超出范围，第 {} 行允许 1 到 {} 列",
                request.line,
                line.characters + 1
            ));
        }
        Ok(self.position(request.line - 1, request.column - 1))
    }

    fn position(&self, line_index: usize, column_offset: usize) -> SourcePosition {
        let line = &self.lines[line_index];
        SourcePosition {
            line: line_index + 1,
            column: column_offset + 1,
            byte_offset: self.line_byte_offset(line, column_offset),
            character_offset: line.start_character + column_offset,
        }
    }

    fn line_byte_offset(&self, line: &Line, column_offset: usize) -> usize {
        self.source[line.start..line.end]
            .char_indices()
            .nth(column_offset)
            .map_or(line.end, |(offset, _)| line.start + offset)
    }

    fn context(&self, position: SourcePosition) -> SourceLineContext {
        let line = &self.lines[position.line - 1];
        let start = (position.column - 1)
            .saturating_sub(MAX_SOURCE_JUMP_CONTEXT_CHARACTERS / 2)
            .min(
                line.characters
                    .saturating_sub(MAX_SOURCE_JUMP_CONTEXT_CHARACTERS),
            );
        let end = (start + MAX_SOURCE_JUMP_CONTEXT_CHARACTERS).min(line.characters);
        let byte_range = self.line_byte_offset(line, start)..self.line_byte_offset(line, end);
        SourceLineContext {
            text: self.source[byte_range.clone()].to_owned(),
            byte_range,
            start_column: start + 1,
            truncated_start: start > 0,
            truncated_end: end < line.characters,
        }
    }
}

impl Request {
    fn parse(request: &str) -> Result<Self, String> {
        if request.len() > MAX_SOURCE_JUMP_REQUEST_BYTES {
            return Err("定位请求超过 128 字节预算".into());
        }
        let request = request.trim();
        let (line, column) = request.split_once(':').unwrap_or((request, "1"));
        let parse = |value: &str| -> Result<usize, String> {
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("请输入行号或行:列，只允许 ASCII 十进制正整数，内部不能含空白".into());
            }
            let number = value
                .parse::<usize>()
                .map_err(|_| "行号或列号数值溢出".to_string())?;
            if number == 0 {
                return Err("行号和列号从 1 开始，不能为 0".into());
            }
            Ok(number)
        };
        Ok(Self {
            line: parse(line)?,
            column: parse(column)?,
        })
    }
}

fn push_line(lines: &mut Vec<Line>, line: Line) -> Result<(), String> {
    if lines.len() >= MAX_SOURCE_COORDINATE_LINES {
        return Err("源码坐标超过 65,536 个物理行预算".into());
    }
    lines.push(line);
    Ok(())
}

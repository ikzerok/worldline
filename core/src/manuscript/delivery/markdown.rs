//! 只渲染已批准的审稿语义字段；不导出 ReviewSource.excerpt、绝对路径或源文件。
use super::*;
use crate::manuscript::{ReviewKind, ReviewNode};
use std::fmt::Write;

pub(super) fn header(scope: &ManuscriptDeliveryScope) -> Result<String, ManuscriptDeliveryError> {
    let mut output = String::new();
    let mut writer = Markdown {
        output: &mut output,
        limit: scope.request.limits.markdown_bytes,
    };
    writer.raw("# ")?;
    writer.text(&scope.title)?;
    writer.raw("\n\n作者私密审稿本 · 静态全分支 · 未执行\n\n")?;
    writer.raw("条件不求值，各个互斥分支和选择分别呈现；调用不展开。编排顺序不代表任何实际路线。未纳入未提交表单、未插入保留输入、批注、附件或运行时状态。\n\n")?;
    writer.field("书稿身份", &scope.request.query.manuscript_id)?;
    writer.field(
        "来源",
        match scope.source {
            ManuscriptQuerySource::Applied => "生成时的已应用工程稿",
            ManuscriptQuerySource::Draft => "生成时的当前稿，包含未应用正文或编排草稿",
        },
    )?;
    writer.field("快照", &scope.snapshot_key)?;
    for (name, value) in [
        ("文字筛选", scope.request.query.text.as_str()),
        ("状态筛选", scope.request.query.status.as_str()),
        ("POV筛选", scope.request.query.pov.as_str()),
        (
            "分节身份",
            scope
                .request
                .query
                .section_id
                .as_deref()
                .unwrap_or("全部分节"),
        ),
    ] {
        writer.field(name, if value.is_empty() { "未限制" } else { value })?;
    }
    writer.field(
        "范围",
        &format!(
            "匹配 {} / 可识别 {} 章；选择 {} 个编排出现；{} 个唯一源；{} 个重复源出现",
            scope.matching_chapters,
            scope.recognized_chapters,
            scope.selected_occurrences,
            scope.unique_sources,
            scope.repeated_source_occurrences
        ),
    )?;
    writer.raw("\n## 选择读序\n\n")?;
    if scope.chapters.is_empty() {
        writer.raw("本次选择为零章；这不表示整部作品没有问题。\n\n")?;
    }
    for (index, row) in scope.chapters.iter().enumerate() {
        writer.raw(&format!("{}. ", index + 1))?;
        writer.text(&row.entry.title)?;
        writer.raw(" · 章 ")?;
        writer.text(&row.entry.id)?;
        writer.raw(&format!(" · 全书位置 {}", row.ordinal + 1))?;
        writer.raw("\n")?;
    }
    writer.raw("\n")?;
    Ok(output)
}

pub(super) fn append_chapter(
    output: &mut String,
    scope: &ManuscriptDeliveryScope,
    occurrence: usize,
    review: &ReviewProjection,
) -> Result<(), ManuscriptDeliveryError> {
    let row = &scope.chapters[occurrence];
    let mut writer = Markdown {
        output,
        limit: scope.request.limits.markdown_bytes,
    };
    writer.raw(&format!("## {}. ", occurrence + 1))?;
    writer.text(&row.entry.title)?;
    writer.raw("\n\n")?;
    writer.field("章身份", &row.entry.id)?;
    writer.field(
        "源身份",
        &format!("{}:{}", review.target.kind, review.target.id),
    )?;
    writer.field("全书位置", &(row.ordinal + 1).to_string())?;
    if !row.section_path.is_empty() {
        writer.raw("- 分节路径：")?;
        for (index, section) in row.section_path.iter().enumerate() {
            if index > 0 {
                writer.raw(" / ")?;
            }
            writer.text(&section.title)?;
            writer.raw("（")?;
            writer.text(&section.id)?;
            writer.raw("）")?;
        }
        writer.raw("\n")?;
    }
    let count = scope
        .chapters
        .iter()
        .filter(|other| other.entry.target_ref.as_ref() == Some(&review.target))
        .count();
    if count > 1 {
        writer.field(
            "重复来源",
            &format!("此源在当前范围出现{count}次；本次按编排位置完整保留"),
        )?;
    }
    writer.raw("\n")?;
    for (index, node) in review.nodes.iter().enumerate() {
        writer.node(node, &(index + 1).to_string())?;
    }
    Ok(())
}

struct Markdown<'a> {
    output: &'a mut String,
    limit: usize,
}
impl Markdown<'_> {
    fn raw(&mut self, value: &str) -> Result<(), ManuscriptDeliveryError> {
        if value.len() > self.limit.saturating_sub(self.output.len()) {
            return Err(ManuscriptDeliveryError::limit(
                "Markdown超过完整材料字节预算；未交付截断文字",
            ));
        }
        self.output.push_str(value);
        Ok(())
    }
    fn text(&mut self, value: &str) -> Result<(), ManuscriptDeliveryError> {
        for character in value.chars() {
            match character {
                '&' => self.raw("&amp;")?,
                '<' => self.raw("&lt;")?,
                '>' => self.raw("&gt;")?,
                '\\' | '`' | '*' | '_' | '[' | ']' | '{' | '}' | '(' | ')' | '#' | '+' | '-'
                | '.' | '!' | '|' => {
                    self.raw("\\")?;
                    let mut bytes = [0; 4];
                    self.raw(character.encode_utf8(&mut bytes))?;
                }
                '\r' => self.raw("&#13;")?,
                '\n' => self.raw("\n")?,
                value if value.is_control() => {
                    let mut encoded = String::new();
                    let _ = write!(encoded, "&#{};", value as u32);
                    self.raw(&encoded)?;
                }
                value => {
                    let mut bytes = [0; 4];
                    self.raw(value.encode_utf8(&mut bytes))?;
                }
            }
        }
        Ok(())
    }
    fn field(&mut self, name: &str, value: &str) -> Result<(), ManuscriptDeliveryError> {
        self.raw("- ")?;
        self.raw(name)?;
        self.raw("：")?;
        self.text(value)?;
        self.raw("\n")
    }
    fn node(&mut self, node: &ReviewNode, number: &str) -> Result<(), ManuscriptDeliveryError> {
        // 穷尽匹配：新 ReviewKind 必须明确选择静态作者语义，不能隐式跳过分支。
        let label = match node.kind {
            ReviewKind::Text => "正文",
            ReviewKind::Say => "对白",
            ReviewKind::If => "条件组 · 按次序择一",
            ReviewKind::Branch => "互斥分支",
            ReviewKind::ChoiceGroup => "选择组 · 分别审阅",
            ReviewKind::Choice => "选择",
            ReviewKind::Scene => "场景",
            ReviewKind::Call => "片段调用 · 未展开、未执行",
            ReviewKind::Return => "返回调用处",
            ReviewKind::Divert => "流程去向",
            ReviewKind::Structure => "结构声明 · 未执行",
            ReviewKind::Description => "实体描述",
        };
        self.raw("**")?;
        self.raw(number)?;
        self.raw(" · ")?;
        self.raw(label)?;
        self.raw("**\n\n")?;
        if !node.label.is_empty() {
            if matches!(
                node.kind,
                ReviewKind::Structure | ReviewKind::Call | ReviewKind::Divert
            ) {
                self.text(crate::lexer::strip_comments(&node.label).trim_end())?;
            } else {
                self.text(&node.label)?;
            }
            self.raw("\n\n")?;
        }
        if let Some(speaker) = &node.speaker {
            self.field(
                "说话者",
                &format!(
                    "{}（{}:{}）",
                    speaker.display, speaker.target.kind, speaker.target.id
                ),
            )?;
        }
        for (name, value) in [
            ("条件（未求值）", &node.condition),
            ("可选条件（未求值）", &node.enable),
        ] {
            if let Some(value) = value {
                self.field(name, crate::lexer::strip_comments(value).trim_end())?;
            }
        }
        if let Some(reason) = &node.disabled_reason {
            self.field("不可选说明", reason)?;
        }
        if node.once {
            self.field("一次性选择", "未判定是否已选")?;
        }
        if let Some(target) = &node.target {
            self.field("目标身份", &format!("{}:{}", target.kind, target.id))?;
        }
        for part in &node.parts {
            self.text(&part.text)?;
        }
        if !node.parts.is_empty() {
            self.raw("\n\n")?;
        }
        for part in &node.parts {
            if let Some(target) = &part.target {
                self.field(
                    "正文链接身份",
                    &format!("{} → {}:{}", part.text, target.kind, target.id),
                )?;
            }
            if part.dynamic {
                self.field("动态内容", "仅保留标记，未求值")?;
            }
        }
        if node.glue {
            self.field("粘接", "保留源粘接标记，不跨分支合并")?;
        }
        for (index, child) in node.children.iter().enumerate() {
            self.node(child, &format!("{number}.{}", index + 1))?;
        }
        if let Some(end) = &node.end_label {
            self.raw("结束 ")?;
            self.raw(number)?;
            self.raw("：")?;
            self.text(end)?;
            self.raw("\n\n")?;
        }
        Ok(())
    }
}

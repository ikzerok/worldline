use super::{PlaythroughReport, PlaythroughReportError, PlaythroughSource};
use crate::RouteStatus;

struct Writer {
    text: String,
    limit: usize,
}
impl Writer {
    fn raw(&mut self, text: &str) -> Result<(), PlaythroughReportError> {
        if self.text.len().saturating_add(text.len()) > self.limit {
            return Err(PlaythroughReportError::new(
                "output_limit",
                "转义后的Markdown超过输出额度",
            ));
        }
        self.text.push_str(text);
        Ok(())
    }
    fn value(&mut self, text: &str) -> Result<(), PlaythroughReportError> {
        let mut leading = true;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '&' => self.raw("&amp;")?,
                '<' => self.raw("&lt;")?,
                '>' => self.raw("&gt;")?,
                '"' => self.raw("&quot;")?,
                '\'' => self.raw("&#39;")?,
                '\n' => self.raw("  \n")?,
                '\r' => self.raw("&#13;")?,
                '\t' => self.raw("&#9;")?,
                ':' if chars.peek() == Some(&'/') => self.raw("&#58;")?,
                ' ' if leading => self.raw("&#32;")?,
                '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-'
                | '.' | '!' | '|' | '~' | '=' => {
                    self.raw("\\")?;
                    self.raw(ch.encode_utf8(&mut [0; 4]))?;
                }
                ch if ch.is_control() => self.raw("�")?,
                ch => self.raw(ch.encode_utf8(&mut [0; 4]))?,
            }
            leading = ch == '\n' || (leading && ch == ' ');
        }
        Ok(())
    }
    fn number(&mut self, number: impl std::fmt::Display) -> Result<(), PlaythroughReportError> {
        self.value(&number.to_string())
    }
    fn source(&mut self, source: Option<&PlaythroughSource>) -> Result<(), PlaythroughReportError> {
        if let Some(source) = source {
            self.value(&source.file)?;
            self.raw(" · 行 ")?;
            self.number(source.line)?;
            self.raw(" 列 ")?;
            self.number(source.column)?;
            self.raw("（语句位置）")
        } else {
            self.raw("来源不可用，不推测替代位置")
        }
    }
}
pub(super) fn render(
    report: &PlaythroughReport,
    maximum: usize,
) -> Result<String, PlaythroughReportError> {
    let mut out = Writer {
        text: String::new(),
        limit: maximum,
    };
    out.raw("# 试玩审阅记录\n\n作者私密交接副本：正文、选择、说话者与源码文件名可能含私人信息，请先审阅再分享。此文件不是读者发布授权，也不会自动公开世界资料或附件。\n\n## 验证摘要\n\n- 结果：")?;
    out.raw(status(report))?;
    out.raw("\n- 工具/runtime版本：")?;
    out.value(&report.runtime_version)?;
    out.raw("；审阅schema：")?;
    out.number(report.schema_version)?;
    out.raw("\n- 语言版本：")?;
    out.value(report.compile_options.language_version.as_str())?;
    out.raw("；编译选项：object_refs=")?;
    out.number(report.compile_options.object_refs)?;
    out.raw("，character_refs=")?;
    out.number(report.compile_options.character_refs)?;
    out.raw("，localization_ids=")?;
    out.number(report.compile_options.localization_ids)?;
    out.raw("\n- 本次验证额度：")?;
    out.number(report.limits.budget.max_steps)?;
    out.raw(" 步 / ")?;
    out.number(report.limits.budget.time_budget_ms)?;
    out.raw(" ms；最终DTO上限 ")?;
    out.number(report.limits.max_output_bytes)?;
    out.raw(" bytes")?;
    out.raw("\n- 当前稿fingerprint：")?;
    out.number(report.source_fingerprint)?;
    out.raw("；原trace fingerprint：")?;
    out.number(report.original_fingerprint)?;
    out.raw("\n- 已应用源码快照：")?;
    out.value(&report.source_snapshot)?;
    out.raw("（FNV-1a-64版本识别摘要，非密码学签名）\n- 生成时间（Unix毫秒）：")?;
    if let Some(time) = report.generated_at_unix_ms {
        out.number(time)?;
    } else {
        out.raw("不可用")?;
    }
    out.raw("\n- 起点：")?;
    out.value(&report.origin.kind)?;
    out.raw("；seed：")?;
    out.number(report.origin.seed)?;
    out.raw("；工程入口：")?;
    out.value(&report.entry)?;
    if let Some(digest) = &report.origin.checkpoint_digest {
        out.raw("\n- 检查点摘要：")?;
        out.value(digest)?;
        out.raw("；继承访问节点：")?;
        out.number(report.inherited_visited_nodes)?;
        out.raw("；继承选择次数：")?;
        out.number(report.inherited_selected_choices)?;
        out.raw("。检查点之前的经历未由本次验证")?;
    }
    out.raw("\n- 验证范围：")?;
    out.number(report.observations.len())?;
    out.raw(" 个匹配成功的观察，")?;
    out.number(report.verified_choices)?;
    out.raw(" 个实际选择，")?;
    out.number(report.executed_steps)?;
    out.raw(" 个解释器步骤。未访问内容均为未探索，不表示错误或不可达\n- 仅当前已应用稿；不包含未应用编辑输入。说话者展示名来自验证快照\n- 仅披露实际状态操作/全局赋值次数；不含变量名和值、调用局部值、任意状态JSON、作者备注、资料、标签元数据或附件\n")?;
    if let Some(step) = report.divergence_step {
        out.raw("- 首个分歧观察：")?;
        out.number(step)?;
        out.raw("；该观察及其后正文未列入验证结果\n")?;
    }
    out.raw("\n## 已验证区段\n")?;
    if report.observations.is_empty() {
        out.raw("\n尚无匹配成功的观察，不能视为通过\n")?;
    }
    for observation in &report.observations {
        out.raw("\n### 观察 ")?;
        out.number(observation.index)?;
        out.raw("\n\n")?;
        if let Some(choice) = &observation.choice {
            out.raw("已选择：")?;
            out.value(&choice.label)?;
            out.raw("\n\n选择来源：")?;
            out.source(choice.source.as_ref())?;
            out.raw("\n\n")?;
        }
        for (index, text) in observation.texts.iter().enumerate() {
            if index > 0 && text.new_line {
                out.raw("\n\n")?;
            }
            if let Some(label) = &text.speaker_label {
                out.value(label)?;
                out.raw("：")?;
            } else if let Some(speaker) = &text.speaker {
                out.value(&speaker.id)?;
                out.raw("：")?;
            }
            out.value(&text.content)?;
        }
        out.raw("\n\n来源（按正文片段顺序）：\n")?;
        for (index, text) in observation.texts.iter().enumerate() {
            out.raw("- ")?;
            out.number(index + 1)?;
            out.raw("：")?;
            out.source(text.source.as_ref())?;
            out.raw("\n")?;
        }
        out.raw("\n本观察区段实际状态操作 ")?;
        out.number(observation.state_actions)?;
        out.raw(" 次；全局赋值 ")?;
        out.number(observation.variable_writes)?;
        out.raw(" 次（同值赋值也计数；不含入口初始化）\n")?;
        if observation.ended {
            out.raw("\n此观察已实际到达故事终态\n")?;
        }
    }
    if let Some(choice) = &report.pending_choice {
        out.raw("\n## 最后已选输入（其后观察未验证）\n\n")?;
        out.value(&choice.label)?;
        out.raw("\n\n")?;
        out.source(choice.source.as_ref())?;
        out.raw("\n")?;
    }
    out.raw("\n## 当前已应用源码清单\n\n相对路径基准：活动源码集合的共同父目录；不公开机器绝对路径。仅列路径、字节数与内容摘要，不包含源文件正文。\n\n")?;
    for file in &report.source_manifest {
        out.raw("- ")?;
        out.value(&file.file)?;
        out.raw(" · ")?;
        out.number(file.bytes)?;
        out.raw(" UTF-8 bytes · ")?;
        out.value(&file.digest)?;
        out.raw("\n")?;
    }
    Ok(out.text)
}
fn status(report: &PlaythroughReport) -> &'static str {
    match report.status {
        RouteStatus::Replayed if report.complete && report.ended => {
            "完整结束并验证通过（所记录区段）"
        }
        RouteStatus::Replayed => "已记录区段验证通过，路线未完整结束或未声明完成",
        RouteStatus::Diverged => "分歧；仅下列匹配前缀已验证",
        RouteStatus::IncompleteTrace => "记录不完整；仅下列观察已验证",
        RouteStatus::StepBudgetExceeded => "解释器步数额度耗尽；未完成验证",
        RouteStatus::TimeBudgetExceeded => "验证时限耗尽；未完成验证",
        RouteStatus::Cancelled => "已取消；未完成验证",
        RouteStatus::StoryFailed => "故事执行失败；未完成验证",
        RouteStatus::OutputBudgetExceeded => "运行输出额度耗尽；未完成验证",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dynamic_markdown_is_literal_but_plain_words_remain_readable() {
        let input = "hello world: <script>x</script> ![image](https://evil.invalid) **bold** `code` <https://evil.invalid> https://evil.invalid www.evil.invalid\n    # heading\nplain\n===\n<script>";
        let mut writer = Writer {
            text: String::new(),
            limit: 4096,
        };
        writer.value(input).unwrap();
        assert!(writer.text.starts_with("hello world: "));
        for active in [
            "<script>", "![image]", "**bold**", "`code`", "https://", "www.evil", "\n    #",
            "\n===", "\n===",
        ] {
            assert!(!writer.text.contains(active), "{active}: {}", writer.text);
        }
        assert!(writer.text.contains("&lt;script&gt;"));
        let mut tiny = Writer {
            text: String::new(),
            limit: 3,
        };
        assert_eq!(tiny.value("<").unwrap_err().code, "output_limit");
    }
}

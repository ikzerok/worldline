//! 诊断模型 —— 规范见 `worldline/spec/diagnostics.md`。
//! 诊断只由 worldline-core 产出,CLI 与编辑器只做展示。

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Hint,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Severity::Error => "错误",
            Severity::Warning => "警告",
            Severity::Hint => "提示",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Hint => "hint",
        })
    }
}

/// 1-based 行,1-based 列(字符计),length 为字符数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: u32,
    pub column: u32,
    pub length: u32,
}

impl Span {
    pub fn new(line: u32, column: u32, length: u32) -> Self {
        Span {
            line,
            column,
            length,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub file: String,
    pub span: Span,
    pub note: Option<String>,
    pub suggestion: Option<String>,
    pub related: Vec<(String, Span)>,
}

impl Diagnostic {
    pub fn error(code: &'static str, file: &str, span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Error,
            code,
            message: message.into(),
            file: file.to_string(),
            span,
            note: None,
            suggestion: None,
            related: Vec::new(),
        }
    }

    pub fn warning(code: &'static str, file: &str, span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            code,
            message: message.into(),
            file: file.to_string(),
            span,
            note: None,
            suggestion: None,
            related: Vec::new(),
        }
    }

    pub fn hint(code: &'static str, file: &str, span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Hint,
            code,
            message: message.into(),
            file: file.to_string(),
            span,
            note: None,
            suggestion: None,
            related: Vec::new(),
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub fn with_related(mut self, file: &str, span: Span) -> Self {
        self.related.push((file.to_string(), span));
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {} [{}]: {}",
            self.file,
            self.span.line,
            self.span.column,
            self.severity.label(),
            self.code,
            self.message
        )
    }
}

/// 规范定义的排序:severity 降序 → 行 → 列 → code。
pub fn sort_diagnostics(diags: &mut [Diagnostic]) {
    diags.sort_by(|a, b| {
        let sev = (b.severity).cmp(&a.severity);
        sev.then(a.span.line.cmp(&b.span.line))
            .then(a.span.column.cmp(&b.span.column))
            .then(a.code.cmp(b.code))
    });
}

/// 兼容 serde 手写序列化的 JSON 视图(`wl check --json`)。
pub mod json {
    use super::{Diagnostic, Severity, Span};
    use serde::ser::{Serialize, SerializeStruct, Serializer};

    impl Serialize for Span {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let mut st = s.serialize_struct("Span", 3)?;
            st.serialize_field("line", &self.line)?;
            st.serialize_field("column", &self.column)?;
            st.serialize_field("length", &self.length)?;
            st.end()
        }
    }

    impl Serialize for Severity {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.serialize_str(match self {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Hint => "hint",
            })
        }
    }

    impl Serialize for Diagnostic {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let mut st = s.serialize_struct("Diagnostic", 8)?;
            st.serialize_field("severity", &self.severity)?;
            st.serialize_field("code", &self.code)?;
            st.serialize_field("message", &self.message)?;
            st.serialize_field("file", &self.file)?;
            st.serialize_field("span", &self.span)?;
            st.serialize_field("note", &self.note)?;
            st.serialize_field("suggestion", &self.suggestion)?;
            st.serialize_field("related", &self.related)?;
            st.end()
        }
    }
}

//! 模板默认强引用参与删除影响；不把模板标签、提示或未知扩展当作引用。
use super::{ProjectTemplateField, ProjectTemplateIndex};
use crate::catalog::{ReferenceInfo, TargetRef};
impl ProjectTemplateIndex {
    /// 从当前缓存直接投影模板默认值引用，不读取磁盘、解析清单或重新编译。
    pub fn references_to(&self, target: &TargetRef) -> Vec<ReferenceInfo> {
        let mut references = Vec::new();
        for document in self.projects.values() {
            let Some(template) = &document.template else {
                continue;
            };
            collect(
                &template.fields,
                target,
                &document.file,
                &document.source_bytes,
                &mut references,
            );
        }
        references
    }
}

fn collect(
    fields: &[ProjectTemplateField],
    target: &TargetRef,
    file: &str,
    bytes: &[u8],
    references: &mut Vec<ReferenceInfo>,
) {
    for field in fields {
        if field.field_type == "object_ref"
            && field.default.as_ref().is_some_and(|value| {
                value.get("kind").and_then(serde_json::Value::as_str) == Some(target.kind.as_str())
                    && value.get("id").and_then(serde_json::Value::as_str)
                        == Some(target.id.as_str())
            })
        {
            let text = String::from_utf8_lossy(bytes);
            let needle = crate::authoring::quote(&field.id);
            let line = text.find(&needle).map_or(1, |at| {
                text[..at].bytes().filter(|byte| *byte == b'\n').count() as u32 + 1
            });
            references.push(ReferenceInfo {
                source: TargetRef::new("file", file),
                target: target.clone(),
                kind: "模板默认值引用".into(),
                file: file.into(),
                line,
            });
        }
        collect(&field.fields, target, file, bytes, references);
    }
}

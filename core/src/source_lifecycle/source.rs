use super::*;
use crate::refactor::preview::{apply, Edit};
use std::path::Path;

pub(super) fn rewrite(
    project: &Project,
    old: &Path,
    new: &Path,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(Vec<SourceLifecycleChange>, Vec<SourceLifecycleResource>), String> {
    let mut changes = Vec::new();
    let mut resources = Vec::new();
    let mut digests = std::collections::BTreeMap::<std::path::PathBuf, String>::new();
    let mut resource_bytes = 0usize;
    for (path, document) in &project.documents {
        if document.is_deleted() {
            continue;
        }
        check_cancelled(cancelled)?;
        let after_path = if path == old { new } else { path };
        let source = &document.text;
        let mut diagnostics = Vec::new();
        crate::lexer::lex_source_with_options(
            &path.to_string_lossy(),
            source,
            &mut diagnostics,
            project.compile_options(),
        );
        if diagnostics
            .iter()
            .any(|item| item.severity == crate::Severity::Error)
        {
            return Err(format!(
                "源码含无法证明完整路径语义的词法错误：{}",
                path.display()
            ));
        }
        let spans = crate::lexer::path_source_spans(
            &path.to_string_lossy(),
            source,
            project.compile_options(),
        );
        let mut edits = Vec::new();
        for span in spans {
            let target = resolve(project, path, &span.target.id)?;
            let moved_target = if target == old {
                new.to_path_buf()
            } else {
                target.clone()
            };
            let new_relative = crate::catalog_edit::relative_source_path(
                after_path.parent().ok_or("源码缺少目录")?,
                &moved_target,
            )?;
            if project
                .documents
                .get(&target)
                .is_some_and(|document| document.is_deleted())
            {
                return Err(format!(
                    "正式路径指向待删除源码，不能从旧磁盘内容补回：{}",
                    target.display()
                ));
            }
            let content_digest = if let Some(digest) = digests.get(&target) {
                digest.clone()
            } else {
                let bytes = super::resource_bytes(project, &target)?;
                resource_bytes = resource_bytes.saturating_add(bytes.len());
                if resource_bytes > 256 * 1024 * 1024 {
                    return Err("源码组织资源超过 256 MiB 验证预算，工程未修改".into());
                }
                let hash = digest(&bytes);
                digests.insert(target.clone(), hash.clone());
                hash
            };
            if span.target.kind == "include_path" && !project.documents.contains_key(&target) {
                return Err(format!(
                    "include 源码未载入，无法证明完整移动：{}",
                    target.display()
                ));
            }
            if span.target.kind == "asset_path" && target == old {
                return Err(
                    "素材声明把待移动源码作为原始附件；无法同时保证资源字节与路径语义，暂不支持"
                        .into(),
                );
            }
            let affected = path == old || target == old;
            let after_relative = if affected {
                new_relative
            } else {
                span.target.id.clone()
            };
            if resources.len() >= 16384 {
                return Err("正式路径超过 16384 项预览预算，工程未修改".into());
            }
            resources.push(SourceLifecycleResource {
                source: path.clone(),
                field: span.field.clone(),
                before_path: span.target.id.clone(),
                after_path: after_relative.clone(),
                resolved_before: target,
                resolved_after: moved_target,
                content_digest,
            });
            if affected && after_relative != span.target.id {
                let replacement = if span.field.contains(".link.") {
                    if after_relative.contains(['[', ']', '{', '}', '\\', '"', '#', '~', '|']) {
                        return Err(
                            "目标路径不能安全写入正式正文链接，请改用不含链接分隔符的路径".into(),
                        );
                    }
                    after_relative
                } else {
                    let quoted = crate::authoring::quote(&after_relative);
                    // 路径声明的官方语法使用引号；不保留无法承载空格的裸 token。
                    let quoted_before =
                        source.as_bytes().get(span.range.start.wrapping_sub(1)) == Some(&b'"');
                    if quoted_before {
                        quoted[1..quoted.len() - 1].into()
                    } else {
                        quoted
                    }
                };
                let direction = if path == old { "outbound" } else { "inbound" };
                edits.push(Edit {
                    range: span.range,
                    replacement,
                    field: format!("{direction}.{}", span.field),
                });
            }
        }
        let (after, occurrences) = apply(source, edits)?;
        // 所有正式路径再次经同一 lexer 投影，防止编码失误把链接降级为普通文字。
        let old_spans = crate::lexer::path_source_spans(
            &path.to_string_lossy(),
            source,
            project.compile_options(),
        );
        let new_spans = crate::lexer::path_source_spans(
            &after_path.to_string_lossy(),
            &after,
            project.compile_options(),
        );
        if old_spans.len() != new_spans.len() {
            return Err("候选正式路径数量变化，无法证明引用完整，工程未修改".into());
        }
        for (before, after) in old_spans.iter().zip(&new_spans) {
            let before_target = resolve(project, path, &before.target.id)?;
            let expected = if before_target == old {
                new.to_path_buf()
            } else {
                before_target
            };
            if before.field != after.field
                || before.target.kind != after.target.kind
                || resolve(project, after_path, &after.target.id)? != expected
            {
                return Err("候选正式路径解析目标变化，工程未修改".into());
            }
        }
        if after != *source || path == old {
            changes.push(SourceLifecycleChange {
                path: path.clone(),
                after_path: after_path.to_path_buf(),
                kind: "source".into(),
                occurrences,
                before: Some(source.as_bytes().to_vec()),
                after: Some(after.into_bytes()),
            });
        }
    }
    Ok((changes, resources))
}

fn resolve(project: &Project, from: &Path, relative: &str) -> Result<PathBuf, String> {
    if Path::new(relative).is_absolute() || relative.contains(['\\', ':', '\0', '\n', '\r']) {
        return Err("正式源码路径必须是工作区内相对路径".into());
    }
    crate::file_access::within(
        &project.root,
        &from.parent().ok_or("源码缺少目录")?.join(relative),
    )
}

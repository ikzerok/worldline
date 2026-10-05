//! 正式路径按真实来源映射逐项比较；作为原始附件的两侧源码不得改变字节。
use super::{entity_proof::LineMap, Failure, SourceLifecycleResource};
use crate::{lexer::IdentitySourceSpan, project::Project};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) fn prove(
    before: &Project,
    after: &Project,
    mapping: Option<&LineMap>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<SourceLifecycleResource>, Failure> {
    let mut expected = paths(before, cancelled)?;
    for (file, span) in &mut expected {
        if let Some(mapping) = mapping {
            mapping.location(file, &mut span.line);
        }
    }
    let actual = paths(after, cancelled)?;
    let key = |(file, span): &(String, IdentitySourceSpan)| {
        (
            file.clone(),
            span.line,
            span.field.clone(),
            span.target.clone(),
        )
    };
    expected.sort_by_key(&key);
    let mut actual = actual;
    actual.sort_by_key(&key);
    if expected.iter().map(&key).collect::<Vec<_>>() != actual.iter().map(&key).collect::<Vec<_>>()
    {
        return Err(Failure::semantic(
            "移源改变正式路径 token、引用次数或来源，工程未修改",
        ));
    }
    let originals = paths(before, cancelled)?;
    let mut resources = Vec::new();
    let mut digests = BTreeMap::<PathBuf, String>::new();
    let mut total = 0usize;
    for (file, span) in originals {
        super::check_cancelled(cancelled)?;
        let resolved_before = resolve(before, Path::new(&file), &span.target.id)?;
        let mut mapped_file = file.clone();
        let mut mapped_line = span.line;
        if let Some(mapping) = mapping {
            mapping.location(&mut mapped_file, &mut mapped_line);
        }
        let resolved_after = resolve(after, Path::new(&mapped_file), &span.target.id)?;
        if resolved_before != resolved_after {
            return Err(Failure::semantic(
                "移源会改变相对资源/文件引用的解析目标，工程未修改",
            ));
        }
        let hash = if let Some(hash) = digests.get(&resolved_before) {
            hash.clone()
        } else {
            let bytes = super::resource_bytes(before, &resolved_before)?;
            total = total.saturating_add(bytes.len());
            if total > 256 * 1024 * 1024 {
                return Err("移源资源超过 256 MiB 验证预算".into());
            }
            let hash = super::digest(&bytes);
            digests.insert(resolved_before.clone(), hash.clone());
            hash
        };
        if span.target.kind == "asset_path"
            && super::digest(&super::resource_bytes(after, &resolved_after)?) != hash
        {
            return Err(Failure::semantic(
                "源或目标源码被当作原始附件；移源会改变资源字节，工程未修改",
            ));
        }
        resources.push(SourceLifecycleResource {
            source: PathBuf::from(file),
            field: span.field,
            before_path: span.target.id.clone(),
            after_path: span.target.id,
            resolved_before,
            resolved_after,
            content_digest: hash,
        });
    }
    Ok(resources)
}

fn paths(
    project: &Project,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<(String, IdentitySourceSpan)>, Failure> {
    let mut output = Vec::new();
    for (path, document) in &project.documents {
        if document.is_deleted() {
            continue;
        }
        super::check_cancelled(cancelled)?;
        let file = path.to_string_lossy();
        let spans =
            crate::lexer::path_source_spans(&file, &document.text, project.compile_options());
        if output.len() + spans.len() > 16384 {
            return Err("正式路径超过 16384 项预览预算".into());
        }
        output.extend(spans.into_iter().map(|span| (file.to_string(), span)));
    }
    Ok(output)
}

fn resolve(project: &Project, from: &Path, relative: &str) -> Result<PathBuf, Failure> {
    if Path::new(relative).is_absolute() || relative.contains(['\\', ':', '\0', '\n', '\r']) {
        return Err(Failure::path("正式路径必须是工作区内相对路径"));
    }
    crate::file_access::within(
        &project.root,
        &from.parent().ok_or("源码缺少目录")?.join(relative),
    )
    .map_err(Failure::path)
}

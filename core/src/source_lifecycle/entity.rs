//! 单 entity 的私有候选；不会重写 include、资料或已登记文档。
use super::{entity_proof::LineMap, *};
use crate::refactor::preview::{apply, Edit};
use std::path::Path;

pub(super) struct PreparedEntityMove {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub membership: String,
    pub changes: Vec<SourceLifecycleChange>,
    pub resources: Vec<SourceLifecycleResource>,
}

pub(super) fn prepare(
    project: &Project,
    candidate: &mut Project,
    before: &crate::CompileResult,
    id: &str,
    to: &Path,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<PreparedEntityMove, Failure> {
    if before.has_errors() {
        return Err("移动实体资料声明需要当前活动源码编译通过，工程未修改".into());
    }
    if !crate::lexer::valid_identifier(id) {
        return Err("entity 稳定 ID 无效，工程未修改".into());
    }
    let entity = before
        .analysis
        .catalog
        .entities
        .get(id)
        .ok_or("当前活动稿没有此 entity 声明")?;
    let source = PathBuf::from(&entity.file);
    safety::native_relative(to).map_err(Failure::path)?;
    let destination =
        crate::file_access::within(&project.root, &project.root.join(to)).map_err(Failure::path)?;
    for path in [&source, &destination] {
        project.document(path).map_err(Failure::path)?;
        if !before
            .program
            .files
            .iter()
            .any(|file| Path::new(file) == path)
            || project
                .source_selection()
                .is_some_and(|selection| !selection.is_active(path))
        {
            return Err(Failure::path(
                "实体来源和目标必须是已跟踪、非删除的活动源码",
            ));
        }
        safety::writable_path(path).map_err(Failure::path)?;
    }
    super::registered::validate_candidate(project, project)?;
    let original = project.document(&source)?;
    let range = entity_span::declaration(
        &entity.file,
        original,
        id,
        entity.line,
        project.compile_options(),
    )?;
    let mut changes = Vec::new();
    let mut mapping = None;
    if source != destination {
        let target = project.document(&destination)?;
        let block = &original[range.clone()];
        let (insertion, inserted_byte) = entity_span::insertion(target, block)?;
        let before_insert = format!("{}{}", target, &insertion[..inserted_byte - target.len()]);
        mapping = Some(LineMap {
            source: source.to_string_lossy().into_owned(),
            destination: destination.to_string_lossy().into_owned(),
            first: entity.line,
            last: entity.line + block.lines().count() as u32 - 1,
            inserted: before_insert.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1,
            removed_newlines: block.bytes().filter(|byte| *byte == b'\n').count() as u32,
        });
        changes.push(change(
            &source,
            original,
            Edit {
                range,
                replacement: String::new(),
                field: "entity.declaration.remove".into(),
            },
        )?);
        changes.push(change(
            &destination,
            target,
            Edit {
                range: target.len()..target.len(),
                replacement: insertion,
                field: "entity.declaration.insert".into(),
            },
        )?);
        for change in &changes {
            candidate.set_text(
                &change.path,
                String::from_utf8(change.after.clone().ok_or("移源候选缺少源码")?)
                    .map_err(|_| "移源候选不是 UTF-8")?,
            )?;
        }
        check_cancelled(cancelled)?;
        let after = candidate.compile_current();
        entity_proof::equivalent(before, &after, mapping.as_ref().unwrap())?;
        super::registered::validate_candidate(project, candidate)?;
    }
    if project.compile_options() != candidate.compile_options()
        || project.source_selection() != candidate.source_selection()
    {
        return Err(Failure::semantic("移源不能改变活动成员、语言版本或能力"));
    }
    let resources = entity_resources::prove(project, candidate, mapping.as_ref(), cancelled)?;
    Ok(PreparedEntityMove {
        membership: super::plan::member(project, &source),
        source,
        destination,
        changes,
        resources,
    })
}

fn change(path: &Path, source: &str, edit: Edit) -> Result<SourceLifecycleChange, Failure> {
    let (after, occurrences) = apply(source, vec![edit])?;
    Ok(SourceLifecycleChange {
        path: path.into(),
        after_path: path.into(),
        kind: "source".into(),
        occurrences,
        before: Some(source.as_bytes().to_vec()),
        after: Some(after.into_bytes()),
    })
}

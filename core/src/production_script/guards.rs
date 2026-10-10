use super::*;
use crate::ast::Stmt;

/// 在共享递归本地化收集器之前验证一次；来源导航只接收该不可变快照。
pub(super) fn ast_envelope(compiled: &crate::CompileResult) -> Result<(), ProductionError> {
    let mut pending: Vec<_> = compiled
        .program
        .events
        .iter()
        .map(|event| (event.body.as_slice(), 0))
        .chain(
            compiled
                .program
                .fragments
                .iter()
                .map(|fragment| (fragment.body.as_slice(), 0)),
        )
        .collect();
    let mut nodes = pending.len();
    while let Some((body, depth)) = pending.pop() {
        if depth > 64 {
            return Err(ProductionError::budget());
        }
        nodes = nodes.saturating_add(body.len());
        if nodes > 200_000 {
            return Err(ProductionError::budget());
        }
        for statement in body {
            match statement {
                Stmt::Scene(scene) => pending.push((&scene.body, depth + 1)),
                Stmt::Choice(choice) => pending.push((&choice.body, depth + 1)),
                Stmt::If(condition) => {
                    nodes = nodes.saturating_add(condition.branches.len());
                    if nodes > 200_000 {
                        return Err(ProductionError::budget());
                    }
                    pending.extend(
                        condition
                            .branches
                            .iter()
                            .map(|(_, body)| (body.as_slice(), depth + 1)),
                    );
                }
                _ => {}
            }
        }
        if pending.len() > 200_000 {
            return Err(ProductionError::budget());
        }
    }
    Ok(())
}

/// 每份不可变编译快照一个 raw PathBuf→SourceId 表，不逐语句复制路径。
pub(super) struct SourceIndex {
    pub ids: BTreeMap<PathBuf, u32>,
    pub bytes: usize,
}

/// 只限制制作台本：公开相对来源必须能够还原原始文件路径。
pub(super) fn source_index<'a>(
    root: &std::path::Path,
    paths: impl IntoIterator<Item = &'a std::path::Path>,
    limit: usize,
) -> Result<SourceIndex, ProductionError> {
    let mut displays = std::collections::BTreeSet::new();
    let mut ids = BTreeMap::new();
    let mut bytes = 0usize;
    for path in paths {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| ProductionError::source())?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(ProductionError::source());
        }
        let raw = relative.to_str().ok_or_else(ProductionError::source)?;
        // Windows 分隔符可以变为 /；Unix 文件名中的字面反斜杠不能被当作目录。
        let display = raw.replace('\\', "/");
        if root.join(&display) != path || path.to_str().is_none() {
            return Err(ProductionError::source());
        }
        let id = u32::try_from(ids.len()).map_err(|_| ProductionError::budget())?;
        bytes = bytes
            .checked_add(bounded_size(
                &(path, &display, id),
                limit.saturating_sub(bytes),
            )?)
            .ok_or_else(ProductionError::budget)?;
        if !displays.insert(display) || ids.insert(path.to_path_buf(), id).is_some() {
            return Err(ProductionError::source());
        }
    }
    Ok(SourceIndex { ids, bytes })
}

#[cfg(test)]
mod source_identity_tests {
    use super::*;
    #[test]
    fn repeated_raw_file_identity_is_rejected() {
        let root = PathBuf::from("workspace");
        let path = root.join("body.wl");
        let paths = [path.as_path(), path.as_path()];
        let error = source_index(&root, paths, 1024).err().unwrap();
        assert_eq!(error.code, "INVALID_SOURCE");
    }
}

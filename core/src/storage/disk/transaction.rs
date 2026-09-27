use super::*;
pub(crate) fn save(root: &Path, files: &[PendingFile]) -> Result<(), String> {
    let _lock = STORAGE_LOCK
        .lock()
        .map_err(|_| "保存锁不可用，未清理已有事务日志".to_string())?;
    if files.is_empty() {
        return Ok(());
    }
    ensure_root(root)?;
    if has_unresolved_transactions(root)? {
        return Err("工程存在未解决的保存事务，请重新打开并处理冲突".into());
    }

    let mut journal_files = Vec::with_capacity(files.len());
    for file in files {
        let relative = validate_target(root, &file.relative)?;
        let before = hash_optional(file.before.as_deref());
        let after = hash_optional(file.after.as_deref());
        let target = root.join(Path::new(&relative));
        let current = read_target(root, &target)?;
        if hash_optional(current.as_deref()) != before {
            return Err(format!("保存事务目标已发生外部修改:{}", target.display()));
        }
        journal_files.push(JournalFile {
            path: relative,
            before,
            after,
            payload: file.after.clone(),
        });
    }

    let id = transaction_id();
    let directory = transaction_root(root).join(&id);
    create_transaction_directory(root, &directory)?;
    let mut journal = Journal {
        version: JOURNAL_VERSION,
        status: Status::Prepared,
        files: journal_files,
    };
    // 先校验日志再使它可见，避免重复目标或错误 payload 生成恢复时
    // 才会拒绝的事务。
    validate_journal(root, &journal)?;
    write_journal(&directory, &journal)?;
    if failure_requested("prepare") {
        return Err("保存故障注入:prepare".into());
    }

    journal.status = Status::Applying;
    write_journal(&directory, &journal)?;
    apply(&directory, root, &journal, true)?;

    if failure_requested("commit") {
        return Err("保存故障注入:commit".into());
    }
    journal.status = Status::Committed;
    write_journal(&directory, &journal)?;

    if failure_requested("cleanup") {
        return Err("保存故障注入:cleanup".into());
    }
    cleanup_transaction(root, &directory)
}
fn apply(
    directory: &Path,
    root: &Path,
    journal: &Journal,
    inject_failure: bool,
) -> Result<(), String> {
    let total = journal.files.len();
    for index in 0..total {
        if inject_failure && failure_requested_at_replacement(index, total) {
            return Err(format!("保存故障注入:replacement:{index}"));
        }
        apply_one(directory, root, journal, index, inject_failure)?;
    }
    Ok(())
}

pub(super) fn apply_one(
    directory: &Path,
    root: &Path,
    journal: &Journal,
    index: usize,
    inject_failure: bool,
) -> Result<(), String> {
    let file = &journal.files[index];
    let target = root.join(Path::new(&file.path));
    validate_target(root, Path::new(&file.path))?;
    let current = read_target(root, &target)?;
    let current_hash = hash_optional(current.as_deref());
    if current_hash == file.after {
        return Ok(());
    }
    if current_hash != file.before {
        return Err(format!("保存事务目标已发生外部修改:{}", target.display()));
    }

    match &file.payload {
        Some(payload) => {
            let temporary = directory.join(format!("payload-{index}.tmp"));
            if inject_failure && failure_requested("temp") {
                return Err("保存故障注入:temp".into());
            }
            write_payload(&temporary, payload)?;
            replace_file(&temporary, &target, root)?;
        }
        None => {
            if target.exists() {
                remove_target(root, &target)?;
            }
        }
    }
    Ok(())
}

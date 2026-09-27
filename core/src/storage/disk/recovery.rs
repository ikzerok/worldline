use super::*;
pub(crate) fn has_unresolved_transactions(root: &Path) -> Result<bool, String> {
    let parent = transaction_root(root);
    let metadata = match fs::symlink_metadata(&parent) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("无法读取保存事务目录:{}", error)),
    };
    if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
        return Err(format!(
            "保存事务目录不能是链接或普通文件:{}",
            parent.display()
        ));
    }
    validate_existing_directory_chain(root, &parent)?;
    let mut has_entry = false;
    for entry in fs::read_dir(&parent).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
            return Err(format!(
                "保存事务目录包含无效条目:{}",
                entry.path().display()
            ));
        }
        has_entry = true;
    }
    Ok(has_entry)
}
pub(crate) fn recovery_drafts(root: &Path) -> Result<Vec<crate::recovery::RecoveryDraft>, String> {
    let _lock = STORAGE_LOCK
        .lock()
        .map_err(|_| "保存锁不可用".to_string())?;
    if !has_unresolved_transactions(root)? {
        return Ok(Vec::new());
    }
    let mut directories = fs::read_dir(transaction_root(root))
        .map_err(|error| error.to_string())?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    directories.sort();
    let mut drafts = Vec::new();
    for directory in directories {
        validate_existing_directory_chain(root, &directory)?;
        let path = directory.join(JOURNAL_NAME);
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_file() {
            return Err("救援事务日志不能是链接或目录".into());
        }
        let journal: Journal =
            serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?)
                .map_err(|error| format!("救援事务日志无效：{error}"))?;
        validate_journal(root, &journal)?;
        let transaction = directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("事务名称不是有效 UTF-8")?
            .to_owned();
        for file in journal.files {
            let current = read_target(root, &root.join(&file.path))?;
            drafts.push(crate::recovery::RecoveryDraft {
                transaction: transaction.clone(),
                path: PathBuf::from(file.path),
                before_hash: file.before,
                current_hash: hash_optional(current.as_deref()),
                after_hash: file.after,
                bytes: file.payload,
            });
        }
    }
    Ok(drafts)
}
pub(crate) fn recover(root: &Path) -> Result<RecoveryReport, String> {
    let _lock = STORAGE_LOCK
        .lock()
        .map_err(|_| "保存锁不可用，保留未完成事务日志".to_string())?;
    let parent = transaction_root(root);
    let metadata = match fs::symlink_metadata(&parent) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RecoveryReport::default())
        }
        Err(error) => return Err(format!("无法读取保存事务目录:{}", error)),
    };
    if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
        return Err(format!(
            "保存事务目录不能是链接或普通文件:{}",
            parent.display()
        ));
    }
    validate_existing_directory_chain(root, &parent)?;

    let mut directories = Vec::new();
    for entry in fs::read_dir(&parent).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
            return Err(format!(
                "保存事务目录包含无效条目:{}",
                entry.path().display()
            ));
        }
        directories.push(entry.path());
    }
    directories.sort();

    let mut report = RecoveryReport::default();
    for directory in directories {
        let recovered = recover_one(root, &directory)?;
        report.conflicts.extend(recovered.conflicts);
        report.recovered.extend(recovered.recovered);
    }
    remove_empty_transaction_root(&parent)?;
    Ok(report)
}
fn recover_one(root: &Path, directory: &Path) -> Result<RecoveryReport, String> {
    let metadata = fs::symlink_metadata(directory).map_err(|error| error.to_string())?;
    if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
        return Err(format!("保存事务目录不能是链接:{}", directory.display()));
    }
    validate_existing_directory_chain(root, directory)?;
    if fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .next()
        .is_none()
    {
        // 提交清理最后一步崩溃可能只留下空目录；它不含作者数据，
        // 可以安全清理并继续打开工程。
        fs::remove_dir(directory).map_err(|error| error.to_string())?;
        sync_directory(directory.parent().unwrap_or(root))?;
        return Ok(RecoveryReport::default());
    }
    let journal_path = directory.join(JOURNAL_NAME);
    let journal_metadata = match fs::symlink_metadata(&journal_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let temporary = directory.join(format!("{JOURNAL_NAME}.tmp"));
            let metadata = fs::symlink_metadata(&temporary).map_err(|temporary_error| {
                format!(
                    "无法读取保存事务日志:{} ({temporary_error})",
                    journal_path.display()
                )
            })?;
            if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_file() {
                return Err(format!("保存事务日志不是普通文件:{}", temporary.display()));
            }
            let bytes = fs::read(&temporary).map_err(|read_error| {
                format!(
                    "无法读取保存事务日志:{} ({read_error})",
                    temporary.display()
                )
            })?;
            let journal: Journal = serde_json::from_slice(&bytes).map_err(|parse_error| {
                format!(
                    "保存事务日志格式错误:{} ({parse_error})",
                    temporary.display()
                )
            })?;
            validate_journal(root, &journal)?;
            fs::rename(&temporary, &journal_path).map_err(|rename_error| {
                format!(
                    "无法恢复保存事务日志:{} ({rename_error})",
                    journal_path.display()
                )
            })?;
            fs::symlink_metadata(&journal_path).map_err(|metadata_error| {
                format!(
                    "无法读取保存事务日志:{} ({metadata_error})",
                    journal_path.display()
                )
            })?
        }
        Err(error) => {
            return Err(format!(
                "无法读取保存事务日志:{} ({error})",
                journal_path.display()
            ))
        }
    };
    if crate::file_access::is_link_or_junction(&journal_metadata) || !journal_metadata.is_file() {
        return Err(format!(
            "保存事务日志不是普通文件:{}",
            journal_path.display()
        ));
    }
    let journal_bytes = fs::read(&journal_path)
        .map_err(|error| format!("无法读取保存事务日志:{} ({error})", journal_path.display()))?;
    let mut journal: Journal = serde_json::from_slice(&journal_bytes)
        .map_err(|error| format!("保存事务日志格式错误:{} ({error})", journal_path.display()))?;
    validate_journal(root, &journal)?;
    if journal.status == Status::Prepared {
        journal.status = Status::Applying;
        write_journal(directory, &journal)?;
    }

    let mut report = RecoveryReport::default();
    for (index, file) in journal.files.iter().enumerate() {
        let target = root.join(Path::new(&file.path));
        let current = read_target(root, &target)?;
        let current_hash = hash_optional(current.as_deref());
        if current_hash == file.after {
            report.recovered.push(RecoveredFile {
                path: target.clone(),
                after: file.payload.clone(),
            });
            continue;
        }
        if current_hash != file.before {
            report.conflicts.push(target);
            continue;
        }
        super::transaction::apply_one(directory, root, &journal, index, false)?;
        report.recovered.push(RecoveredFile {
            path: target,
            after: file.payload.clone(),
        });
    }
    if !report.conflicts.is_empty() {
        return Ok(report);
    }

    journal.status = Status::Committed;
    write_journal(directory, &journal)?;
    cleanup_transaction(root, directory)?;
    Ok(report)
}

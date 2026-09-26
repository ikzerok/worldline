//! 磁盘保存事务与恢复。
//!
//! 文件系统只能逐文件替换；这里记录每个目标的前后状态，让下一次打开时
//! 可以重做尚未完成的替换，或在第三方改动时停在可人工处理的冲突状态。

#[cfg(not(target_arch = "wasm32"))]
use std::path::Component;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub(crate) struct RecoveryReport {
    pub(crate) conflicts: Vec<PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) recovered: Vec<RecoveredFile>,
}

#[derive(Debug, Clone)]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct RecoveredFile {
    pub(crate) path: PathBuf,
    pub(crate) after: Option<Vec<u8>>,
}

#[cfg(not(target_arch = "wasm32"))]
mod disk {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeSet;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    const TRANSACTIONS_RELATIVE: &str = ".world/.transactions";
    const JOURNAL_NAME: &str = "journal.json";
    const JOURNAL_VERSION: u32 = 1;
    static NEXT_TRANSACTION: AtomicU64 = AtomicU64::new(0);
    static STORAGE_LOCK: Mutex<()> = Mutex::new(());

    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    enum Status {
        Prepared,
        Applying,
        Committed,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct Journal {
        version: u32,
        status: Status,
        files: Vec<JournalFile>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct JournalFile {
        path: String,
        before: Option<String>,
        after: Option<String>,
        payload: Option<Vec<u8>>,
    }

    #[derive(Debug, Clone)]
    pub(crate) struct PendingFile {
        pub(crate) relative: PathBuf,
        pub(crate) before: Option<Vec<u8>>,
        pub(crate) after: Option<Vec<u8>>,
    }

    pub(crate) fn transaction_root(root: &Path) -> PathBuf {
        root.join(TRANSACTIONS_RELATIVE)
    }

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

    pub(crate) fn recovery_drafts(
        root: &Path,
    ) -> Result<Vec<crate::recovery::RecoveryDraft>, String> {
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
        if crate::file_access::is_link_or_junction(&journal_metadata) || !journal_metadata.is_file()
        {
            return Err(format!(
                "保存事务日志不是普通文件:{}",
                journal_path.display()
            ));
        }
        let journal_bytes = fs::read(&journal_path).map_err(|error| {
            format!("无法读取保存事务日志:{} ({error})", journal_path.display())
        })?;
        let mut journal: Journal = serde_json::from_slice(&journal_bytes).map_err(|error| {
            format!("保存事务日志格式错误:{} ({error})", journal_path.display())
        })?;
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
            apply_one(directory, root, &journal, index, false)?;
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

    fn apply_one(
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

    fn validate_journal(root: &Path, journal: &Journal) -> Result<(), String> {
        if journal.version != JOURNAL_VERSION || journal.files.is_empty() {
            return Err("保存事务日志版本或文件列表无效".into());
        }
        let mut paths = BTreeSet::new();
        for file in &journal.files {
            let path = Path::new(&file.path);
            if path.is_absolute() {
                return Err(format!("保存事务日志路径必须是相对路径:{}", file.path));
            }
            validate_target(root, path)?;
            if !paths.insert(file.path.clone()) {
                return Err(format!("保存事务日志包含重复路径:{}", file.path));
            }
            if file.before.as_deref().is_some_and(|hash| !valid_hash(hash))
                || file.after.as_deref().is_some_and(|hash| !valid_hash(hash))
            {
                return Err(format!("保存事务日志包含无效 hash:{}", file.path));
            }
            if hash_optional(file.payload.as_deref()) != file.after
                && (file.payload.is_some() || file.after.is_some())
            {
                return Err(format!(
                    "保存事务日志 payload 与 after 不匹配:{}",
                    file.path
                ));
            }
        }
        Ok(())
    }

    fn create_transaction_directory(root: &Path, directory: &Path) -> Result<(), String> {
        let parent = transaction_root(root);
        ensure_directory_chain(root, &parent)?;
        fs::create_dir(directory).map_err(|error| format!("无法建立保存事务目录:{error}"))?;
        sync_directory(&parent)?;
        Ok(())
    }

    fn write_journal(directory: &Path, journal: &Journal) -> Result<(), String> {
        let path = directory.join(JOURNAL_NAME);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_file() {
                return Err(format!("保存事务日志不是普通文件:{}", path.display()));
            }
        }
        let temporary = directory.join(format!("{JOURNAL_NAME}.tmp"));
        if let Ok(metadata) = fs::symlink_metadata(&temporary) {
            if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_file() {
                return Err(format!(
                    "保存事务暂存文件不是普通文件:{}",
                    temporary.display()
                ));
            }
        }
        let bytes = serde_json::to_vec(journal).map_err(|error| error.to_string())?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("无法写入保存事务日志:{} ({error})", temporary.display()))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("无法同步保存事务日志:{} ({error})", temporary.display()))?;
        drop(file);
        fs::rename(&temporary, &path)
            .map_err(|error| format!("无法提交保存事务日志:{} ({error})", path.display()))?;
        sync_directory(directory)?;
        Ok(())
    }

    fn write_payload(path: &Path, payload: &[u8]) -> Result<(), String> {
        if let Ok(metadata) = fs::symlink_metadata(path) {
            if crate::file_access::is_link_or_junction(&metadata) {
                return Err(format!("事务暂存文件不能是链接:{}", path.display()));
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .map_err(|error| format!("无法写入保存暂存文件:{} ({error})", path.display()))?;
        file.write_all(payload)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("无法同步保存暂存文件:{} ({error})", path.display()))
    }

    fn replace_file(temporary: &Path, target: &Path, root: &Path) -> Result<(), String> {
        ensure_target_parent(root, target)?;
        // rename 在同一卷内完成替换，旧文件在替换前始终存在；不先删除目标。
        fs::rename(temporary, target)
            .map_err(|error| format!("替换保存目标失败:{} ({error})", target.display()))?;
        sync_directory(target.parent().unwrap_or(root))?;
        Ok(())
    }

    fn remove_target(root: &Path, target: &Path) -> Result<(), String> {
        validate_target(root, target)?;
        match fs::remove_file(target) {
            Ok(()) => {
                sync_directory(target.parent().unwrap_or(root))?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("删除保存目标失败:{} ({error})", target.display())),
        }
    }

    fn cleanup_transaction(root: &Path, directory: &Path) -> Result<(), String> {
        let entries = fs::read_dir(directory).map_err(|error| error.to_string())?;
        let mut payloads = Vec::new();
        let mut journal = None;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_file() {
                return Err(format!("保存事务暂存区包含无效条目:{}", path.display()));
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == JOURNAL_NAME {
                journal = Some(path);
            } else if name == format!("{JOURNAL_NAME}.tmp")
                || (name.starts_with("payload-") && name.ends_with(".tmp"))
            {
                payloads.push(path);
            } else {
                return Err(format!("保存事务暂存区包含未知条目:{}", path.display()));
            }
        }
        let journal = journal.ok_or_else(|| "保存事务日志缺失".to_string())?;
        for payload in payloads {
            fs::remove_file(payload).map_err(|error| error.to_string())?;
        }
        // 先移除暂存 payload，日志最后移除；中途失败仍保留可读取的日志。
        fs::remove_file(journal).map_err(|error| error.to_string())?;
        fs::remove_dir(directory).map_err(|error| error.to_string())?;
        sync_directory(directory.parent().unwrap_or(root))?;
        let transactions = transaction_root(root);
        remove_empty_transaction_root(&transactions)?;
        if let Some(world) = transactions.parent() {
            remove_empty_directory(world)?;
        }
        Ok(())
    }

    fn remove_empty_transaction_root(parent: &Path) -> Result<(), String> {
        match fs::remove_dir(parent) {
            Ok(()) => {
                sync_directory(parent.parent().unwrap_or(parent))?;
                Ok(())
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.to_string()),
        }
    }

    fn remove_empty_directory(directory: &Path) -> Result<(), String> {
        let mut entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        if entries.next().is_some() {
            return Ok(());
        }
        fs::remove_dir(directory).map_err(|error| error.to_string())
    }

    fn read_target(root: &Path, target: &Path) -> Result<Option<Vec<u8>>, String> {
        validate_target(root, target)?;
        match fs::read(target) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("无法读取保存目标:{} ({error})", target.display())),
        }
    }

    fn ensure_root(root: &Path) -> Result<(), String> {
        if let Ok(metadata) = fs::symlink_metadata(root) {
            if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
                return Err(format!(
                    "工作区根目录不能是链接或普通文件:{}",
                    root.display()
                ));
            }
        } else {
            fs::create_dir_all(root).map_err(|error| error.to_string())?;
        }
        ensure_directory_chain(root, root)
    }

    fn ensure_directory_chain(root: &Path, directory: &Path) -> Result<(), String> {
        let root = crate::compiler::source_path(root);
        let directory = lexical_normalize(directory)?;
        if !directory.starts_with(&root) || directory == root && !root.is_dir() {
            return Err(format!("目录越出工作区边界:{}", directory.display()));
        }
        let relative = directory
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?;
        let mut current = root.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(format!("目录路径包含非法组件:{}", directory.display()));
            };
            current.push(name);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if crate::file_access::is_link_or_junction(&metadata) => {
                    return Err(format!("保存路径不能经过链接:{}", current.display()))
                }
                Ok(metadata) if !metadata.is_dir() => {
                    return Err(format!("保存路径的父级不是目录:{}", current.display()))
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&current).map_err(|error| error.to_string())?;
                    sync_directory(current.parent().unwrap_or(&root))?;
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(())
    }

    fn ensure_target_parent(root: &Path, target: &Path) -> Result<(), String> {
        let parent = target.parent().ok_or("保存目标缺少父目录")?;
        ensure_directory_chain(root, parent)
    }

    fn validate_existing_directory_chain(root: &Path, directory: &Path) -> Result<(), String> {
        let root = crate::compiler::source_path(root);
        let directory = lexical_normalize(directory)?;
        if !directory.starts_with(&root) {
            return Err(format!("目录越出工作区边界:{}", directory.display()));
        }
        let relative = directory
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?;
        let mut current = root;
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(format!("目录路径包含非法组件:{}", directory.display()));
            };
            current.push(name);
            let metadata = fs::symlink_metadata(&current).map_err(|error| error.to_string())?;
            if crate::file_access::is_link_or_junction(&metadata) || !metadata.is_dir() {
                return Err(format!("保存路径不能经过链接:{}", current.display()));
            }
        }
        Ok(())
    }

    fn validate_target(root: &Path, path: &Path) -> Result<String, String> {
        let root = crate::compiler::source_path(root);
        if !path.is_absolute()
            && path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(format!("保存事务目标路径无效:{}", path.display()));
        }
        let candidate = if path.is_absolute() {
            lexical_normalize(path)?
        } else {
            lexical_normalize(&root.join(path))?
        };
        if candidate == root || !candidate.starts_with(&root) {
            return Err(format!("保存事务目标越出工作区边界:{}", path.display()));
        }
        let relative = candidate
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?;
        if relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(format!("保存事务目标路径无效:{}", path.display()));
        }
        if is_transaction_relative(relative) || is_checkpoint_relative(relative) {
            return Err(format!("保存事务目标不能位于受控存储区:{}", path.display()));
        }

        let mut current = root.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(format!("保存事务目标路径无效:{}", path.display()));
            };
            current.push(name);
            if let Ok(metadata) = fs::symlink_metadata(&current) {
                if crate::file_access::is_link_or_junction(&metadata) {
                    return Err(format!("保存事务目标不能是链接:{}", current.display()));
                }
            }
        }
        Ok(relative.to_string_lossy().replace('\\', "/"))
    }

    fn lexical_normalize(path: &Path) -> Result<PathBuf, String> {
        let mut normal = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    if !normal.pop() {
                        return Err(format!("路径越出工作区边界:{}", path.display()));
                    }
                }
                Component::Prefix(prefix) => normal.push(prefix.as_os_str()),
                Component::RootDir => normal.push(component.as_os_str()),
                Component::Normal(name) => normal.push(name),
            }
        }
        Ok(normal)
    }

    fn is_transaction_relative(path: &Path) -> bool {
        let mut components = path.components();
        matches!(components.next(), Some(Component::Normal(world)) if storage_component_matches(world, ".world"))
            && matches!(components.next(), Some(Component::Normal(transactions)) if storage_component_matches(transactions, ".transactions"))
    }

    fn is_checkpoint_relative(path: &Path) -> bool {
        let mut components = path.components();
        matches!(components.next(), Some(Component::Normal(world)) if storage_component_matches(world, ".world"))
            && matches!(components.next(), Some(Component::Normal(checkpoints)) if storage_component_matches(checkpoints, ".checkpoints"))
    }

    fn storage_component_matches(name: &std::ffi::OsStr, expected: &str) -> bool {
        #[cfg(windows)]
        {
            name.to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(expected))
        }
        #[cfg(not(windows))]
        {
            name == std::ffi::OsStr::new(expected)
        }
    }

    fn hash_optional(bytes: Option<&[u8]>) -> Option<String> {
        bytes.map(hash_bytes)
    }

    fn hash_bytes(bytes: &[u8]) -> String {
        // 事务日志需要稳定且无依赖的内容指纹；缺失用 None 表示，不能与空字节混淆。
        let mut hash = 0xcbf29ce484222325u64;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("{hash:016x}")
    }

    fn valid_hash(hash: &str) -> bool {
        hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    fn transaction_id() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sequence = NEXT_TRANSACTION.fetch_add(1, Ordering::Relaxed);
        format!("{nanos:032x}-{}-{sequence:016x}", std::process::id())
    }

    fn failure_requested(phase: &str) -> bool {
        failure_phase_matches(phase)
    }

    fn failure_requested_at_replacement(index: usize, total: usize) -> bool {
        let Some(value) = failure_phase() else {
            return false;
        };
        match value.as_str() {
            "first" | "replacement:first" => index == 0,
            "middle" | "replacement:middle" => index == total / 2,
            "last" | "replacement:last" => index + 1 == total,
            _ => false,
        }
    }

    fn failure_phase_matches(phase: &str) -> bool {
        failure_phase().is_some_and(|value| value == phase)
    }

    fn failure_phase() -> Option<String> {
        let value = std::env::var("WORLDLINE_SAVE_FAIL_PHASE").ok()?;
        if let Ok(thread) = std::env::var("WORLDLINE_SAVE_FAIL_THREAD") {
            if thread != format!("{:?}", std::thread::current().id()) {
                return None;
            }
        }
        Some(value)
    }

    #[cfg(unix)]
    fn sync_directory(directory: &Path) -> Result<(), String> {
        fs::File::open(directory)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("无法同步保存目录：{} ({error})", directory.display()))
    }

    #[cfg(not(unix))]
    fn sync_directory(_directory: &Path) -> Result<(), String> {
        // std 未提供这些平台通用的目录持久化接口。文件内容仍 sync_all；
        // 对进程中断提供恢复，不承诺断电后的目录元数据持久化。
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use disk::{has_unresolved_transactions, recover, recovery_drafts, save, PendingFile};

#[cfg(target_arch = "wasm32")]
pub(crate) fn recover(_root: &Path) -> Result<RecoveryReport, String> {
    Ok(RecoveryReport::default())
}

//! 从当前工程缓冲建立可移植的原始工作区快照。
//!
//! 这里保留作者文件的原始字节，不编译、不迁移，也不把普通文件误当作展示
//! 文档。严格的可编译导出仍由 [`Project::export_files`] 提供。

use crate::project::Project;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

/// 相对工作区路径到原始文件字节的映射。
pub type Files = BTreeMap<PathBuf, Vec<u8>>;

const PROJECT_MANIFEST: &str = ".world/project.json";

/// 生成当前缓冲的完整原始文件快照。
///
/// 源码和已注册展示文档以缓冲中的内容为准；墓碑只用于阻止磁盘上的旧
/// 文件重新出现。其余工作区文件逐字节复制，因此普通 JSON、无效 UTF-8
/// 和工具未知格式都能随工程包保留。
pub fn snapshot_files(project: &Project) -> Result<Files, String> {
    ensure_storage_ready(project)?;

    let root = crate::compiler::source_path(&project.root);
    let mut managed = BTreeSet::new();
    let mut files = Files::new();

    for (path, document) in &project.documents {
        let relative = relative_path(&root, path)?;
        if !relative
            .extension()
            .is_some_and(|extension| extension == "wl")
        {
            return Err(format!("源码路径必须是 .wl 文件:{}", path.display()));
        }
        if !managed.insert(relative.clone()) {
            return Err(format!("工程包含重复文件路径:{}", relative.display()));
        }
        if !document.is_deleted() {
            insert_file(&mut files, relative, document.text.as_bytes().to_vec())?;
        }
    }

    for (path, document) in &project.authoring_documents {
        let relative = relative_path(&root, path)?;
        if !relative
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            return Err(format!("展示文档路径必须是 .json 文件:{}", path.display()));
        }
        if !managed.insert(relative.clone()) {
            return Err(format!("工程包含重复文件路径:{}", relative.display()));
        }
        if !document.is_deleted() {
            insert_file(&mut files, relative, document.bytes().to_vec())?;
        }
    }

    for path in workspace_files(&root)? {
        let relative = relative_path(&root, &path)?;
        if is_transaction_path(&relative) {
            return Err("工程包含未完成的保存事务".into());
        }
        if managed.contains(&relative) {
            continue;
        }
        let bytes = crate::file_access::read(&path).map_err(|error| error.to_string())?;
        insert_file(&mut files, relative, bytes)?;
    }
    Ok(files)
}

/// 从新的 `.world/project.json` 中读取可选的 `.wl` 入口。
///
/// 新清单是作者数据，解析失败时返回 `None`，使宿主继续使用旧清单或
/// `world.wl` 回退；成功解析后，入口路径缺失或越界仍是包结构错误。解析
/// 复用 core 的无重复键 JSON 规则，避免宿主层采用不同的 last-wins 语义。
pub fn project_entry(files: &Files) -> Result<Option<PathBuf>, String> {
    let Some(manifest) = files.get(Path::new(PROJECT_MANIFEST)) else {
        return Ok(None);
    };
    let value = match crate::workspace_documents::parse_unique_json(manifest) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let Some(entry) = object.get("entry").and_then(Value::as_str) else {
        return Ok(None);
    };
    let entry = relative_file_path(entry)?;
    if !entry.extension().is_some_and(|extension| extension == "wl") || !files.contains_key(&entry)
    {
        return Err("工程包记录的 .wl 入口文件不存在".into());
    }
    Ok(Some(entry))
}

fn ensure_storage_ready(project: &Project) -> Result<(), String> {
    if !project.recovery_conflicts().is_empty() {
        return Err("工程存在未解决的保存事务，请重新打开并处理冲突".into());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if crate::storage::has_unresolved_transactions(&project.root)? {
        return Err("工程存在未解决的保存事务，请重新打开并处理冲突".into());
    }
    Ok(())
}

fn workspace_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    if !root.exists() {
        return Ok(Vec::new());
    }
    crate::file_access::workspace_files(root).map_err(|error| error.to_string())
}

fn insert_file(files: &mut Files, path: PathBuf, bytes: Vec<u8>) -> Result<(), String> {
    if files.insert(path, bytes).is_some() {
        return Err("工程包含重复文件路径".into());
    }
    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = crate::compiler::source_path(root);
    let path = crate::compiler::source_path(path);
    let relative = path
        .strip_prefix(&root)
        .map_err(|_| format!("文件不在工作区内:{}", path.display()))?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("工作区文件路径无效:{}", path.display()));
    }
    Ok(relative.to_path_buf())
}

fn relative_file_path(value: &str) -> Result<PathBuf, String> {
    let value = value.replace('\\', "/");
    let path = Path::new(&value);
    if value.is_empty()
        || value.contains(':')
        || value.starts_with('/')
        || value
            .split('/')
            .any(|part| part == ".." || part == "." || part.is_empty())
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("工程包入口路径无效".into());
    }
    Ok(path.to_path_buf())
}

fn is_transaction_path(path: &Path) -> bool {
    let mut components = path.components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(world)), Some(Component::Normal(transactions)))
            if world == ".world" && transactions == ".transactions"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("worldline-snapshot-{name}-{}", std::process::id()))
    }

    #[test]
    fn snapshot_preserves_drafts_registered_bytes_and_unknown_files() {
        let root = temp_root("raw");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("notes.json"), br#"{"ordinary":true}"#).unwrap();
        fs::write(root.join("assets/blob.bin"), [0, 17, 255]).unwrap();

        let mut project = Project::new(&root);
        project
            .set_text(&root.join("world.wl"), "broken draft".into())
            .unwrap();
        let manifest = root.join(PROJECT_MANIFEST);
        project
            .create_authoring_document(
                &manifest,
                br#"{"schema_version":1,"maps":{"raw":".world/maps/raw.json"}}"#.to_vec(),
            )
            .unwrap();
        let raw = vec![b'{', 0xff, b'}'];
        project
            .create_authoring_document(&root.join(".world/maps/raw.json"), raw.clone())
            .unwrap();

        let files = snapshot_files(&project).unwrap();
        assert_eq!(files[Path::new("world.wl")], b"broken draft");
        assert_eq!(files[Path::new(".world/maps/raw.json")], raw);
        assert_eq!(files[Path::new("notes.json")], br#"{"ordinary":true}"#);
        assert_eq!(files[Path::new("assets/blob.bin")], [0, 17, 255]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn snapshot_omits_tombstones_without_resurrecting_disk_files() {
        let root = temp_root("tombstones");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".world/maps")).unwrap();
        fs::write(root.join("world.wl"), b"old source").unwrap();
        fs::write(root.join(".world/maps/deleted.json"), b"old map").unwrap();
        fs::write(root.join("ordinary.json"), b"keep").unwrap();

        let mut project = Project::new(&root);
        project.delete_document(&root.join("world.wl")).unwrap();
        let manifest = root.join(PROJECT_MANIFEST);
        project
            .create_authoring_document(
                &manifest,
                br#"{"schema_version":1,"maps":{"deleted":".world/maps/deleted.json"}}"#.to_vec(),
            )
            .unwrap();
        project
            .delete_authoring_document(&root.join(".world/maps/deleted.json"))
            .unwrap();

        let files = snapshot_files(&project).unwrap();
        assert!(!files.contains_key(Path::new("world.wl")));
        assert!(!files.contains_key(Path::new(".world/maps/deleted.json")));
        assert_eq!(files[Path::new("ordinary.json")], b"keep");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn raw_snapshot_keeps_a_draft_that_strict_export_rejects() {
        let root = temp_root("draft");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut project = Project::new(&root);
        project
            .set_text(
                &root.join("world.wl"),
                "this is not a valid worldline draft".into(),
            )
            .unwrap();

        let files = snapshot_files(&project).unwrap();
        assert_eq!(
            files[Path::new("world.wl")],
            b"this is not a valid worldline draft"
        );
        assert!(project.export_files().is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn unresolved_transaction_blocks_raw_snapshot() {
        let root = temp_root("transactions");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".world/.transactions/tx")).unwrap();
        fs::write(root.join(".world/.transactions/tx/journal.json"), b"{}").unwrap();
        let project = Project::new(&root);
        let error = snapshot_files(&project).unwrap_err();
        assert!(error.contains("未解决的保存事务"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_entry_rejects_duplicate_keys_using_core_parser() {
        let files = Files::from([
            (PathBuf::from("world.wl"), b"source".to_vec()),
            (PathBuf::from("other.wl"), b"other".to_vec()),
            (
                PathBuf::from(PROJECT_MANIFEST),
                br#"{"entry":"other.wl","entry":"world.wl"}"#.to_vec(),
            ),
        ]);
        assert_eq!(project_entry(&files).unwrap(), None);
    }

    #[test]
    fn project_entry_keeps_unknown_fields_and_normalizes_separators() {
        let files = Files::from([
            (PathBuf::from("stories/intro.wl"), b"source".to_vec()),
            (
                PathBuf::from(PROJECT_MANIFEST),
                br#"{"schema_version":1,"unknown":{"kept":true},"entry":"stories\\intro.wl"}"#
                    .to_vec(),
            ),
        ]);
        assert_eq!(
            project_entry(&files).unwrap(),
            Some(PathBuf::from("stories/intro.wl"))
        );
    }

    #[test]
    fn project_entry_rejects_ambiguous_relative_segments() {
        for entry in [
            "stories//intro.wl",
            "stories/./intro.wl",
            "stories/../intro.wl",
        ] {
            let files = Files::from([
                (PathBuf::from("world.wl"), b"source".to_vec()),
                (
                    PathBuf::from(PROJECT_MANIFEST),
                    serde_json::to_vec(&serde_json::json!({"entry": entry})).unwrap(),
                ),
            ]);
            assert!(project_entry(&files).is_err(), "{entry}");
        }
    }
}

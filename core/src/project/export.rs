use super::*;

impl Project {
    /// 导出可直接编译的完整目录;不覆盖目标目录,包含所有内存修改。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn export(&self, destination: &Path) -> Result<(), String> {
        self.ensure_storage_ready()?;
        if destination.exists() {
            return Err("导出目标已存在,请选择新的文件夹名称".into());
        }
        self.validate_destination(destination)?;
        let mut migrated = self.clone();
        if migrated.root.exists() || cfg!(target_arch = "wasm32") {
            migrated.refresh()?;
        }
        if migrated.authoring_diagnostics.is_empty() {
            migrated.migrate_permissions()?;
        }
        let files = migrated.export_file_contents()?;
        let parent = destination.parent().ok_or("导出目录缺少父目录")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let staging = parent.join(format!(".worldedit-export-{}", std::process::id()));
        std::fs::create_dir(&staging).map_err(|e| format!("无法建立导出暂存目录:{e}"))?;
        let write_result = (|| -> std::io::Result<()> {
            for (path, text) in files.sources {
                let path = staging.join(path);
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(path, text)?;
            }
            for (path, bytes) in files.authoring {
                let path = staging.join(path);
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(path, bytes)?;
            }
            for (relative, source) in files.copies {
                let target = staging.join(relative);
                std::fs::create_dir_all(target.parent().unwrap())?;
                std::fs::copy(source, target)?;
            }
            std::fs::rename(&staging, destination)
        })();
        if let Err(e) = write_result {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("导出失败:{e}"));
        }
        Ok(())
    }

    /// 校验并生成完整可移植工程；桌面目录与浏览器下载共用同一导出内容。
    pub fn export_files(&self) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
        #[cfg(not(target_arch = "wasm32"))]
        self.ensure_storage_ready()?;
        let mut migrated = self.clone();
        if migrated.root.exists() || cfg!(target_arch = "wasm32") {
            migrated.refresh()?;
        }
        if migrated.authoring_diagnostics.is_empty() {
            migrated.migrate_permissions()?;
        }
        let contents = migrated.export_file_contents()?;
        let mut files: BTreeMap<_, _> = contents
            .sources
            .into_iter()
            .map(|(path, text)| (path, text.into_bytes()))
            .collect();
        files.extend(contents.authoring);
        for (relative, source) in contents.copies {
            files.insert(
                relative,
                crate::file_access::read(source).map_err(|e| e.to_string())?,
            );
        }
        Ok(files)
    }

    fn export_file_contents(&self) -> Result<crate::catalog_edit::PortableAssets, String> {
        let result = self.compile_current();
        if result.has_errors() {
            return Err("工程存在编译错误,修复后才能导出".into());
        }
        for asset in result.analysis.catalog.assets.values() {
            if !asset.available {
                return Err(format!("素材 `{}` 不可用，请修复引用后导出", asset.id));
            }
        }
        let portable = self.portable_assets()?;
        let mut files = BTreeMap::<PathBuf, String>::new();
        for (path, text) in &portable.sources {
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| format!("引用文件在工程目录之外:{}", path.display()))?;
            validate_relative(relative)?;
            for line in crate::lexer::lex_source(&path.to_string_lossy(), text, &mut Vec::new()) {
                if let crate::lexer::LineKind::Include { path, .. } = line.kind {
                    if Path::new(&path).is_absolute() {
                        return Err("导出要求 include 使用相对路径".into());
                    }
                }
            }
            files.insert(relative.into(), text.clone());
        }
        let mut authoring = BTreeMap::new();
        for (path, bytes) in portable.authoring {
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| format!("展示文档在工程目录之外:{}", path.display()))?;
            validate_authoring_relative(relative)?;
            authoring.insert(relative.into(), bytes);
        }
        if self.entry.file_name() != Some(std::ffi::OsStr::new("world.wl")) {
            if files.contains_key(Path::new("world.wl"))
                || authoring.contains_key(Path::new("world.wl"))
            {
                return Err("world.wl 已被其他引用文件占用,请先整理总入口".into());
            }
            files.insert(
                "world.wl".into(),
                format!(
                    "include {}\n",
                    crate::authoring::quote(&self.entry.file_name().unwrap().to_string_lossy())
                ),
            );
        }
        Ok(crate::catalog_edit::PortableAssets {
            sources: files,
            authoring,
            copies: portable.copies,
        })
    }
}

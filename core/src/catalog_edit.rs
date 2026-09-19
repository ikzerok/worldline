//! 目录结构编辑与素材的可移植打包计划。
use crate::authoring::{identifier, property_lines, quote, WorldDraft};
use crate::catalog::{CatalogDecl, TargetRef};
use crate::lexer::{lex_source, LineKind};
use crate::project::Project;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Default)]
pub struct AssetDraft {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub display: String,
}

pub(crate) fn link_source(target: &TargetRef, values: &[String], attach: bool) -> String {
    format!(
        "{} {} {} with {}",
        if attach { "attach" } else { "mark" },
        target.kind,
        if target.kind == "file" {
            quote(&target.id)
        } else {
            target.id.clone()
        },
        values.join(", ")
    )
}

fn asset_source(asset: &AssetDraft) -> String {
    format!(
        "asset {} {} {} as {}",
        asset.id,
        asset.kind,
        quote(&asset.path),
        quote(&asset.display)
    )
}

impl Project {
    pub fn write_tag(&mut self, original: Option<&str>, draft: &WorldDraft) -> Result<(), String> {
        identifier(&draft.id)?;
        if original.is_some_and(|id| id != draft.id) {
            return Err("标签 ID 是引用身份,修改名称和资料时请保留 ID".into());
        }
        let result = self.compile_current();
        let path = original
            .and_then(|id| result.analysis.catalog.tags.get(id))
            .map(|tag| PathBuf::from(&tag.file))
            .unwrap_or_else(|| self.entry.clone());
        let out = format!(
            "tag {} as {}\n  description {}\n{}",
            draft.id,
            quote(&draft.display),
            quote(&draft.description),
            property_lines(&draft.properties)?
        );
        self.replace_metadata(&path, original, "tag", &out)
    }

    pub fn write_asset(&mut self, draft: &AssetDraft) -> Result<(), String> {
        identifier(&draft.id)?;
        let result = self.compile_current();
        let existing = result.analysis.catalog.assets.get(&draft.id);
        let path = existing
            .map(|a| PathBuf::from(&a.file))
            .unwrap_or_else(|| self.entry.clone());
        if Path::new(&draft.path).is_absolute() {
            return Err("附件必须使用相对路径".into());
        }
        crate::file_access::within(&self.root, &path.parent().unwrap().join(&draft.path))?;
        let mut text = self.document(&path)?.to_string();
        let out = asset_source(draft);
        if let Some(asset) = existing {
            let mut physical: Vec<_> = text.split_inclusive('\n').map(str::to_string).collect();
            let raw = &physical[asset.line as usize - 1];
            let comment = crate::authoring::comments(raw);
            physical[asset.line as usize - 1] = format!("{comment}{out}\n");
            text = physical.concat();
        } else {
            text.push_str(&format!("\n{out}\n"));
        }
        self.set_text(&path, text)
    }

    /// 修改完整对象的直接引用;不改动正文旧标签,不删除对象或素材本体。
    pub fn set_catalog_links(
        &mut self,
        target: &TargetRef,
        values: &[String],
        attach: bool,
    ) -> Result<(), String> {
        for value in values {
            identifier(value)?;
        }
        let mut destination = self.entry.clone();
        for (path, document) in &mut self.documents {
            let parsed = lex_source(&path.to_string_lossy(), &document.text, &mut Vec::new());
            let remove: std::collections::BTreeSet<_> = parsed
                .iter()
                .filter_map(|line| {
                    let link = match &line.kind {
                        LineKind::Catalog(CatalogDecl::Attach(link)) if attach => link,
                        LineKind::Catalog(CatalogDecl::Mark(link)) if !attach => link,
                        _ => return None,
                    };
                    let mut actual = link.target.clone();
                    if actual.kind == "file" {
                        actual.id = crate::catalog::resolved_asset(&link.file, &actual.id)
                            .to_string_lossy()
                            .into_owned();
                    }
                    if &actual == target {
                        Some(line.no as usize)
                    } else {
                        None
                    }
                })
                .collect();
            if !remove.is_empty() {
                destination = path.clone();
            }
            document.text = document
                .text
                .split_inclusive('\n')
                .enumerate()
                .map(|(i, raw)| {
                    if remove.contains(&(i + 1)) {
                        crate::authoring::comments(raw)
                    } else {
                        raw.into()
                    }
                })
                .collect();
        }
        if !values.is_empty() {
            let mut target = target.clone();
            if target.kind == "file" {
                let parent = destination.parent().unwrap_or(&self.root);
                target.id = relative_source_path(parent, Path::new(&target.id))?;
            }
            let mut text = self.document(&destination)?.to_string();
            text.push_str(&format!("\n{}\n", link_source(&target, values, attach)));
            self.set_text(&destination, text)?;
        }
        Ok(())
    }

    pub fn add_asset_reference(
        &mut self,
        target: &TargetRef,
        external: &Path,
    ) -> Result<String, String> {
        let path = crate::file_access::within(&self.root, external)?;
        let kind = if crate::catalog::supported_extension("image", &path) {
            "image"
        } else if crate::catalog::supported_extension("audio", &path) {
            "audio"
        } else {
            "file"
        };
        let result = self.compile_current();
        let existing = result
            .analysis
            .catalog
            .assets
            .values()
            .find(|a| Path::new(&a.resolved_path) == path);
        let id = if let Some(asset) = existing {
            asset.id.clone()
        } else {
            let mut index = 1;
            while result
                .analysis
                .catalog
                .assets
                .contains_key(&format!("asset_{index}"))
            {
                index += 1;
            }
            let id = format!("asset_{index}");
            let source = path
                .strip_prefix(&self.root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| path.to_string_lossy().into_owned());
            self.write_asset(&AssetDraft {
                id: id.clone(),
                kind: kind.into(),
                path: source,
                display: path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            })?;
            id
        };
        let mut values: Vec<_> = result
            .analysis
            .catalog
            .assets_for(target)
            .into_iter()
            .map(|a| a.id)
            .collect();
        if !values.contains(&id) {
            values.push(id.clone());
        }
        self.set_catalog_links(target, &values, true)?;
        Ok(id)
    }

    pub(crate) fn portable_assets(&self) -> Result<PortableAssets, String> {
        let sources = self.sources();
        let authoring = self
            .authoring_documents
            .iter()
            .filter(|(_, document)| !document.is_deleted())
            .map(|(path, document)| (path.clone(), document.bytes().to_vec()))
            .collect();
        let registered: std::collections::BTreeSet<_> =
            self.authoring_documents.keys().cloned().collect();
        let source_paths: std::collections::BTreeSet<_> = self.documents.keys().cloned().collect();
        let mut copies = BTreeMap::new();
        if self.root.exists() || cfg!(target_arch = "wasm32") {
            for path in
                crate::file_access::workspace_files(&self.root).map_err(|e| e.to_string())?
            {
                if !sources.contains_key(&path)
                    && !source_paths.contains(&path)
                    && !registered.contains(&path)
                {
                    copies.insert(
                        path.strip_prefix(&self.root)
                            .map_err(|e| e.to_string())?
                            .to_path_buf(),
                        path,
                    );
                }
            }
        }
        Ok(PortableAssets {
            sources,
            authoring,
            copies,
        })
    }
}

pub(crate) struct PortableAssets {
    pub sources: BTreeMap<PathBuf, String>,
    pub authoring: BTreeMap<PathBuf, Vec<u8>>,
    pub copies: BTreeMap<PathBuf, PathBuf>,
}

pub(crate) fn relative_source_path(from: &Path, to: &Path) -> Result<String, String> {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if shared == 0 {
        return Err("路径不在同一工程卷中".into());
    }
    let mut path = PathBuf::new();
    for _ in shared..from.len() {
        path.push("..");
    }
    for component in &to[shared..] {
        path.push(component.as_os_str());
    }
    Ok(path.to_string_lossy().replace('\\', "/"))
}

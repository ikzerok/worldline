//! 就地建档与稳定正文引用：候选工程验证后整体提交。
use crate::authoring::EntityDraft;
use crate::catalog::TargetRef;
use crate::presentation_commands::{self, Command, CommandEnvelope, Revision};
use crate::project::Project;
use std::path::{Path, PathBuf};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum IntentTarget {
    Existing(TargetRef),
    CreateEntity { path: PathBuf, draft: EntityDraft },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TextSelection {
    pub path: PathBuf,
    pub start: usize,
    pub end: usize,
    pub expected_text: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthoringIntent {
    pub expected_baseline: String,
    pub target: IntentTarget,
    pub selection: Option<TextSelection>,
    pub placement: Option<PlacementRequest>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PlacementRequest {
    pub map_id: String,
    pub placement_id: String,
    pub layer_id: String,
    pub geometry: crate::MapGeometry,
    pub annotation: String,
    pub role: String,
    pub label_override: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct IntentResult {
    pub target: TargetRef,
    /// 候选工程中此目标的全部引用与来源；complete 标识分析是否完整。
    pub reference_impact: crate::reference_impact::DeletionImpact,
    pub changed_files: Vec<PathBuf>,
    pub new_baseline: String,
}

impl Project {
    pub fn preview_authoring_intent(
        &self,
        intent: &AuthoringIntent,
    ) -> Result<IntentResult, String> {
        self.prepare_authoring_intent(intent)
            .map(|(_, result)| result)
    }

    pub fn apply_authoring_intent(
        &mut self,
        intent: &AuthoringIntent,
    ) -> Result<IntentResult, String> {
        let (candidate, result) = self.prepare_authoring_intent(intent)?;
        *self = candidate;
        Ok(result)
    }

    fn prepare_authoring_intent(
        &self,
        intent: &AuthoringIntent,
    ) -> Result<(Project, IntentResult), String> {
        self.ensure_workspace_writable()?;
        if intent.expected_baseline != self.content_baseline() {
            return Err("组合意图基线已过期，请保留草稿并重新预览".into());
        }
        if !self.recovery_conflicts().is_empty() {
            return Err("工程有未解决的保存事务冲突".into());
        }
        // 保存基线与当前缓冲不同；本地未保存草稿仍可编辑，外部版本变化须拒绝。
        for path in self.documents.keys().chain(self.authoring_documents.keys()) {
            crate::file_access::within(&self.root, path)?;
            let state = self.tracked_file_state(path).ok_or("文档基线不存在")?;
            let disk = match crate::file_access::read(path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(format!("无法检查文档基线：{error}")),
            };
            if disk != state.baseline {
                return Err(format!(
                    "文档已被外部修改，请保留草稿并刷新：{}",
                    path.display()
                ));
            }
        }
        if intent.selection.is_none() && intent.placement.is_none() {
            return Err("组合意图需要正文选区或地图入口".into());
        }
        let target = match &intent.target {
            IntentTarget::Existing(target) => target.clone(),
            IntentTarget::CreateEntity { draft, .. } => TargetRef::new("entity", &draft.id),
        };
        let selection = match &intent.selection {
            Some(selection) => {
                let mut selection = selection.clone();
                selection.path = require_active_source(self, &selection.path)?;
                Some(selection)
            }
            None => None,
        };
        let create_entity_path = match &intent.target {
            IntentTarget::CreateEntity { path, .. } => Some(require_active_source(self, path)?),
            IntentTarget::Existing(_) => None,
        };
        let mut candidate = self.clone();
        if let Some(selection) = &selection {
            let original = self.document(&selection.path)?;
            if selection.expected_text.is_empty()
                || original.get(selection.start..selection.end)
                    != Some(selection.expected_text.as_str())
            {
                return Err("正文选区已失效或不在 UTF-8 字符边界".into());
            }
            let mut replacement = original.to_owned();
            let link = crate::navigation::link_source(
                &target,
                &selection.expected_text,
                &selection.path.to_string_lossy(),
            )?;
            replacement.replace_range(selection.start..selection.end, &link);
            // 先替换正文，避免在同一文件创建实体后使原始字节位置偏移。
            candidate.set_text(&selection.path, replacement)?;
        }
        if let (IntentTarget::CreateEntity { draft, .. }, Some(path)) =
            (&intent.target, &create_entity_path)
        {
            candidate.write_entity(path, None, draft)?;
        }
        let compiled = candidate.compile();
        if let Some(diagnostic) = compiled
            .diagnostics
            .iter()
            .find(|d| d.severity == crate::Severity::Error)
        {
            return Err(format!("{}：{}", diagnostic.code, diagnostic.message));
        }
        if compiled.analysis.catalog.object(&target).is_none() {
            return Err("引用目标不存在".into());
        }
        if let Some(selection) = &selection {
            let count = |result: &crate::CompileResult| {
                result
                    .analysis
                    .catalog
                    .text_links
                    .iter()
                    .filter(|link| {
                        Path::new(&link.file) == selection.path
                            && link.target == target
                            && link.label == selection.expected_text
                    })
                    .count()
            };
            if count(&compiled) != count(&self.compile_current()) + 1 {
                return Err("选区必须位于可生成显式引用的正文或选项中".into());
            }
        }
        let mut changed_files: Vec<_> = candidate
            .documents
            .iter()
            .filter(|(path, document)| {
                self.documents
                    .get(*path)
                    .is_none_or(|before| before.text != document.text)
            })
            .map(|(path, _)| path.clone())
            .collect();
        if let Some(placement) = &intent.placement {
            let path = presentation_commands::map_document_path(&candidate, &placement.map_id)
                .map_err(|e| e.to_string())?;
            let hash =
                presentation_commands::document_hash(candidate.authoring_document(&path)?.bytes());
            let result = presentation_commands::apply(
                &mut candidate,
                &mut Revision::default(),
                CommandEnvelope {
                    expected_revision: Revision::default(),
                    expected_documents: [(path, hash)].into(),
                    command: Command::CreatePlacement {
                        map_id: placement.map_id.clone(),
                        placement_id: placement.placement_id.clone(),
                        layer_id: placement.layer_id.clone(),
                        target_ref: Some(target.clone()),
                        geometry: placement.geometry.clone(),
                        annotation: placement.annotation.clone(),
                        role: placement.role.clone(),
                        label_override: placement.label_override.clone(),
                    },
                },
            )
            .map_err(|e| e.to_string())?;
            changed_files.extend(result.changed_files);
        }
        changed_files.sort();
        changed_files.dedup();
        let result = IntentResult {
            reference_impact: candidate.deletion_impact(&target),
            target,
            changed_files,
            new_baseline: candidate.content_baseline(),
        };
        Ok((candidate, result))
    }
}

fn require_active_source(project: &Project, path: &Path) -> Result<PathBuf, String> {
    // Web's `/world` mount is workspace-absolute even when Path::is_absolute returns false.
    let web_rooted = matches!(
        path.components().next(),
        Some(std::path::Component::RootDir)
    ) && matches!(
        project.root.components().next(),
        Some(std::path::Component::RootDir)
    );
    let absolute = path.is_absolute() || web_rooted;
    let root_absolute = project.root.is_absolute() || web_rooted;
    let traverses_root = web_rooted
        && path
            .components()
            .any(|part| part == std::path::Component::ParentDir);
    let root = lexical_absolute_path(&project.root);
    let path = lexical_absolute_path(path);
    if !absolute
        || !root_absolute
        || traverses_root
        || !path.starts_with(&root)
        || !project.sources().contains_key(&path)
    {
        return Err("源文件必须是工作区内已载入的活动源码".into());
    }
    Ok(path)
}

fn lexical_absolute_path(path: &Path) -> PathBuf {
    // Normalize VFS paths lexically; browser-mounted paths have no host filesystem entry.
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let rooted = matches!(
        absolute.components().next(),
        Some(std::path::Component::Prefix(_) | std::path::Component::RootDir)
    );
    let mut normalized = PathBuf::new();
    for part in absolute.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                } else if !rooted {
                    normalized.push(part.as_os_str());
                }
            }
            _ => normalized.push(part.as_os_str()),
        }
    }
    normalized
}

#[cfg(all(test, windows))]
mod tests {
    use super::{lexical_absolute_path, require_active_source};
    use crate::project::Project;
    use std::path::{Path, PathBuf};

    #[test]
    fn web_rooted_source_requires_loaded_identity_and_stays_within_root() {
        let mut project = Project::new(
            &std::env::temp_dir().join(format!("worldline-web-path-{}", std::process::id())),
        );
        let original_entry = project.entry.clone();
        let document = project.documents.remove(&original_entry).unwrap();
        let root = PathBuf::from(r"\world")
            .join(format!("web-path-{}", std::process::id()));
        let entry = root.join("world.wl");
        project.root = root.clone();
        project.entry = entry.clone();
        project.documents.clear();
        project
            .documents
            .insert(lexical_absolute_path(&entry), document);

        assert!(!entry.is_absolute());
        assert_eq!(
            require_active_source(&project, &entry).unwrap(),
            lexical_absolute_path(&entry)
        );
        assert!(require_active_source(&project, &root.join("..").join("outside.wl")).is_err());
        assert!(
            require_active_source(&project, &PathBuf::from(r"\elsewhere\world.wl")).is_err()
        );
        assert!(require_active_source(&project, Path::new("world.wl")).is_err());
    }
}

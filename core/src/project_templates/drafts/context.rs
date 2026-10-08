use super::*;

/// 草稿只检查已加载缓冲；不得调用会 canonicalize/读取文件身份的注册解析器。
pub(super) struct DraftContext {
    pub(super) manifest: Value,
    pub(super) features: BTreeSet<String>,
    pub(super) read_only: bool,
}

impl DraftContext {
    pub(super) fn new(project: &Project) -> Self {
        let document = project
            .authoring_documents
            .get(&project.root.join(".world/project.json"))
            .filter(|document| !document.is_deleted());
        let parsed = document.map(|d| parse_unique_json(d.bytes()));
        let malformed = parsed.as_ref().is_some_and(Result::is_err);
        let manifest = parsed.and_then(Result::ok).unwrap_or_else(|| json!({}));
        let features = manifest
            .get("required_features")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        Self {
            manifest,
            features,
            read_only: malformed
                || !project.authoring_diagnostics().is_empty()
                || document.is_some_and(AuthoringDocument::is_read_only),
        }
    }

    pub(super) fn registered(&self, id: &str) -> bool {
        self.manifest
            .get("templates")
            .and_then(Value::as_object)
            .is_some_and(|templates| templates.contains_key(id))
    }

    pub(super) fn template_path(&self, project: &Project, id: &str) -> Option<PathBuf> {
        let relative = self.manifest.get("templates")?.get(id)?.as_str()?;
        let mut path = project.root.clone();
        for component in Path::new(relative).components() {
            match component {
                std::path::Component::Normal(part) => path.push(part),
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir if path != project.root => {
                    path.pop();
                }
                _ => return None,
            }
        }
        (path != project.root && path.starts_with(&project.root)).then_some(path)
    }

    pub(super) fn next_id(&self, prefix: &str) -> String {
        (1..)
            .map(|n| format!("project:{prefix}_{n}"))
            .find(|id| !self.registered(id))
            .expect("可用模板 ID")
    }
}

use super::*;

impl Project {
    /// 返回 builtin 与清单注册的工程模板；坏文档只产生局部诊断，不中断其他项。
    pub fn template_index(&self) -> ProjectTemplateIndex {
        let builtins = crate::content_templates::builtin_templates()
            .templates
            .clone();
        let manifest = manifest_path(&self.root);
        let Some(manifest_document) = self
            .authoring_documents
            .get(&manifest)
            .filter(|document| !document.is_deleted())
        else {
            return ProjectTemplateIndex {
                builtins,
                projects: BTreeMap::new(),
                diagnostics: Vec::new(),
            };
        };
        let registry = parse_registry(&self.root, manifest_document.bytes());
        let root_has_feature = registry
            .required_features
            .contains(PROJECT_TEMPLATE_REQUIRED_FEATURE);
        let content = self.compile_current();
        let mut projects = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for (id, path) in &registry.templates {
            let document = self.authoring_documents.get(path);
            let bytes = document
                .filter(|document| !document.is_deleted())
                .map(AuthoringDocument::bytes)
                .unwrap_or_default();
            let registered_read_only = registry.read_only(path)
                || document.is_some_and(AuthoringDocument::is_read_only)
                || !root_has_feature;
            let mut entry = parse_template_document(
                bytes,
                &path.to_string_lossy(),
                id,
                &registry.required_features,
                registered_read_only,
                self.language_version_kind(),
                &content,
            );
            if !root_has_feature {
                entry.error(
                    "TPL005",
                    &path.to_string_lossy(),
                    1,
                    format!("清单注册模板时必须声明 `{PROJECT_TEMPLATE_REQUIRED_FEATURE}`"),
                );
                entry.read_only = true;
            }
            diagnostics.extend(entry.diagnostics.iter().cloned());
            projects.insert(id.clone(), entry);
        }
        ProjectTemplateIndex {
            builtins,
            projects,
            diagnostics,
        }
    }

    /// 只生成影响预览；此方法不改变 Project、源码、文档或保存基线。
    pub fn preview_template_mutation(
        &self,
        revision: Revision,
        command: &TemplateCommand,
    ) -> Result<ProjectTemplatePreview, String> {
        if command.expected_revision != revision {
            return Err("StaleRevision：模板编辑修订已过期，请重新预览".into());
        }
        if command.expected_baseline != self.content_baseline() {
            return Err("StaleRevision：模板编辑基线已过期，请重新预览".into());
        }
        self.ensure_workspace_writable()?;
        if !self.recovery_conflicts().is_empty() {
            return Err("工程有未解决的保存事务冲突".into());
        }
        ensure_disk_matches_saved_baselines(self)?;
        let (candidate, old, new, changed_files, diagnostics) =
            prepare_template_mutation(self, &command.mutation, command.check_integrity)?;
        let mut preview = ProjectTemplatePreview {
            mutation: command.mutation.clone(),
            expected_revision: revision,
            expected_baseline: command.expected_baseline.clone(),
            field_changes: Vec::new(),
            instances: Vec::new(),
            diagnostics,
            changed_files,
            candidate,
        };
        preview.field_changes = field_changes(old.as_ref(), new.as_ref());
        let content = self.compile_current();
        preview.instances = instance_impacts(
            old.as_ref(),
            new.as_ref(),
            &content,
            command.check_integrity,
            &mut preview.diagnostics,
        );
        Ok(preview)
    }

    /// 仅提交显式预览；基线、修订或任一磁盘保存基线过期时整批拒绝。
    pub fn apply_template_mutation(
        &mut self,
        revision: &mut Revision,
        preview: ProjectTemplatePreview,
    ) -> Result<ProjectTemplateResult, String> {
        if *revision != preview.expected_revision {
            return Err("StaleRevision：模板编辑修订已过期，请重新预览".into());
        }
        if self.content_baseline() != preview.expected_baseline {
            return Err("StaleRevision：模板编辑基线已过期，请重新预览".into());
        }
        self.ensure_workspace_writable()?;
        if !self.recovery_conflicts().is_empty() {
            return Err("工程有未解决的保存事务冲突".into());
        }
        ensure_disk_matches_saved_baselines(self)?;
        let next_revision = revision.next_presentation();
        *self = preview.candidate;
        *revision = next_revision;
        Ok(ProjectTemplateResult {
            changed_files: preview.changed_files,
            new_revision: next_revision,
        })
    }
}
fn prepare_template_mutation(
    project: &Project,
    mutation: &ProjectTemplateMutation,
    check_integrity: bool,
) -> Result<PreparedTemplateMutation, String> {
    let manifest = manifest_path(&project.root);
    let existing_manifest = project
        .authoring_documents
        .get(&manifest)
        .filter(|document| !document.is_deleted());
    if existing_manifest.is_some_and(AuthoringDocument::is_read_only) {
        return Err("工作区清单为只读，不能编辑模板注册".into());
    }
    let (mut manifest_value, registry) = if let Some(document) = existing_manifest {
        let value = parse_unique_json(document.bytes())
            .map_err(|error| format!("工作区清单 JSON 无法安全读取：{error}"))?;
        let registry = parse_registry(&project.root, document.bytes());
        if !registry.diagnostics.is_empty() {
            return Err("工作区清单诊断未修复，不能编辑模板注册".into());
        }
        (value, registry)
    } else {
        let entry = project
            .entry
            .strip_prefix(&project.root)
            .map_err(|_| "工程入口不在工作区内")?
            .to_string_lossy()
            .replace('\\', "/");
        (
            json!({
                "schema_version": 1,
                "project_id": project_id(&project.root),
                "language_version": project.language_version(),
                "entry": entry,
                "required_features": [],
                "templates": {}
            }),
            crate::workspace_documents::Registry::default(),
        )
    };
    let manifest_object = manifest_value
        .as_object_mut()
        .ok_or("工作区清单顶层必须为对象")?;

    let (id, document, operation) = match mutation {
        ProjectTemplateMutation::Import { id, document } => {
            if registry.templates.contains_key(id) {
                return Err("工程模板已注册，必须使用替换操作".into());
            }
            (id.clone(), Some(document.clone()), "import")
        }
        ProjectTemplateMutation::Replace { id, document } => {
            if !crate::workspace_documents::valid_template_id(id) {
                return Err("TPL003：工程模板 ID 必须使用 project: 命名空间".into());
            }
            if !registry.templates.contains_key(id) {
                return Err("待替换工程模板未注册".into());
            }
            (id.clone(), Some(document.clone()), "replace")
        }
        ProjectTemplateMutation::Delete { id } => {
            if !crate::workspace_documents::valid_template_id(id) {
                return Err("TPL003：工程模板 ID 必须使用 project: 命名空间".into());
            }
            if !registry.templates.contains_key(id) {
                return Err("待删除工程模板未注册".into());
            }
            (id.clone(), None, "delete")
        }
    };
    if !crate::workspace_documents::valid_template_id(&id) {
        return Err("TPL003：工程模板 ID 必须使用 project: 命名空间".into());
    }
    if id.starts_with("template_")
        || crate::content_templates::builtin_templates()
            .templates
            .iter()
            .any(|template| template.id == id)
    {
        return Err("内置模板只读，不能覆盖或删除".into());
    }

    let old_path = registry.templates.get(&id).cloned();
    let old_entry = old_path.as_ref().map(|path| {
        let document = project.authoring_documents.get(path);
        parse_template_document(
            document
                .filter(|document| !document.is_deleted())
                .map(AuthoringDocument::bytes)
                .unwrap_or_default(),
            &path.to_string_lossy(),
            &id,
            &registry.required_features,
            registry.read_only(path) || document.is_some_and(AuthoringDocument::is_read_only),
            project.language_version_kind(),
            &project.compile_current(),
        )
    });
    if old_entry.as_ref().is_some_and(|entry| entry.read_only) {
        return Err("模板格式或必需能力未知，只能只读查看".into());
    }
    let old_template = old_entry.as_ref().and_then(|entry| entry.template.clone());

    let mut bytes = document;
    if let (Some(old_path), Some(new_bytes)) = (old_path.as_ref(), bytes.as_mut()) {
        let old = project
            .authoring_documents
            .get(old_path)
            .ok_or("模板文档未载入")?;
        if old.is_read_only() {
            return Err("模板格式或必需能力未知，只能只读查看".into());
        }
        let old_value = parse_unique_json(old.bytes())
            .map_err(|error| format!("TPL001：模板 JSON 无法安全读取：{error}"))?;
        let mut new_value = parse_unique_json(new_bytes)
            .map_err(|error| format!("TPL001：模板 JSON 无法安全读取：{error}"))?;
        preserve_unknown_fields(&old_value, &mut new_value, "root");
        *new_bytes = serde_json::to_vec_pretty(&new_value).map_err(|e| e.to_string())?;
    }

    let mut new_template = None;
    let mut diagnostics = Vec::new();
    let path = if let Some(bytes) = bytes.as_ref() {
        let path = match old_path.as_ref() {
            Some(path) => path.clone(),
            None => {
                let slug = id.strip_prefix("project:").ok_or("模板 ID 无效")?;
                registered_path(&project.root, &format!(".world/templates/{slug}.json"))?
            }
        };
        if old_path.is_none() && registry.documents.contains_key(&path) {
            return Err("模板注册路径与其他展示文档冲突".into());
        }
        let mut parse_features = registry.required_features.clone();
        if operation == "import" {
            parse_features.insert(PROJECT_TEMPLATE_REQUIRED_FEATURE.to_owned());
            if project.language_version_kind().supports_entities() {
                parse_features.insert(OBJECT_REFS_REQUIRED_FEATURE.to_owned());
            }
        }
        let registered_read_only = old_path.is_some()
            && !registry
                .required_features
                .contains(PROJECT_TEMPLATE_REQUIRED_FEATURE);
        let parsed = parse_template_document(
            bytes,
            &path.to_string_lossy(),
            &id,
            &parse_features,
            registered_read_only,
            project.language_version_kind(),
            &project.compile_current(),
        );
        diagnostics.extend(parsed.diagnostics.iter().cloned());
        if parsed
            .template
            .as_ref()
            .is_some_and(|template| template.fields.iter().any(field_contains_character_ref))
            && !registry
                .required_features
                .contains(OBJECT_REFS_REQUIRED_FEATURE)
        {
            return Err(
                "TPL005：人物引用模板必须预先声明 content.object_refs.v1；不会自动开启缺失能力"
                    .into(),
            );
        }
        if parsed.read_only {
            return Err(parsed
                .diagnostics
                .first()
                .map(|diagnostic| format!("{}：{}", diagnostic.code, diagnostic.message))
                .unwrap_or_else(|| "模板文档只读".into()));
        }
        if let Some(diagnostic) = parsed
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity == crate::Severity::Error)
        {
            return Err(format!("{}：{}", diagnostic.code, diagnostic.message));
        }
        if parsed.template.is_none() {
            let diagnostic = parsed.diagnostics.first();
            return Err(diagnostic
                .map(|d| format!("{}：{}", d.code, d.message))
                .unwrap_or_else(|| "TPL004：模板字段无效".into()));
        }
        new_template = parsed.template;
        (path, Some(bytes.clone()))
    } else {
        let path = old_path.clone().ok_or("待删除工程模板未注册")?;
        (path, None)
    };
    if let Some(template) = &new_template {
        if template.id != id {
            return Err("TPL003：模板文档 id 与注册 ID 不一致".into());
        }
    }

    {
        let required_features = manifest_object
            .entry("required_features")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("工作区清单 required_features 必须为数组")?;
        if !required_features
            .iter()
            .any(|feature| feature.as_str() == Some(PROJECT_TEMPLATE_REQUIRED_FEATURE))
        {
            required_features.push(json!(PROJECT_TEMPLATE_REQUIRED_FEATURE));
        }
        if new_template
            .as_ref()
            .is_some_and(|template| template.fields.iter().any(field_contains_object_ref))
            && !required_features
                .iter()
                .any(|feature| feature.as_str() == Some(OBJECT_REFS_REQUIRED_FEATURE))
        {
            required_features.push(json!(OBJECT_REFS_REQUIRED_FEATURE));
        }
    }
    let templates = manifest_object
        .entry("templates")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("工作区清单 templates 必须为对象")?;
    match operation {
        "import" => {
            let relative = path
                .0
                .strip_prefix(&project.root)
                .map_err(|_| "模板路径越界")?;
            templates.insert(
                id.clone(),
                Value::String(relative.to_string_lossy().replace('\\', "/")),
            );
        }
        "replace" => {}
        "delete" => {
            templates.remove(&id);
        }
        _ => unreachable!(),
    }

    let manifest_bytes = serde_json::to_vec_pretty(&manifest_value).map_err(|e| e.to_string())?;
    let mut candidate = project.clone();
    if candidate.authoring_document(&manifest).is_ok() {
        candidate.set_authoring_document(&manifest, manifest_bytes)?;
    } else {
        candidate.create_authoring_document(&manifest, manifest_bytes)?;
    }
    match path.1 {
        Some(bytes) => {
            if candidate.authoring_document(&path.0).is_ok() {
                candidate.set_authoring_document(&path.0, bytes)?;
            } else {
                candidate.create_authoring_document(&path.0, bytes)?;
            }
        }
        None => candidate.delete_authoring_document(&path.0)?,
    }
    let mut changed_files = vec![manifest, path.0];
    changed_files.sort();
    changed_files.dedup();
    let _ = check_integrity;
    Ok((
        candidate,
        old_template,
        new_template,
        changed_files,
        diagnostics,
    ))
}
fn ensure_disk_matches_saved_baselines(project: &Project) -> Result<(), String> {
    for path in project
        .documents
        .keys()
        .chain(project.authoring_documents.keys())
    {
        crate::file_access::within(&project.root, path)?;
        let state = project.tracked_file_state(path).ok_or("文档基线不存在")?;
        let disk = match crate::file_access::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("无法检查文档基线：{error}")),
        };
        if disk != state.baseline {
            return Err(format!(
                "文档已被外部修改，请刷新后重新预览：{}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn project_id(root: &Path) -> String {
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    let mut id = String::from("project_");
    id.extend(name.chars().map(|character| {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
            character
        } else {
            '_'
        }
    }));
    id
}

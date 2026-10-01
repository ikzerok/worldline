use super::{catalog, manifest, preview, CapabilityEnablePlan, CapabilityEnableRequest};
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry};
use std::collections::BTreeSet;

impl Project {
    /// 当前已应用清单的准确能力集合；未知能力仍可展示，但不因此可编辑。
    pub fn required_features(&self) -> Vec<String> {
        self.authoring_documents
            .get(&manifest_path(&self.root))
            .filter(|document| !document.is_deleted())
            .map(|document| parse_registry(&self.root, document.bytes()).required_features)
            .unwrap_or_default()
            .into_iter()
            .collect()
    }

    pub fn plan_capability_enable(
        &self,
        request: &CapabilityEnableRequest,
    ) -> Result<CapabilityEnablePlan, String> {
        self.ensure_workspace_writable()?;
        if request.expected_baseline != self.content_baseline() {
            return Err("语言能力预览的工程基线已过期，请重新预览".into());
        }
        ensure_current_files(self)?;
        let before_features = self.required_features();
        let after_features = validate_request(self, request, &before_features)?;
        let added_features = after_features
            .iter()
            .filter(|feature| !before_features.contains(feature))
            .cloned()
            .collect::<Vec<_>>();
        let path = manifest_path(&self.root);
        let original = self.authoring_documents.get(&path);
        if original.is_some_and(|document| {
            document.is_read_only() || (document.is_deleted() && document.is_dirty())
        }) {
            return Err("清单待删除或只读，请先解决清单状态，不能启用能力".into());
        }
        let before = original
            .filter(|document| !document.is_deleted())
            .map(|document| document.bytes().to_vec());
        let entry = self
            .entry
            .strip_prefix(&self.root)
            .map_err(|_| "工程入口越界")?
            .to_str()
            .ok_or("工程入口路径不是 UTF-8")?
            .replace('\\', "/");
        let manifest_changed =
            self.language_version_kind() != request.target_language || !added_features.is_empty();
        let after = if manifest_changed {
            manifest::prepare(
                before.as_deref(),
                &entry,
                request.target_language,
                &added_features,
            )?
        } else {
            before.clone().unwrap_or_default()
        };
        let mut candidate = self.clone();
        if manifest_changed {
            replace_manifest(&mut candidate, after.clone())?;
        }
        if !candidate.authoring_diagnostics().is_empty() {
            return Err("候选语言能力清单无效，未修改工程".into());
        }
        let current_result = self.compile_current();
        let candidate_result = candidate.compile_current();
        let mut plan = CapabilityEnablePlan {
            request: request.clone(),
            current_language: self.language_version_kind(),
            target_language: request.target_language,
            required_features_before: before_features,
            required_features_after: after_features,
            added_features,
            new_diagnostics: preview::added_diagnostics(
                &current_result.diagnostics,
                &candidate_result.diagnostics,
            ),
            keyword_changes: preview::keyword_changes(&current_result, candidate.compile_options()),
            runtime_fingerprint_before: current_result.analysis.fingerprint,
            runtime_fingerprint_after: candidate_result.analysis.fingerprint,
            fingerprint_comparison_reliable: !current_result.has_errors()
                && !candidate_result.has_errors(),
            compatibility_notes: Vec::new(),
            manifest_changed,
            can_apply: manifest_changed && !candidate_result.has_errors(),
            diagnostics_before: current_result.diagnostics,
            diagnostics_after: candidate_result.diagnostics,
            root: self.root.clone(),
            manifest_before: before,
            manifest_after: after,
        };
        plan.compatibility_notes = preview::compatibility_notes(&plan);
        ensure_current_files(self)?;
        Ok(plan)
    }

    /// 重新生成并核对完整预览；整批提交内存，不保存，不执行故事。
    pub fn apply_capability_enable(&mut self, plan: &CapabilityEnablePlan) -> Result<(), String> {
        self.ensure_workspace_writable()?;
        if self.root != plan.root || self.content_baseline() != plan.request.expected_baseline {
            return Err("语言能力预览已过期或属于其他工作区，未修改工程".into());
        }
        let expected = self.plan_capability_enable(&plan.request)?;
        if serde_json::to_vec(&expected).map_err(|error| error.to_string())?
            != serde_json::to_vec(plan).map_err(|error| error.to_string())?
            || expected.manifest_before != plan.manifest_before
            || expected.manifest_after != plan.manifest_after
        {
            return Err("语言能力计划或完整预览已变化，请重新预览；未修改工程".into());
        }
        if !expected.can_apply {
            return Err("候选有编译错误或没有需要启用的变化，未修改工程".into());
        }
        let mut candidate = self.clone();
        replace_manifest(&mut candidate, expected.manifest_after)?;
        ensure_current_files(self)?;
        *self = candidate;
        Ok(())
    }
}

fn validate_request(
    project: &Project,
    request: &CapabilityEnableRequest,
    before: &[String],
) -> Result<Vec<String>, String> {
    if catalog::rank(request.target_language) < catalog::rank(project.language_version_kind()) {
        return Err("此入口只启用能力，不支持降低语言版本".into());
    }
    let target = super::language_capabilities()
        .iter()
        .find(|capability| capability.version == request.target_language)
        .ok_or("不支持目标语言版本")?;
    let mut after = before.iter().cloned().collect::<BTreeSet<_>>();
    after.extend(
        target
            .required_features
            .iter()
            .map(|feature| feature.to_string()),
    );
    for feature in &request.enable_features {
        if !before.contains(feature)
            && !super::feature_capabilities()
                .iter()
                .any(|capability| capability.id == feature)
        {
            return Err(format!(
                "此入口不能启用能力 `{feature}`；未知能力或展示文档能力不能凭空声明"
            ));
        }
        after.insert(feature.clone());
    }
    for capability in super::feature_capabilities() {
        if !after.contains(capability.id) {
            continue;
        }
        if catalog::rank(request.target_language) < catalog::rank(capability.minimum_language) {
            return Err(format!(
                "能力 `{}` 至少需要显式语言 {}",
                capability.id,
                capability.minimum_language.as_str()
            ));
        }
        for dependency in capability.dependencies {
            if !after.contains(*dependency) {
                return Err(format!(
                    "能力 `{}` 必须同时显式启用 `{dependency}`",
                    capability.id
                ));
            }
        }
    }
    Ok(after.into_iter().collect())
}

fn replace_manifest(project: &mut Project, bytes: Vec<u8>) -> Result<(), String> {
    let path = manifest_path(&project.root);
    if project.authoring_documents.contains_key(&path) {
        project.set_authoring_document(&path, bytes)
    } else {
        project.create_authoring_document(&path, bytes)
    }
}

fn ensure_current_files(project: &Project) -> Result<(), String> {
    project.checkpoint_disk_baselines_match()?;
    let files = match crate::file_access::workspace_files(&project.root) {
        Ok(files) => files,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(format!("无法检查语言能力预览的工作区：{error}")),
    };
    let manifest = manifest_path(&project.root);
    for path in files {
        if (path.extension().is_some_and(|extension| extension == "wl")
            && !project.documents.contains_key(&path))
            || (path == manifest && !project.authoring_documents.contains_key(&path))
        {
            return Err(format!(
                "存在尚未载入的外部源码或清单，请刷新后重新预览：{}",
                path.display()
            ));
        }
    }
    Ok(())
}

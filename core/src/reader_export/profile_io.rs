use super::*;
use crate::workspace_documents::{manifest_path, parse_unique_json};
use serde_json::{json, Value};

impl Project {
    pub fn preview_save_reader_profile(
        &self,
        profile: &ReaderPublicationProfile,
    ) -> Result<ReaderProfileSavePlan, String> {
        Ok(self.prepare_save_reader_profile(profile)?.0)
    }

    // The rehearsal belongs only to this invocation and is never an input token.
    fn prepare_save_reader_profile(
        &self,
        profile: &ReaderPublicationProfile,
    ) -> Result<(ReaderProfileSavePlan, Self), String> {
        self.ensure_workspace_writable()?;
        self.checkpoint_disk_baselines_match()?;
        let profile = super::profile_api::fill_routes(self, profile)?;
        let path = profile_path(self, &profile.id);
        let document_path = authoring_relative_path(self, &path)?;
        let document_before_hash = self
            .authoring_document(&path)
            .ok()
            .filter(|document| !document.is_deleted())
            .map(|document| super::routes::hash_bytes(document.bytes()));
        let content_baseline = self.content_baseline();
        let bytes = serde_json::to_vec(&(
            &profile,
            &content_baseline,
            &document_path,
            &document_before_hash,
        ))
        .map_err(|e| e.to_string())?;
        let plan = ReaderProfileSavePlan {
            profile,
            content_baseline,
            document_path,
            document_before_hash,
            plan_digest: super::routes::hash_bytes(&bytes),
        };
        let mut rehearsal = self.clone();
        write_profile(&mut rehearsal, &plan.profile)?;
        Ok((plan, rehearsal))
    }

    pub fn apply_save_reader_profile(
        &mut self,
        plan: &ReaderProfileSavePlan,
    ) -> Result<(), String> {
        self.checkpoint_disk_baselines_match()?;
        let (current_plan, candidate) = self.prepare_save_reader_profile(&plan.profile)?;
        self.finish_save_reader_profile(plan, current_plan, candidate)
    }

    fn finish_save_reader_profile(
        &mut self,
        plan: &ReaderProfileSavePlan,
        current_plan: ReaderProfileSavePlan,
        candidate: Self,
    ) -> Result<(), String> {
        if current_plan != *plan {
            return Err("发布配置保存预览已过期或被改动，请重新核对".into());
        }
        // Recheck late disk changes, including saved=None for a newly created
        // profile, before adopting this invocation's fully validated rehearsal.
        candidate.checkpoint_disk_baselines_match()?;
        // Equal bytes do not prove distinct file identity. Retain the registry's
        // live path/alias checks from the former second write_profile call.
        let paths = candidate.reader_profile_paths();
        let path = paths
            .get(&plan.profile.id)
            .ok_or("发布配置注册已失效，未应用")?;
        if authoring_relative_path(&candidate, path)? != plan.document_path {
            return Err("发布配置路径已改变，未应用".into());
        }
        for (path, document) in &candidate.authoring_documents {
            if document.is_deleted()
                || self
                    .authoring_documents
                    .get(path)
                    .is_some_and(|previous| !previous.is_deleted())
            {
                continue;
            }
            // File enumeration omits directories. Preserve create's direct-read
            // refusal if a file or directory has appeared at a new target.
            crate::file_access::within(&self.root, path)?;
            match crate::file_access::read(path) {
                Ok(_) => return Err("目标文件已存在，不能覆盖未载入的文件".into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        *self = candidate;
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "profile_io_tests.rs"]
mod tests;

fn profile_path(project: &Project, id: &str) -> PathBuf {
    project
        .reader_profile_paths()
        .get(id)
        .cloned()
        .unwrap_or_else(|| {
            project
                .root
                .join(format!(".world/reader-profiles/{id}.json"))
        })
}

fn write_profile(project: &mut Project, profile: &ReaderPublicationProfile) -> Result<(), String> {
    super::profile_api::validate_profile(profile)?;
    project.ensure_workspace_writable()?;
    let manifest = manifest_path(&project.root);
    if !project.authoring_documents.contains_key(&manifest) {
        let bytes = serde_json::to_vec_pretty(&json!({
            "schema_version":1, "language_version":project.language_version(),
            "required_features":[],
        }))
        .map_err(|e| e.to_string())?;
        project.create_authoring_document(&manifest, bytes)?;
    }
    let manifest_document = project.authoring_document(&manifest)?;
    if manifest_document.is_deleted() || manifest_document.is_read_only() {
        return Err("发布配置需要可写的工程清单".into());
    }
    let mut value = parse_unique_json(manifest_document.bytes()).map_err(|e| e.to_string())?;
    let registrations = project.reader_profile_paths();
    let path = profile_path(project, &profile.id);
    let existing = project
        .authoring_document(&path)
        .ok()
        .filter(|d| !d.is_deleted())
        .cloned();
    let mut stored = if let Some(document) = &existing {
        if document.is_read_only() {
            return Err("发布配置是未知版本或能力，只读保留原文".into());
        }
        let source = parse_unique_json(document.bytes())
            .map_err(|e| format!("原发布配置 JSON 无效：{e}"))?;
        let original: ReaderPublicationProfile =
            serde_json::from_value(source.clone()).map_err(|e| e.to_string())?;
        super::profile_api::validate_profile(&original)?;
        if original.id != profile.id {
            return Err("发布配置原文 ID 与注册不一致".into());
        }
        source
    } else {
        json!({})
    };
    if !registrations.contains_key(&profile.id) {
        let object = value.as_object_mut().ok_or("工程清单必须是对象")?;
        let features = object
            .entry("required_features")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("工程清单 required_features 必须是数组")?;
        if !features
            .iter()
            .any(|f| f.as_str() == Some(READER_PROFILES_FEATURE))
        {
            features.push(json!(READER_PROFILES_FEATURE));
        }
        let entries = object
            .entry("reader_profiles")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or("工程清单 reader_profiles 必须是对象")?;
        if entries.contains_key(&profile.id) {
            return Err("发布配置注册已存在但无效，保留原文".into());
        }
        let relative = authoring_relative_path(project, &path)?;
        entries.insert(profile.id.clone(), Value::String(relative));
        project.set_authoring_document(
            &manifest,
            serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
        )?;
        if !project.authoring_diagnostics().is_empty() {
            return Err("发布配置注册产生清单诊断，未应用".into());
        }
    }
    let fresh = serde_json::to_value(profile).map_err(|e| e.to_string())?;
    let target = stored.as_object_mut().ok_or("发布配置顶层必须是对象")?;
    for (key, value) in fresh.as_object().ok_or("发布配置无法序列化")? {
        target.insert(key.clone(), value.clone());
    }
    let bytes = serde_json::to_vec_pretty(&stored).map_err(|e| e.to_string())?;
    if existing.is_some() {
        project.set_authoring_document(&path, bytes)
    } else {
        project.create_authoring_document(&path, bytes)
    }
}

// 配置原文是工程文档，不是公开 URL；保留合法的普通文件名（例如 a&b.json）。
fn authoring_relative_path(project: &Project, path: &std::path::Path) -> Result<String, String> {
    let raw = path
        .strip_prefix(&project.root)
        .map_err(|_| "发布配置路径越出工程")?
        .to_str()
        .ok_or("发布配置相对路径必须为 UTF-8")?;
    #[cfg(windows)]
    {
        Ok(raw.replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        Ok(raw.to_owned())
    }
}

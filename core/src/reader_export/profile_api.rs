use super::*;
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json};

impl Project {
    pub fn reader_profile_paths(&self) -> BTreeMap<String, PathBuf> {
        self.authoring_document(&manifest_path(&self.root))
            .ok()
            .filter(|document| !document.is_deleted())
            .map(|document| parse_registry(&self.root, document.bytes()).reader_profiles)
            .unwrap_or_default()
    }

    /// 读取注册配置；失效选择保留给作者修复，不自动裁剪。
    pub fn reader_profiles(&self) -> Result<Vec<ReaderPublicationProfile>, String> {
        let mut profiles = Vec::new();
        for (id, path) in self.reader_profile_paths() {
            let document = self.authoring_document(&path)?;
            if document.is_deleted() || document.is_read_only() {
                return Err(format!("发布配置 {id} 缺失或只读，请先核对文档"));
            }
            let profile: ReaderPublicationProfile = serde_json::from_value(
                parse_unique_json(document.bytes())
                    .map_err(|e| format!("发布配置 JSON 无效：{e}"))?,
            )
            .map_err(|e| format!("发布配置结构无效：{e}"))?;
            validate_profile(&profile)?;
            if profile.id != id {
                return Err("发布配置 ID 与清单注册不一致".into());
            }
            profiles.push(profile);
        }
        Ok(profiles)
    }

    pub fn create_reader_profile(
        &self,
        id: &str,
        selection: &ReaderExportSelection,
    ) -> Result<ReaderPublicationProfile, String> {
        let profile = ReaderPublicationProfile {
            schema_version: READER_PROFILE_SCHEMA_VERSION,
            required_features: vec![READER_PROFILES_FEATURE.into()],
            id: id.into(),
            title: selection.site_title.clone(),
            selection: selection.clone(),
            routes: Vec::new(),
        };
        validate_profile(&profile)?;
        fill_routes(self, &profile)
    }

    pub fn preview_reader_profile(
        &self,
        profile: &ReaderPublicationProfile,
    ) -> Result<ReaderExportPreview, String> {
        self.preview_reader_profile_with_progress(profile, &mut |_| true)
    }

    pub fn preview_reader_profile_with_progress(
        &self,
        profile: &ReaderPublicationProfile,
        progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
    ) -> Result<ReaderExportPreview, String> {
        validate_profile(profile)?;
        Ok(
            super::plan::prepare_with_routes(self, &profile.selection, &profile.routes, progress)?
                .preview,
        )
    }

    pub fn build_reader_profile(
        &self,
        profile: &ReaderPublicationProfile,
        expected_plan_digest: &str,
    ) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
        self.build_reader_profile_with_progress(profile, expected_plan_digest, &mut |_| true)
    }

    pub fn build_reader_profile_with_progress(
        &self,
        profile: &ReaderPublicationProfile,
        expected_plan_digest: &str,
        progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
    ) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
        validate_profile(profile)?;
        let prepared =
            super::plan::prepare_with_routes(self, &profile.selection, &profile.routes, progress)?;
        super::progress::build_prepared(prepared, expected_plan_digest, progress)
    }

    pub fn preview_reader_profile_migration(
        &self,
        profile: &ReaderPublicationProfile,
    ) -> Result<ReaderProfileMigrationPlan, String> {
        let before = fill_routes(self, profile)?;
        let mut after = before.clone();
        let mut changes = Vec::new();
        if after.selection.schema_version != READER_SITE_SCHEMA_VERSION {
            after.selection.schema_version = READER_SITE_SCHEMA_VERSION;
            after
                .selection
                .required_features
                .push(READER_SITE_FEATURE.into());
            changes.push("已选对象将公开别名和类型结构；字段、附件及故事效果仍需另外授权".into());
        }
        let content_baseline = self.content_baseline();
        let bytes = serde_json::to_vec(&(&before, &after, &changes, &content_baseline))
            .map_err(|e| e.to_string())?;
        Ok(ReaderProfileMigrationPlan {
            before,
            after,
            authorization_changes: changes,
            content_baseline,
            plan_digest: super::routes::hash_bytes(&bytes),
        })
    }

    pub fn apply_reader_profile_migration(
        &self,
        plan: &ReaderProfileMigrationPlan,
    ) -> Result<ReaderPublicationProfile, String> {
        if self.preview_reader_profile_migration(&plan.before)? != *plan {
            return Err("发布配置迁移预览已过期或被改动".into());
        }
        Ok(plan.after.clone())
    }
}

pub(super) fn validate_profile(profile: &ReaderPublicationProfile) -> Result<(), String> {
    if profile.schema_version != READER_PROFILE_SCHEMA_VERSION
        || profile.required_features != [READER_PROFILES_FEATURE]
    {
        return Err("不支持的发布配置版本或必需能力，只读保留原文".into());
    }
    if !crate::workspace_documents::valid_reader_profile_id(&profile.id) {
        return Err("发布配置 ID 必须为 1 至 80 个 ASCII 字母、数字、下划线或连字符".into());
    }
    if profile.title.trim().is_empty() || profile.title.chars().count() > 160 {
        return Err("发布配置标题必须为 1 至 160 个字符".into());
    }
    super::routes::validate_profile_routes(&profile.routes)
}

pub(super) fn fill_routes(
    project: &Project,
    profile: &ReaderPublicationProfile,
) -> Result<ReaderPublicationProfile, String> {
    validate_profile(profile)?;
    let preview = project.preview_reader_profile(profile)?;
    let mut updated = profile.clone();
    for entry in preview.included {
        let route = super::routes::route_from_included(&entry);
        if !updated
            .routes
            .iter()
            .any(|existing| super::routes::same_identity(existing, &route))
        {
            updated.routes.push(route);
        }
    }
    validate_profile(&updated)?;
    Ok(updated)
}

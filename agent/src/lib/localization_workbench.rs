//! Stateful, in-memory authoring; the separate project.save remains explicit.
use super::*;
use worldline_core::localization::{
    LocalizationCatalogQuery, LocalizationEditDraft, LocalizationError, LocalizationIdDraft,
    LocalizationImportDraft,
};

#[cfg(test)]
#[path = "localization_workbench/tests.rs"]
mod tests;

impl Server {
    pub(super) fn localization_workbench(
        &mut self,
        params: &Value,
        kind: &str,
        apply: bool,
    ) -> Result<Value, ProtoError> {
        let object = params
            .as_object()
            .ok_or_else(|| ProtoError::new(-32602, "参数必须是对象"))?;
        if object.keys().any(|key| {
            !matches!(key.as_str(), "project_id" | "request") && !(apply && key == "plan_digest")
        }) {
            return Err(ProtoError::new(-32602, "本地化请求含未知参数"));
        }
        let id = param_str(params, "project_id")?;
        let request = params
            .get("request")
            .ok_or_else(|| ProtoError::new(-32602, "需要 request DTO"))?;
        if serde_json::to_vec(request)
            .map_err(|e| ProtoError::new(-32602, e.to_string()))?
            .len()
            > worldline_core::localization::MAX_LOCALIZATION_JSON_BYTES
        {
            return Err(ProtoError::new(-32602, "本地化请求超过 8 MiB 限制"));
        }
        let input = Input::decode(kind, request.clone())?;
        let digest = if apply {
            Some(param_str(params, "plan_digest")?)
        } else {
            None
        };
        let unit = self
            .projects
            .get_mut(id)
            .ok_or_else(|| ProtoError::new(-32602, "未知 project_id"))?;
        let mut payload = match input.execute(&mut unit.project, digest) {
            Ok(value) => value,
            Err(error) => json!({"ok":false,"error":error,"applied":false,"saved":false}),
        };
        payload["baseline"] = json!(unit.project.content_baseline());
        payload["workspace_diagnostics"] = json!(unit.project.authoring_diagnostics());
        payload["read_only"] = payload
            .get("page")
            .and_then(|page| page.get("read_only"))
            .cloned()
            .unwrap_or_else(|| json!(!unit.project.authoring_diagnostics().is_empty()));
        Ok(payload)
    }
}

enum Input {
    Catalog(LocalizationCatalogQuery),
    Ids(LocalizationIdDraft),
    Edit(LocalizationEditDraft),
    Import(LocalizationImportDraft),
}

impl Input {
    fn decode(kind: &str, value: Value) -> Result<Self, ProtoError> {
        match kind {
            "catalog" => serde_json::from_value(value).map(Self::Catalog),
            "ids" => serde_json::from_value(value).map(Self::Ids),
            "edit" => serde_json::from_value(value).map(Self::Edit),
            "import" => serde_json::from_value(value).map(Self::Import),
            _ => unreachable!("internal method routing"),
        }
        .map_err(|error| ProtoError::new(-32602, format!("本地化 request DTO 无效：{error}")))
    }

    fn execute(
        self,
        project: &mut Project,
        digest: Option<&str>,
    ) -> Result<Value, LocalizationError> {
        let applied = digest.is_some();
        let operation = if applied { "apply" } else { "preview" };
        let mut payload = match self {
            Self::Catalog(query) => {
                return project.query_localization_catalog(&query)
                    .map(|page| json!({"ok":true,"page":page,"applied":false,"saved":false}));
            }
            Self::Ids(draft) => match digest {
                Some(digest) => project.apply_localization_ids(&draft, digest)
                    .map(|result| json!({"plan":result.plan,"changed_files":result.changed_files,"new_baseline":result.new_baseline})),
                None => project.preview_localization_ids(&draft).map(|plan| json!({"plan":plan})),
            },
            Self::Edit(draft) => match digest {
                Some(digest) => project.apply_localization_edit(&draft, digest)
                    .map(|result| json!({"plan":result.plan,"changed_files":result.changed_files,"new_baseline":result.new_baseline})),
                None => project.preview_localization_edit(&draft).map(|plan| json!({"plan":plan})),
            },
            Self::Import(draft) => match digest {
                Some(digest) => project.apply_localization_import_candidate(&draft.selection, &draft.exchange, digest)
                    .map(|result| json!({"plan":result.plan,"changed_files":result.changed_files,"new_baseline":result.new_baseline})),
                None => project.preview_localization_import_candidate(&draft.selection, &draft.exchange)
                    .map(|plan| json!({"plan":plan})),
            },
        }?;
        payload["ok"] = json!(true);
        payload["operation"] = json!(operation);
        payload["applied"] = json!(applied);
        payload["saved"] = json!(false);
        Ok(payload)
    }
}

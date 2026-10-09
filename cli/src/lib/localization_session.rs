//! CLI selection and core preparation only; execution stays in runtime.
use super::*;
use worldline_core::localization::{
    LocalizationPresentationPolicy, LocalizationPresentationRequest,
    LocalizationPresentationSnapshot,
};

pub(super) fn for_play(
    args: &FileArgs,
    snapshot: &CompileSnapshot,
) -> Result<Option<LocalizationPresentationSnapshot>, String> {
    args.locale
        .as_ref()
        .map(|locale| {
            prepare(
                snapshot.project.as_ref(),
                &LocalizationPresentationRequest {
                    schema_version: 1,
                    target_locale: locale.clone(),
                    policy: if args.locale_fallback {
                        LocalizationPresentationPolicy::SourceFallback
                    } else {
                        LocalizationPresentationPolicy::Strict
                    },
                },
            )
        })
        .transpose()
}

pub(super) fn for_trace(
    project: Option<&Project>,
    trace: &ReplayTrace,
) -> Result<Option<LocalizationPresentationSnapshot>, String> {
    trace
        .presentation
        .as_ref()
        .map(|identity| prepare(project, &identity.request))
        .transpose()
}

fn prepare(
    project: Option<&Project>,
    request: &LocalizationPresentationRequest,
) -> Result<LocalizationPresentationSnapshot, String> {
    project
        .ok_or("locale 体验需要带本地化文档的工程，请从工程目录编译")?
        .prepare_localization_presentation(request)
        .map_err(|error| format!("{}：{}", error.code, error.message))
}

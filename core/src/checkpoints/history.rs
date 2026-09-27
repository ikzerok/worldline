use super::*;
pub(super) fn publish_project_checkpoint(
    project: &Project,
    manifest: CheckpointManifest,
    files: &Files,
    text_base: &BTreeMap<PathBuf, Option<Vec<u8>>>,
    limits: &CheckpointLimits,
) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        super::native::publish_checkpoint(&project.root, manifest, files, text_base, limits)
    }
    #[cfg(target_arch = "wasm32")]
    {
        super::wasm::publish_checkpoint(
            &project.root,
            &project.checkpoint_session_id,
            manifest,
            files,
            text_base,
            limits,
        )
    }
}

pub(super) fn list_project_checkpoint_records(
    project: &Project,
) -> Result<Vec<CheckpointListing>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        super::native::list_checkpoint_records(&project.root)
    }
    #[cfg(target_arch = "wasm32")]
    {
        super::wasm::list_checkpoint_records(&project.root, &project.checkpoint_session_id)
    }
}

pub(super) fn load_project_checkpoint(
    project: &Project,
    id: &str,
) -> Result<CheckpointBundle, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        super::native::load_checkpoint(&project.root, id)
    }
    #[cfg(target_arch = "wasm32")]
    {
        super::wasm::load_checkpoint(&project.root, &project.checkpoint_session_id, id)
    }
}

pub(super) fn delete_project_checkpoint_record(project: &Project, id: &str) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        super::native::delete_checkpoint_record(&project.root, id)
    }
    #[cfg(target_arch = "wasm32")]
    {
        super::wasm::delete_checkpoint_record(&project.root, &project.checkpoint_session_id, id)
    }
}

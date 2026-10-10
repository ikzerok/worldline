use super::*;
use std::path::Path;

pub fn write_production_script_new(
    workspace: &Path,
    destination: &Path,
    artifact: &ProductionArtifact,
    before_publish: &mut dyn FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let extension = match artifact.format() {
        ProductionFormat::Json => "json",
        ProductionFormat::Markdown => "md",
        ProductionFormat::Csv => "csv",
    };
    crate::manuscript::write_private_bytes_new(
        workspace,
        destination,
        artifact.bytes(),
        extension,
        before_publish,
    )
}

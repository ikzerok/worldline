use super::*;
use std::collections::{BTreeSet, HashSet};
use std::path::Path;
pub(super) fn preview_restore(
    project: &Project,
    checkpoint: CheckpointBundle,
) -> Result<CheckpointRestorePlan, String> {
    super::capture::ensure_workspace_snapshot_limits(project, DEFAULT_MAX_CHECKPOINT_BYTES as u64)?;
    let current_files = crate::workspace_snapshot::snapshot_files(project)?;
    super::capture::validate_snapshot_files(&current_files)?;
    let disk_files = disk_workspace_files(&project.root)?;
    let (current_compile, current_complete) = compile_snapshot(project, &current_files);
    let (checkpoint_compile, checkpoint_complete) = compile_snapshot(project, &checkpoint.files);
    let current_objects = objects_by_source(&current_compile);
    let checkpoint_objects = objects_by_source(&checkpoint_compile);
    let before_fingerprint = current_compile.analysis.fingerprint;
    let after_fingerprint = checkpoint_compile.analysis.fingerprint;
    let mut changed_paths = BTreeSet::new();
    changed_paths.extend(current_files.keys().cloned());
    changed_paths.extend(checkpoint.files.keys().cloned());
    let mut changes = Vec::new();
    for path in changed_paths {
        let current = current_files.get(&path);
        let restored = checkpoint.files.get(&path);
        if current == restored {
            continue;
        }
        let operation = match (current, restored) {
            (None, Some(_)) => CheckpointFileOperation::Added,
            (Some(_), None) => CheckpointFileOperation::Deleted,
            (Some(_), Some(_)) => CheckpointFileOperation::Modified,
            (None, None) => continue,
        };
        let source_file = path.extension().is_some_and(|extension| extension == "wl");
        let mut affected_objects = BTreeSet::new();
        if source_file {
            let absolute = crate::compiler::source_path(&project.root.join(&path));
            for objects in [&current_objects, &checkpoint_objects] {
                if let Some(objects) = objects.get(&absolute) {
                    affected_objects.extend(objects.iter().cloned());
                }
            }
        }
        changes.push(CheckpointFileChange {
            path,
            operation,
            current_bytes: current.map(|bytes| bytes.len() as u64),
            checkpoint_bytes: restored.map(|bytes| bytes.len() as u64),
            affected_objects: affected_objects.into_iter().collect(),
            objects_complete: !source_file
                || (current_complete
                    && checkpoint_complete
                    && !current_compile.has_errors()
                    && !checkpoint_compile.has_errors()),
        });
    }
    let (text_differences, text_differences_truncated) =
        checkpoint_text_differences(&current_files, &checkpoint);
    Ok(CheckpointRestorePlan {
        checkpoint_id: checkpoint.manifest.id.clone(),
        expected_content_baseline: project.content_baseline(),
        expected_workspace_digest: super::capture::files_digest(&current_files),
        expected_disk_digest: super::capture::files_digest(&disk_files),
        checkpoint_digest: super::capture::checkpoint_record_digest(&checkpoint.manifest),
        fingerprint_before: before_fingerprint,
        fingerprint_after: after_fingerprint,
        changes,
        text_differences,
        text_differences_truncated,
    })
}
fn checkpoint_text_differences(
    current_files: &Files,
    checkpoint: &CheckpointBundle,
) -> (Vec<CheckpointTextDiff>, bool) {
    let mut paths = BTreeSet::new();
    for files in [current_files, &checkpoint.files] {
        paths.extend(
            files
                .keys()
                .filter(|path| path.extension().is_some_and(|extension| extension == "wl"))
                .cloned(),
        );
    }
    if let Some(bases) = &checkpoint.text_base {
        paths.extend(bases.keys().cloned());
    }

    let mut result = Vec::new();
    let mut truncated_files = false;
    for path in paths {
        let current = current_files.get(&path);
        let target = checkpoint.files.get(&path);
        let captured_base = checkpoint
            .text_base
            .as_ref()
            .and_then(|bases| bases.get(&path));
        let base_available = captured_base.is_some();
        let base = captured_base.and_then(Option::as_ref);
        let differs = if base_available {
            base != current && base != target || current != target
        } else {
            current != target
        };
        if !differs {
            continue;
        }
        if result.len() == MAX_CHECKPOINT_TEXT_FILES {
            truncated_files = true;
            break;
        }
        result.push(project_checkpoint_text_diff(
            path,
            base_available,
            base.map(Vec::as_slice),
            current.map(Vec::as_slice),
            target.map(Vec::as_slice),
        ));
    }
    (result, truncated_files)
}

fn project_checkpoint_text_diff(
    path: PathBuf,
    base_available: bool,
    base: Option<&[u8]>,
    current: Option<&[u8]>,
    checkpoint: Option<&[u8]>,
) -> CheckpointTextDiff {
    let decoded_base = base.map(std::str::from_utf8).transpose();
    let decoded_current = current.map(std::str::from_utf8).transpose();
    let decoded_checkpoint = checkpoint.map(std::str::from_utf8).transpose();
    let undecodable =
        decoded_base.is_err() || decoded_current.is_err() || decoded_checkpoint.is_err();
    let mut truncated = false;
    let (differences, alignment_uncertain, raw) = if undecodable {
        (
            Vec::new(),
            true,
            CheckpointTextSourceSnippets {
                base: base.map(|bytes| escaped_byte_snippet(bytes, &mut truncated)),
                current: current.map(|bytes| escaped_byte_snippet(bytes, &mut truncated)),
                checkpoint: checkpoint.map(|bytes| escaped_byte_snippet(bytes, &mut truncated)),
            },
        )
    } else if !base_available {
        (
            Vec::new(),
            true,
            CheckpointTextSourceSnippets {
                base: None,
                current: decoded_current
                    .ok()
                    .flatten()
                    .map(|text| bounded_text(text, &mut truncated)),
                checkpoint: decoded_checkpoint
                    .ok()
                    .flatten()
                    .map(|text| bounded_text(text, &mut truncated)),
            },
        )
    } else {
        let (proposal_differences, review_truncated, uncertain, raw) =
            crate::collaboration::review_checkpoint_text(
                decoded_base.ok().flatten(),
                decoded_current.ok().flatten(),
                decoded_checkpoint.ok().flatten(),
            );
        truncated |= review_truncated;
        let mut differences = proposal_differences
            .into_iter()
            .map(|difference| CheckpointTextDifference {
                path: path.clone(),
                base: difference.base,
                current: difference.current,
                checkpoint: difference.proposed,
                base_range: difference.base_range.map(checkpoint_source_range),
                current_range: difference.current_range.map(checkpoint_source_range),
                checkpoint_range: difference.proposed_range.map(checkpoint_source_range),
            })
            .collect::<Vec<_>>();
        truncated |= limit_checkpoint_hunks(&mut differences);
        (
            differences,
            uncertain,
            CheckpointTextSourceSnippets {
                base: raw.base,
                current: raw.current,
                checkpoint: raw.proposed,
            },
        )
    };
    let summary = CheckpointTextDiffSummary {
        base_bytes: base.map(|bytes| bytes.len() as u64),
        current_bytes: current.map(|bytes| bytes.len() as u64),
        checkpoint_bytes: checkpoint.map(|bytes| bytes.len() as u64),
        base_lines: if !base_available || decoded_base.is_err() {
            None
        } else {
            Some(
                decoded_base
                    .ok()
                    .flatten()
                    .map_or(0, |text| text.lines().count()),
            )
        },
        current_lines: if decoded_current.is_err() {
            None
        } else {
            Some(
                decoded_current
                    .ok()
                    .flatten()
                    .map_or(0, |text| text.lines().count()),
            )
        },
        checkpoint_lines: if decoded_checkpoint.is_err() {
            None
        } else {
            Some(
                decoded_checkpoint
                    .ok()
                    .flatten()
                    .map_or(0, |text| text.lines().count()),
            )
        },
        difference_count: differences.len(),
    };
    CheckpointTextDiff {
        path,
        base_available,
        summary,
        differences,
        raw,
        alignment_uncertain,
        undecodable,
        truncated,
    }
}

fn checkpoint_source_range(
    range: crate::collaboration::ProposalSourceRange,
) -> CheckpointTextSourceRange {
    CheckpointTextSourceRange {
        start_byte: range.start_byte,
        end_byte: range.end_byte,
    }
}

fn bounded_text(value: &str, truncated: &mut bool) -> String {
    if value.len() <= MAX_CHECKPOINT_TEXT_BYTES {
        return value.to_owned();
    }
    *truncated = true;
    let mut end = MAX_CHECKPOINT_TEXT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn escaped_byte_snippet(bytes: &[u8], truncated: &mut bool) -> String {
    const PREFIX: &str = "hex:";
    let max_bytes = (MAX_CHECKPOINT_TEXT_BYTES - PREFIX.len()) / 2;
    let shown = bytes.len().min(max_bytes);
    if shown < bytes.len() {
        *truncated = true;
    }
    let mut output = String::with_capacity(PREFIX.len() + shown * 2);
    output.push_str(PREFIX);
    for byte in &bytes[..shown] {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn limit_checkpoint_hunks(differences: &mut [CheckpointTextDifference]) -> bool {
    let mut remaining = MAX_CHECKPOINT_TEXT_BYTES;
    let mut truncated = false;
    for difference in differences {
        for snippet in [
            &mut difference.base,
            &mut difference.current,
            &mut difference.checkpoint,
        ] {
            let Some(text) = snippet else {
                continue;
            };
            if text.len() <= remaining {
                remaining -= text.len();
                continue;
            }
            let mut end = remaining;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            remaining = 0;
            truncated = true;
        }
    }
    truncated
}
fn objects_by_source(result: &crate::CompileResult) -> BTreeMap<PathBuf, BTreeSet<TargetRef>> {
    let mut objects_by_source = BTreeMap::new();
    for object in &result.analysis.catalog.objects {
        objects_by_source
            .entry(crate::compiler::source_path(Path::new(&object.file)))
            .or_insert_with(BTreeSet::new)
            .insert(object.target.clone());
    }
    objects_by_source
}

fn compile_snapshot(project: &Project, files: &Files) -> (crate::CompileResult, bool) {
    let manifest_path = Path::new(".world/project.json");
    let registry = files
        .get(manifest_path)
        .map(|manifest| crate::workspace_documents::parse_registry(&project.root, manifest))
        .unwrap_or_default();
    let mut complete = registry.diagnostics.is_empty();
    let mut sources = BTreeMap::new();
    for (relative, bytes) in files {
        if relative
            .extension()
            .is_some_and(|extension| extension == "wl")
        {
            match String::from_utf8(bytes.clone()) {
                Ok(text) => {
                    sources.insert(
                        crate::compiler::source_path(&project.root.join(relative)),
                        text,
                    );
                }
                Err(_) => complete = false,
            }
        }
    }
    let active = registry.source_selection.as_ref();
    let inactive = sources
        .keys()
        .filter(|path| active.is_some_and(|selection| !selection.is_active(path)))
        .cloned()
        .collect::<HashSet<_>>();
    let deleted = project
        .documents
        .keys()
        .filter(|path| !sources.contains_key(*path))
        .cloned()
        .collect::<HashSet<_>>();
    let result = crate::compiler::compile_sources_excluding_inactive_with_options(
        &project.entry,
        &sources,
        deleted,
        inactive,
        crate::CompileOptions::new(registry.language_version),
    );
    (result, complete)
}
pub(super) fn disk_workspace_files(root: &Path) -> Result<Files, String> {
    let paths = match crate::file_access::workspace_files(root) {
        Ok(paths) => paths,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Files::new()),
        Err(error) => return Err(format!("无法读取工作区文件清单：{error}")),
    };
    super::capture::ensure_disk_file_limits(&paths, DEFAULT_MAX_CHECKPOINT_BYTES as u64)?;
    let mut files = Files::new();
    for path in paths {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| format!("工作区文件越界：{}", path.display()))?
            .to_path_buf();
        super::capture::validate_relative_file(&relative)?;
        let bytes = crate::file_access::read(&path)
            .map_err(|error| format!("无法读取工作区文件 {}：{error}", path.display()))?;
        files.insert(relative, bytes);
    }
    super::capture::validate_snapshot_files(&files)?;
    Ok(files)
}

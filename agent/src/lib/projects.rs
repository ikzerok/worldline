use super::*;
impl Server {
    pub(super) fn project_open(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let path = param_str(params, "path")?;
        let path = Path::new(path);
        let entry = match project_entry(path) {
            Ok(entry) => entry,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        let mut project = match Project::open(path) {
            Ok(project) => project,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        let result = project.compile();
        let baseline = project.content_baseline();
        self.next_project += 1;
        let project_id = format!("p{}", self.next_project);
        let mut response = project_view(
            &result,
            Some(project_id.clone()),
            baseline,
            project.authoring_diagnostics(),
        );
        response["revision"] = json!(worldline_core::presentation_commands::Revision::default());
        self.projects.insert(
            project_id,
            ProjectUnit {
                project,
                entry,
                scene_revision: Default::default(),
                problems_report: None,
            },
        );
        Ok(response)
    }

    pub(super) fn project_analyze(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let id = param_str(params, "project_id")?;
        let unit = self
            .projects
            .get_mut(id)
            .ok_or_else(|| ProtoError::new(-32602, format!("未知 project_id `{id}`")))?;
        let snapshot = match refreshed_workspace(&mut unit.project) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let result = unit.project.compile();
                return Ok(project_failure_with_workspace(
                    "IO_ERROR",
                    format!("刷新工程失败：{error}"),
                    Some(&result.diagnostics),
                    Some(unit.project.content_baseline()),
                    Some(result.options.language_version.as_str()),
                    unit.project.authoring_diagnostics(),
                ));
            }
        };
        let mut response = project_view(
            &snapshot.result,
            None,
            snapshot.baseline,
            &snapshot.workspace_diagnostics,
        );
        response["maps"] = json!(&snapshot.map_index.maps);
        response["references"] = json!(map_references(&snapshot.map_index));
        if !snapshot.conflicts.is_empty() {
            response["conflicts"] = json!(snapshot.conflicts);
        }
        Ok(response)
    }

    pub(super) fn workspace_check(&mut self, params: &Value) -> Result<Value, ProtoError> {
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return Ok(workspace_check_project(&mut unit.project));
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => {
                return Ok(query_failure(
                    "IO_ERROR",
                    error.to_string(),
                    None,
                    None,
                    None,
                    &[],
                ))
            }
        };
        Ok(workspace_check_project(&mut project))
    }

    pub(super) fn maps_list(&mut self, params: &Value) -> Result<Value, ProtoError> {
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return Ok(maps_list_project(&mut unit.project));
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => {
                return Ok(query_failure(
                    "IO_ERROR",
                    error.to_string(),
                    None,
                    None,
                    None,
                    &[],
                ))
            }
        };
        Ok(maps_list_project(&mut project))
    }
}
pub(super) fn refreshed_workspace(project: &mut Project) -> Result<WorkspaceSnapshot, String> {
    let conflicts = project.refresh().map_err(|error| error.to_string())?;
    let result = project.compile();
    let map_index = project.map_index();
    let mut workspace_diagnostics = project.authoring_diagnostics().to_vec();
    for diagnostic in &map_index.diagnostics {
        if !workspace_diagnostics.iter().any(|existing| {
            existing.severity == diagnostic.severity
                && existing.code == diagnostic.code
                && existing.message == diagnostic.message
                && existing.file == diagnostic.file
                && existing.span == diagnostic.span
        }) {
            workspace_diagnostics.push(diagnostic.clone());
        }
    }
    Ok(WorkspaceSnapshot {
        baseline: project.content_baseline(),
        result,
        map_index,
        workspace_diagnostics,
        conflicts,
    })
}

pub(super) fn query_payload_base(snapshot: &WorkspaceSnapshot) -> serde_json::Map<String, Value> {
    let read_only = !snapshot.workspace_diagnostics.is_empty();
    serde_json::Map::from_iter([
        ("schema_version".into(), json!(1)),
        (
            "language_version".into(),
            json!(snapshot.result.options.language_version.as_str()),
        ),
        ("workspace_revision".into(), json!(snapshot.baseline)),
        ("diagnostics".into(), json!(snapshot.result.diagnostics)),
        (
            "workspace_diagnostics".into(),
            json!(snapshot.workspace_diagnostics),
        ),
        ("read_only".into(), json!(read_only)),
        ("truncated".into(), json!(false)),
        ("continuation".into(), Value::Null),
    ])
}
pub(super) fn query_failure(
    code: &str,
    message: String,
    diagnostics: Option<&Vec<Diagnostic>>,
    baseline: Option<String>,
    language_version: Option<&str>,
    workspace_diagnostics: &[Diagnostic],
) -> Value {
    json!({
        "ok": false,
        "schema_version": 1,
        "error": {"code": code, "message": message},
        "language_version": language_version,
        "workspace_revision": baseline,
        "diagnostics": diagnostics.cloned().unwrap_or_default(),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
        "truncated": false,
        "continuation": Value::Null,
    })
}

fn map_references(index: &worldline_core::MapIndex) -> Vec<Value> {
    index
        .placements_by_target
        .iter()
        .map(|(target, placements)| json!({"target": target, "placements": placements}))
        .collect()
}

fn workspace_check_project(project: &mut Project) -> Value {
    let snapshot = match refreshed_workspace(project) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let result = project.compile();
            return query_failure(
                "IO_ERROR",
                format!("刷新工程失败：{error}"),
                Some(&result.diagnostics),
                Some(project.content_baseline()),
                Some(result.options.language_version.as_str()),
                project.authoring_diagnostics(),
            );
        }
    };
    let ok = !snapshot.result.has_errors();
    let mut payload = query_payload_base(&snapshot);
    payload.insert("ok".into(), json!(ok));
    payload.insert("stats".into(), json!(snapshot.result.analysis.stats));
    if !snapshot.conflicts.is_empty() {
        payload.insert("conflicts".into(), json!(snapshot.conflicts));
    }
    Value::Object(payload)
}

fn maps_list_project(project: &mut Project) -> Value {
    let snapshot = match refreshed_workspace(project) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let result = project.compile();
            let mut response = query_failure(
                "IO_ERROR",
                format!("刷新工程失败：{error}"),
                Some(&result.diagnostics),
                Some(project.content_baseline()),
                Some(result.options.language_version.as_str()),
                project.authoring_diagnostics(),
            );
            response["maps"] = json!({});
            response["references"] = json!([]);
            return response;
        }
    };
    let mut payload = query_payload_base(&snapshot);
    payload.insert("ok".into(), json!(!snapshot.result.has_errors()));
    payload.insert("maps".into(), json!(&snapshot.map_index.maps));
    payload.insert(
        "references".into(),
        json!(map_references(&snapshot.map_index)),
    );
    if !snapshot.conflicts.is_empty() {
        payload.insert("conflicts".into(), json!(snapshot.conflicts));
    }
    Value::Object(payload)
}

fn project_view(
    result: &CompileResult,
    project_id: Option<String>,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
) -> Value {
    let mut payload = json!({
        "language_version": result.options.language_version.as_str(),
        "baseline": baseline,
        "catalog": &result.analysis.catalog,
        "diagnostics": result.diagnostics,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
    });
    payload["ok"] = json!(!result.has_errors());
    if let Some(project_id) = project_id {
        payload["project_id"] = json!(project_id);
    }
    payload
}

pub(super) fn project_failure(
    code: &str,
    message: String,
    diagnostics: Option<&Vec<worldline_core::Diagnostic>>,
    baseline: Option<String>,
    language_version: Option<&str>,
) -> Value {
    project_failure_with_workspace(code, message, diagnostics, baseline, language_version, &[])
}

pub(super) fn project_failure_with_workspace(
    code: &str,
    message: String,
    diagnostics: Option<&Vec<Diagnostic>>,
    baseline: Option<String>,
    language_version: Option<&str>,
    workspace_diagnostics: &[Diagnostic],
) -> Value {
    json!({
        "ok": false,
        "error": { "code": code, "message": message },
        "diagnostics": diagnostics.cloned().unwrap_or_default(),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
        "baseline": baseline,
        "language_version": language_version,
    })
}
pub(super) fn project_entry(path: &Path) -> Result<PathBuf, String> {
    let entry = if path.is_dir() {
        path.join("world.wl")
    } else {
        path.to_path_buf()
    };
    std::fs::canonicalize(&entry)
        .map_err(|error| format!("无法定位工程入口 {}: {error}", entry.display()))
}

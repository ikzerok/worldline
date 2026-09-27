use super::*;
impl Server {
    pub(super) fn compile(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let options = compile_options(params)?;
        let input = if let Some(p) = params.get("path").and_then(Value::as_str) {
            match compile_path_input(Path::new(p), params, options) {
                Ok(r) => r,
                Err(e) => {
                    return Ok(json!({
                        "ok": false,
                        "error": {
                            "code": "IO_ERROR",
                            "message": format!("无法读取 {p}:{e}"),
                        },
                        "workspace_diagnostics": [],
                        "read_only": false,
                    }))
                }
            }
        } else if let Some(src) = params.get("source").and_then(Value::as_str) {
            let name = params
                .get("file_name")
                .and_then(Value::as_str)
                .unwrap_or("未命名.wl");
            if params.get("language_version").is_some()
                || params
                    .get("options")
                    .and_then(|value| value.get("language_version"))
                    .is_some()
            {
                CompileInput::plain(compile_source_with_options(name, src, options))
            } else {
                CompileInput::plain(compile_source(name, src))
            }
        } else {
            return Err(ProtoError::new(-32602, "需要 `path` 或 `source` 参数"));
        };
        let CompileInput {
            result,
            workspace_diagnostics,
        } = input;
        let read_only = !workspace_diagnostics.is_empty();
        if result.has_errors() {
            return Ok(json!({
                "ok": false,
                "diagnostics": result.diagnostics,
                "language_version": result.options.language_version.as_str(),
                "workspace_diagnostics": workspace_diagnostics,
                "read_only": read_only,
            }));
        }
        self.next_story += 1;
        let story_id = format!("s{}", self.next_story);
        let program: &'static Program = Box::leak(Box::new(result.program));
        let analysis: &'static Analysis = Box::leak(Box::new(result.analysis));
        self.stories.insert(
            story_id.clone(),
            StoryUnit {
                program,
                analysis,
                language_version: result.options.language_version,
            },
        );
        Ok(json!({
            "ok": true,
            "story_id": story_id,
            "fingerprint": analysis.fingerprint,
            "stats": analysis.stats,
            "diagnostics": result.diagnostics,
            "language_version": result.options.language_version.as_str(),
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": read_only,
        }))
    }

    pub(super) fn story(&self, id: &str) -> Result<&StoryUnit, ProtoError> {
        self.stories
            .get(id)
            .ok_or_else(|| ProtoError::new(-32602, format!("未知 story_id `{id}`")))
    }

    pub(super) fn session(&mut self, params: &Value) -> Result<&mut Session, ProtoError> {
        let id = param_str(params, "session_id")?;
        self.sessions
            .get_mut(id)
            .ok_or_else(|| ProtoError::new(-32602, format!("未知 session_id `{id}`")))
    }

    pub(super) fn session_open(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let story_id = param_str(params, "story_id")?.to_string();
        let seed = match params.get("seed") {
            None => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .ok_or_else(|| ProtoError::new(-32602, "`seed` 必须是非负 64 位整数"))?,
            ),
        };
        if seed.is_some() && params.get("save").and_then(Value::as_str).is_some() {
            return Err(ProtoError::new(-32602, "`seed` 不能与 `save` 同时使用"));
        }
        let (program, analysis) = {
            let unit = self.story(&story_id)?;
            (unit.program, unit.analysis)
        };
        let opened = match params.get("save").and_then(Value::as_str) {
            Some(save) => Story::load(program, analysis, save),
            None => match seed {
                Some(seed) => Story::new_with_seed(program, analysis, seed),
                None => Story::new(program, analysis),
            },
        };
        let story = match opened {
            Ok(s) => s,
            Err(e) => return Ok(json!({ "ok": false, "run_error": e })),
        };
        let state = story.state_view();
        self.next_session += 1;
        let session_id = format!("c{}", self.next_session);
        self.sessions
            .insert(session_id.clone(), Session { story_id, story });
        Ok(json!({ "session_id": session_id, "state": state }))
    }

    pub(super) fn trace_replay(&self, params: &Value) -> Result<Value, ProtoError> {
        let story_id = param_str(params, "story_id")?;
        let unit = self.story(story_id)?;
        let trace_value = params
            .get("trace")
            .cloned()
            .ok_or_else(|| ProtoError::new(-32602, "需要 `trace` DTO"))?;
        let trace: ReplayTrace = serde_json::from_value(trace_value)
            .map_err(|error| ProtoError::new(-32602, format!("`trace` DTO 无效:{error}")))?;
        let defaults = ReplayBudget::default();
        let max_steps = optional_u64(params, "max_steps")?.unwrap_or(defaults.max_steps);
        let time_budget_ms =
            optional_u64(params, "time_budget_ms")?.unwrap_or(defaults.time_budget_ms);
        let replay = ReplayTrace::replay(
            unit.program,
            unit.analysis,
            &trace,
            ReplayBudget::new(max_steps, time_budget_ms),
            &ReplayCancellation::new(),
        )
        .map_err(|error| ProtoError::new(-32602, format!("重放 DTO 不兼容:{error}")))?;
        Ok(json!({ "ok": true, "replay": replay }))
    }
}
fn compile_options(params: &Value) -> Result<CompileOptions, ProtoError> {
    let version = params.get("language_version").or_else(|| {
        params
            .get("options")
            .and_then(|value| value.get("language_version"))
    });
    let Some(version) = version else {
        return Ok(CompileOptions::default());
    };
    let version = version
        .as_str()
        .ok_or_else(|| ProtoError::new(-32602, "`language_version` 必须是字符串"))?;
    match version {
        "1.9" => Ok(CompileOptions::new(LanguageVersion::V1_9)),
        "1.10" => Ok(CompileOptions::new(LanguageVersion::V1_10)),
        _ => Err(ProtoError::new(
            -32602,
            format!("不支持的语言版本 `{version}`(可用: 1.9 / 1.10)"),
        )),
    }
}

fn compile_path_input(
    path: &Path,
    params: &Value,
    options: CompileOptions,
) -> Result<CompileInput, std::io::Error> {
    let explicit = params.get("language_version").is_some()
        || params
            .get("options")
            .and_then(|value| value.get("language_version"))
            .is_some();
    if !explicit {
        let root = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().unwrap_or(Path::new(".")).to_path_buf()
        };
        if root.join(".world").join("project.json").is_file() {
            let mut project = Project::open(path).map_err(std::io::Error::other)?;
            return Ok(CompileInput {
                result: project.compile(),
                workspace_diagnostics: project.authoring_diagnostics().to_vec(),
            });
        }
    }
    let result = compile_path_with_options(path, options)?;
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    if root.join(".world").join("project.json").is_file() {
        let project = Project::open(path).map_err(std::io::Error::other)?;
        return Ok(CompileInput {
            result,
            workspace_diagnostics: project.authoring_diagnostics().to_vec(),
        });
    }
    Ok(CompileInput::plain(result))
}
/// 选项列表的机器视图(index 0 起,规范 agent-protocol.md §2.4)。
pub(super) fn choices_json(story: &Story) -> Vec<Value> {
    story
        .choices()
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mut choice = json!(c);
            choice["index"] = json!(i);
            choice
        })
        .collect()
}

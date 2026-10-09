use super::*;
impl Server {
    pub(super) fn compile(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let options = compile_options(params)?;
        let input = if let Some(value) = params.get("project_id") {
            if params.get("path").is_some()
                || params.get("source").is_some()
                || params.get("file_name").is_some()
                || params.get("language_version").is_some()
                || params.get("options").is_some()
            {
                return Err(ProtoError::new(
                    -32602,
                    "project_id 编译不能同时指定其他源码或语言选项",
                ));
            }
            let id = value
                .as_str()
                .ok_or_else(|| ProtoError::new(-32602, "project_id 必须是字符串"))?;
            let unit = self
                .projects
                .get(id)
                .ok_or_else(|| ProtoError::new(-32602, "未知 project_id"))?;
            let mut project = unit.project.clone();
            CompileInput {
                result: project.compile(),
                workspace_diagnostics: project.authoring_diagnostics().to_vec(),
                project: Some(project),
            }
        } else if let Some(p) = params.get("path").and_then(Value::as_str) {
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
            return Err(ProtoError::new(
                -32602,
                "需要 `project_id`、`path` 或 `source` 参数",
            ));
        };
        let CompileInput {
            result,
            workspace_diagnostics,
            project,
        } = input;
        let read_only = !workspace_diagnostics.is_empty();
        if result.has_errors() {
            return Ok(json!({
                "ok": false,
                "timeline": result.analysis.timeline,
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
                project,
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
        let localization = localization_session::requested(params)?;
        let choice_presentation = match params.get("capabilities") {
            None => false,
            Some(value) => {
                let capabilities = value
                    .as_array()
                    .ok_or_else(|| ProtoError::new(-32602, "capabilities 必须是字符串数组"))?;
                if capabilities.iter().any(|v| !v.is_string()) {
                    return Err(ProtoError::new(-32602, "capabilities 必须是字符串数组"));
                }
                capabilities
                    .iter()
                    .any(|v| v.as_str() == Some(worldline_runtime::CHOICE_PRESENTATION_CAPABILITY))
            }
        };
        let bounded_continue = params
            .get("capabilities")
            .and_then(Value::as_array)
            .is_some_and(|values| {
                values.iter().any(|value| {
                    value.as_str() == Some(worldline_runtime::BOUNDED_CONTINUE_CAPABILITY)
                })
            });
        let budget = continuation_budget(
            params,
            bounded_continue,
            worldline_runtime::DEFAULT_CONTINUATION_BUDGET,
        )?;
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
        let (program, analysis, presentation) = {
            let unit = self.story(&story_id)?;
            let presentation = match localization
                .as_ref()
                .map(|request| localization_session::prepare(unit.project.as_ref(), request))
                .transpose()
            {
                Ok(value) => value,
                Err(failure) => return Ok(failure),
            };
            (unit.program, unit.analysis, presentation)
        };
        let opened = match (&presentation, params.get("save").and_then(Value::as_str)) {
            (Some(presentation), Some(save)) => {
                Story::load_with_presentation(program, analysis, save, presentation)
            }
            (None, Some(save)) => Story::load(program, analysis, save),
            (Some(presentation), None) => match seed {
                Some(seed) => Story::new_with_presentation(program, analysis, seed, presentation),
                None => Story::new_localized(program, analysis, presentation),
            },
            (None, None) => match seed {
                Some(seed) => Story::new_with_seed(program, analysis, seed),
                None => Story::new(program, analysis),
            },
        };
        let mut story = match opened {
            Ok(s) => s,
            Err(e) => return Ok(json!({ "ok": false, "run_error": e })),
        };
        story.set_continuation_budget(budget);
        let state = story.state_view();
        let presentation_identity = story.presentation_identity().cloned();
        self.next_session += 1;
        let session_id = format!("c{}", self.next_session);
        self.sessions.insert(
            session_id.clone(),
            Session {
                story_id,
                story,
                choice_presentation,
                bounded_continue,
                cancel_next: false,
            },
        );
        let mut response = json!({ "session_id": session_id, "state": state });
        if let Some(identity) = presentation_identity {
            response["presentation"] = json!(identity);
        }
        if bounded_continue {
            response["execution_diagnostics"] = json!(
                worldline_core::analysis::execution_diagnostics(program, analysis)
            );
        }
        if params.get("capabilities").is_some() {
            let mut capabilities = Vec::new();
            if choice_presentation {
                capabilities.push(worldline_runtime::CHOICE_PRESENTATION_CAPABILITY);
            }
            if bounded_continue {
                capabilities.push(worldline_runtime::BOUNDED_CONTINUE_CAPABILITY);
            }
            if localization.is_some() {
                capabilities.push(worldline_runtime::LOCALIZATION_PRESENTATION_CAPABILITY);
            }
            response["capabilities"] = json!(capabilities);
        }
        Ok(response)
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
        let presentation = match localization_session::for_trace(unit.project.as_ref(), &trace) {
            Ok(value) => value,
            Err(failure) => return Ok(failure),
        };
        let outcome = match &presentation {
            Some(presentation) => ReplayTrace::replay_with_presentation(
                unit.program,
                unit.analysis,
                &trace,
                ReplayBudget::new(max_steps, time_budget_ms),
                &ReplayCancellation::new(),
                presentation,
            ),
            None => ReplayTrace::replay(
                unit.program,
                unit.analysis,
                &trace,
                ReplayBudget::new(max_steps, time_budget_ms),
                &ReplayCancellation::new(),
            ),
        };
        let replay = match outcome {
            Ok(value) => value,
            Err(error) if presentation.is_some() => {
                return Ok(json!({"ok":false,"run_error":error}))
            }
            Err(error) => return Err(ProtoError::new(-32602, format!("重放 DTO 不兼容:{error}"))),
        };
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
    LanguageVersion::from_supported_str(version)
        .map(CompileOptions::new)
        .ok_or_else(|| {
            ProtoError::new(
                -32602,
                format!(
                    "不支持的语言版本 `{version}`(可用: {})",
                    LanguageVersion::SUPPORTED
                        .map(LanguageVersion::as_str)
                        .join(" / ")
                ),
            )
        })
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
                project: Some(project),
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
            project: Some(project),
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

impl Server {
    pub(super) fn session_continue(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let session = self.session(params)?;
        let budget = continuation_budget(
            params,
            session.bounded_continue,
            session.story.continuation_budget(),
        )?;
        let cancellation = ReplayCancellation::new();
        if std::mem::take(&mut session.cancel_next) {
            cancellation.cancel();
        }
        match session.story.continue_story_bounded(budget, &cancellation) {
            Ok(result) => {
                let mut response = session_response(session);
                response["outputs"] = json!(result.outputs);
                if session.bounded_continue {
                    response["outcome"] = json!(result.outcome);
                    response["executed_steps"] = json!(result.executed_steps);
                }
                if result.outcome.is_suspended() {
                    response["ok"] = json!(false);
                    response["run_error"] = json!(session.story.continuation_error(result.outcome));
                }
                Ok(response)
            }
            Err(error) => Ok(json!({ "ok": false, "run_error": error })),
        }
    }
    pub(super) fn session_cancel(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let session = self.session(params)?;
        if !session.bounded_continue {
            return Err(ProtoError::new(
                -32602,
                "需先协商 runtime.bounded_continue.v1",
            ));
        }
        session.cancel_next = true;
        Ok(json!({"cancel_pending": true, "state": session.story.state_view()}))
    }
    pub(super) fn session_choose(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let session = self.session(params)?;
        let count = ["index", "presentation_index", "choice_id"]
            .iter()
            .filter(|key| params.get(**key).is_some())
            .count();
        if count != 1 {
            return Err(ProtoError::new(
                -32602,
                "index、presentation_index、choice_id 必须且只能提供一个",
            ));
        }
        let selected = if let Some(index) = params.get("index") {
            let index = index
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| ProtoError::new(-32602, "需要整数参数 index(0 起)"))?;
            let len = session.story.choices().len();
            if index >= len {
                return Err(ProtoError::new(
                    -32602,
                    format!("选择越界:index {index},共 {len} 项"),
                ));
            }
            session.story.choose(index)
        } else {
            if !session.choice_presentation {
                return Err(ProtoError::new(
                    -32602,
                    "需先在 session.open 协商 runtime.choice_presentation.v1",
                ));
            }
            if let Some(index) = params.get("presentation_index") {
                let index = index
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| ProtoError::new(-32602, "presentation_index 必须为非负整数"))?;
                session.story.choose_presentation(index)
            } else {
                let id = param_str(params, "choice_id")?;
                session.story.choose_id(id)
            }
        };
        match selected {
            Ok(()) => Ok(session_response(session)),
            Err(error) => Ok(json!({ "ok": false, "run_error": error })),
        }
    }
}
fn session_response(session: &Session) -> Value {
    let mut response = json!({
        "choices": choices_json(&session.story),
        "state": session.story.state_view(),
        "paused": session.story.is_paused(),
        "ended": session.story.is_ended(),
    });
    if session.choice_presentation {
        response["choice_presentation"] = json!(session.story.choice_presentations());
    }
    response
}

fn continuation_budget(
    params: &Value,
    negotiated: bool,
    defaults: ReplayBudget,
) -> Result<ReplayBudget, ProtoError> {
    if !negotiated
        && ["max_steps", "time_budget_ms"]
            .iter()
            .any(|key| params.get(key).is_some())
    {
        return Err(ProtoError::new(
            -32602,
            "执行预算参数需先协商 runtime.bounded_continue.v1",
        ));
    }
    Ok(ReplayBudget::new(
        optional_u64(params, "max_steps")?.unwrap_or(defaults.max_steps),
        optional_u64(params, "time_budget_ms")?.unwrap_or(defaults.time_budget_ms),
    ))
}

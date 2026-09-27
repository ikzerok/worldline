use super::projects::project_failure_with_workspace;
use super::relation_drafts::relation_target_value;
use super::*;
impl Server {
    pub(super) fn relation_query(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let target = relation_target(params)?;
        let (options, include_period_children) = relation_query_options(params)?;
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            let conflicts = match unit.project.refresh() {
                Ok(conflicts) => conflicts,
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
            let result = unit.project.compile();
            if result.has_errors() {
                return Ok(relation_compile_failure(
                    &result,
                    unit.project.authoring_diagnostics(),
                    Some(unit.project.content_baseline()),
                ));
            }
            let baseline = unit.project.content_baseline();
            let mut project_options = options.clone();
            project_options.scope_refs = worldline_core::relations::expand_period_scope_refs(
                &result.analysis.timeline,
                &project_options.scope_refs,
                include_period_children,
            );
            let mut response = relation_query_value(
                &result.analysis,
                &result.diagnostics,
                unit.project.authoring_diagnostics(),
                result.options.language_version,
                Some(&baseline),
                &target,
                project_options,
            )?;
            if !conflicts.is_empty() {
                response["conflicts"] = json!(conflicts);
            }
            return Ok(response);
        }
        let story_id = param_str(params, "story_id")?;
        let unit = self.story(story_id)?;
        let mut story_options = options;
        story_options.scope_refs = worldline_core::relations::expand_period_scope_refs(
            &unit.analysis.timeline,
            &story_options.scope_refs,
            include_period_children,
        );
        relation_query_value(
            unit.analysis,
            &[],
            &[],
            unit.language_version,
            None,
            &target,
            story_options,
        )
    }
    pub(super) fn relation_project(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let target = relation_target(params)?;
        let options = topic_projection_options(params)?;
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            let conflicts = match unit.project.refresh() {
                Ok(conflicts) => conflicts,
                Err(error) => {
                    let result = unit.project.compile();
                    let mut response = project_failure_with_workspace(
                        "IO_ERROR",
                        format!("刷新工程失败：{error}"),
                        Some(&result.diagnostics),
                        Some(unit.project.content_baseline()),
                        Some(result.options.language_version.as_str()),
                        unit.project.authoring_diagnostics(),
                    );
                    response["target"] = json!(target);
                    response["relations"] = Value::Null;
                    response["history"] = Value::Null;
                    response["truncated"] = json!(false);
                    return Ok(response);
                }
            };
            let result = unit.project.compile();
            if result.has_errors() {
                let mut response = relation_compile_failure(
                    &result,
                    unit.project.authoring_diagnostics(),
                    Some(unit.project.content_baseline()),
                );
                response["target"] = json!(target);
                response["relations"] = Value::Null;
                response["history"] = Value::Null;
                return Ok(response);
            }
            let baseline = unit.project.content_baseline();
            let mut response = topic_projection_value(
                &result.analysis,
                &result.diagnostics,
                unit.project.authoring_diagnostics(),
                result.options.language_version,
                Some(&baseline),
                &target,
                options,
            )?;
            if !conflicts.is_empty() {
                response["conflicts"] = json!(conflicts);
            }
            return Ok(response);
        }
        let story_id = param_str(params, "story_id")?;
        let unit = self.story(story_id)?;
        topic_projection_value(
            unit.analysis,
            &[],
            &[],
            unit.language_version,
            None,
            &target,
            options,
        )
    }
}
fn relation_target(params: &Value) -> Result<TargetRef, ProtoError> {
    let value = params
        .get("target")
        .ok_or_else(|| ProtoError::new(-32602, "关系查询需要 `target`"))?;
    relation_target_value(value, "target")
}

fn relation_query_options(params: &Value) -> Result<(RelationQueryOptions, bool), ProtoError> {
    let offset = match params.get("offset") {
        None => 0,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| ProtoError::new(-32602, "`offset` 必须是非负整数"))?
            .try_into()
            .map_err(|_| ProtoError::new(-32602, "`offset` 超出平台整数范围"))?,
    };
    let depth = match params.get("depth") {
        None => 1,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| ProtoError::new(-32602, "`depth` 必须是整数 1 或 2"))?
            .try_into()
            .map_err(|_| ProtoError::new(-32602, "`depth` 必须是整数 1 或 2"))?,
    };
    if !matches!(depth, 1 | 2) {
        return Err(ProtoError::new(-32602, "`depth` 只能是 1 或 2"));
    }
    let direction = match params.get("direction").and_then(Value::as_str) {
        None | Some("both") => RelationQueryDirection::Both,
        Some("outgoing") => RelationQueryDirection::Outgoing,
        Some("incoming") => RelationQueryDirection::Incoming,
        Some(value) => {
            return Err(ProtoError::new(
                -32602,
                format!("未知关系方向 `{value}`(可用: outgoing / incoming / both)"),
            ))
        }
    };
    let relation_type = params
        .get("relation_type")
        .or_else(|| params.get("type"))
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| ProtoError::new(-32602, "`relation_type` 必须是字符串"))
        })
        .transpose()?;
    let scope_refs = params
        .get("scope_refs")
        .or_else(|| params.get("scopes"))
        .map(|value| {
            value
                .as_array()
                .ok_or_else(|| ProtoError::new(-32602, "`scope_refs` 必须是数组"))?
                .iter()
                .enumerate()
                .map(|(index, value)| relation_target_value(value, &format!("scope_refs[{index}]")))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let include_unscoped = params
        .get("include_unscoped")
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| ProtoError::new(-32602, "`include_unscoped` 必须是布尔值"))
        })
        .transpose()?
        .unwrap_or(false);
    let include_period_children = params
        .get("include_period_children")
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| ProtoError::new(-32602, "`include_period_children` 必须是布尔值"))
        })
        .transpose()?
        .unwrap_or(false);
    Ok((
        RelationQueryOptions {
            offset,
            depth,
            relation_type,
            scope_refs,
            include_unscoped,
            direction,
            ..RelationQueryOptions::default()
        },
        include_period_children,
    ))
}

fn topic_projection_options(
    params: &Value,
) -> Result<worldline_core::TopicProjectionOptions, ProtoError> {
    if params.get("relation_type").is_some() || params.get("type").is_some() {
        return Err(ProtoError::new(
            -32602,
            "`relation.project` 使用 `role_mapping`，不接受 `relation_type`",
        ));
    }
    let (relation, include_period_children) = relation_query_options(params)?;
    let role_mapping = match params.get("role_mapping") {
        None => std::collections::BTreeMap::new(),
        Some(Value::Object(values)) => values
            .iter()
            .map(|(relation_type, role)| {
                role.as_str()
                    .map(|role| (relation_type.clone(), role.to_string()))
                    .ok_or_else(|| ProtoError::new(-32602, "`role_mapping` 的值必须是字符串"))
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(ProtoError::new(-32602, "`role_mapping` 必须是 JSON 对象")),
    };
    let history_offset = params
        .get("history_offset")
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| ProtoError::new(-32602, "`history_offset` 必须是非负整数"))?
                .try_into()
                .map_err(|_| ProtoError::new(-32602, "`history_offset` 超出平台整数范围"))
        })
        .transpose()?
        .unwrap_or_default();
    let max_nodes = params
        .get("max_nodes")
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| ProtoError::new(-32602, "`max_nodes` 必须是非负整数"))?
                .try_into()
                .map_err(|_| ProtoError::new(-32602, "`max_nodes` 超出平台整数范围"))
        })
        .transpose()?
        .unwrap_or(250);
    let max_edges = params
        .get("max_edges")
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| ProtoError::new(-32602, "`max_edges` 必须是非负整数"))?
                .try_into()
                .map_err(|_| ProtoError::new(-32602, "`max_edges` 超出平台整数范围"))
        })
        .transpose()?
        .unwrap_or(500);
    Ok(worldline_core::TopicProjectionOptions {
        role_mapping,
        offset: relation.offset,
        history_offset,
        depth: relation.depth,
        direction: relation.direction,
        scope_refs: relation.scope_refs,
        include_unscoped: relation.include_unscoped,
        include_period_children,
        max_nodes,
        max_edges,
    })
}

fn relation_compile_failure(
    result: &CompileResult,
    workspace_diagnostics: &[Diagnostic],
    baseline: Option<String>,
) -> Value {
    json!({
        "ok": false,
        "schema_version": 1,
        "diagnostics": result.diagnostics,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
        "language_version": result.options.language_version.as_str(),
        "workspace_revision": baseline,
        "truncated": false,
        "continuation": Value::Null,
    })
}

fn relation_query_value(
    analysis: &Analysis,
    diagnostics: &[Diagnostic],
    workspace_diagnostics: &[Diagnostic],
    language_version: LanguageVersion,
    baseline: Option<&str>,
    target: &TargetRef,
    options: RelationQueryOptions,
) -> Result<Value, ProtoError> {
    if analysis.catalog.object(target).is_none() {
        return Err(ProtoError::new(
            -32602,
            format!("关系查询目标不存在 {}:{}", target.kind, target.id),
        ));
    }
    if let Some(relation_type) = options.relation_type.as_deref() {
        if !analysis.catalog.relation_types.contains_key(relation_type) {
            return Err(ProtoError::new(
                -32602,
                format!("未知关系类型 `{relation_type}`"),
            ));
        }
    }
    for scope in &options.scope_refs {
        if analysis.catalog.object(scope).is_none() {
            return Err(ProtoError::new(
                -32602,
                format!("关系查询范围对象不存在 {}:{}", scope.kind, scope.id),
            ));
        }
    }
    let query = analysis.catalog.query_relations(target, options);
    let mut payload = serde_json::to_value(query)
        .expect("关系查询结果可序列化")
        .as_object()
        .cloned()
        .expect("关系查询结果必须是对象");
    payload.insert("ok".into(), json!(true));
    payload.insert("language_version".into(), json!(language_version.as_str()));
    payload.insert(
        "workspace_revision".into(),
        baseline.map_or(Value::Null, |value| json!(value)),
    );
    payload.insert("diagnostics".into(), json!(diagnostics));
    payload.insert("workspace_diagnostics".into(), json!(workspace_diagnostics));
    payload.insert("read_only".into(), json!(!workspace_diagnostics.is_empty()));
    Ok(Value::Object(payload))
}
fn topic_projection_value(
    analysis: &Analysis,
    diagnostics: &[Diagnostic],
    workspace_diagnostics: &[Diagnostic],
    language_version: LanguageVersion,
    baseline: Option<&str>,
    target: &TargetRef,
    options: worldline_core::TopicProjectionOptions,
) -> Result<Value, ProtoError> {
    let projection = analysis
        .query_topic_projection(target, options)
        .map_err(|error| ProtoError::new(-32602, error.to_string()))?;
    let mut payload = serde_json::to_value(projection)
        .expect("专题投影结果可序列化")
        .as_object()
        .cloned()
        .expect("专题投影结果必须是对象");
    payload.insert("ok".into(), json!(true));
    payload.insert("language_version".into(), json!(language_version.as_str()));
    payload.insert(
        "workspace_revision".into(),
        baseline.map_or(Value::Null, |value| json!(value)),
    );
    payload.insert("diagnostics".into(), json!(diagnostics));
    payload.insert("workspace_diagnostics".into(), json!(workspace_diagnostics));
    payload.insert("read_only".into(), json!(!workspace_diagnostics.is_empty()));
    Ok(Value::Object(payload))
}

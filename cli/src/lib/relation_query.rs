use super::support::*;
use super::*;

pub(super) fn cmd_relations(args: &RelationsArgs, out: &mut impl Write) -> Result<i32, String> {
    let snapshot = match open_workspace_snapshot(&args.path) {
        Ok(snapshot) => snapshot,
        Err(error) => return write_query_failure(&args.path, args.json, &error, out),
    };
    if snapshot.result.has_errors() {
        if args.json {
            let mut payload = query_payload_base(&snapshot);
            payload.insert("ok".into(), json!(false));
            payload.insert("target".into(), json!(&args.target));
            payload.insert("depth".into(), json!(args.depth));
            payload.insert("nodes".into(), json!([]));
            payload.insert("edges".into(), json!([]));
            writeln!(out, "{}", Value::Object(payload)).map_err(|e| e.to_string())?;
        } else {
            print_errors_hint(&snapshot.result.diagnostics, out);
        }
        return Ok(1);
    }
    if snapshot
        .result
        .analysis
        .catalog
        .object(&args.target)
        .is_none()
    {
        return write_query_error(
            &args.path,
            args.json,
            "UNKNOWN_TARGET",
            &format!("目标对象不存在 {}:{}", args.target.kind, args.target.id),
            out,
            2,
        );
    }
    if let Some(relation_type) = args.relation_type.as_deref() {
        if !snapshot
            .result
            .analysis
            .catalog
            .relation_types
            .contains_key(relation_type)
        {
            return write_query_error(
                &args.path,
                args.json,
                "UNKNOWN_RELATION_TYPE",
                &format!("关系类型 `{relation_type}` 不存在"),
                out,
                2,
            );
        }
    }
    for scope in &args.scope_refs {
        if snapshot.result.analysis.catalog.object(scope).is_none() {
            return write_query_error(
                &args.path,
                args.json,
                "UNKNOWN_SCOPE",
                &format!("范围对象不存在 {}:{}", scope.kind, scope.id),
                out,
                2,
            );
        }
    }
    let scope_refs = worldline_core::relations::expand_period_scope_refs(
        &snapshot.result.analysis.timeline,
        &args.scope_refs,
        args.include_period_children,
    );
    let query = snapshot.result.analysis.catalog.query_relations(
        &args.target,
        RelationQueryOptions {
            offset: args.offset,
            depth: args.depth,
            relation_type: args.relation_type.clone(),
            scope_refs,
            include_unscoped: args.include_unscoped,
            direction: args.direction,
            ..RelationQueryOptions::default()
        },
    );
    if args.json {
        let mut payload = query_payload_base(&snapshot);
        payload.insert("ok".into(), json!(true));
        let query = serde_json::to_value(query).expect("关系查询结果可序列化");
        if let Value::Object(fields) = query {
            for (key, value) in fields {
                payload.insert(key, value);
            }
        }
        writeln!(out, "{}", Value::Object(payload)).map_err(|e| e.to_string())?;
    } else {
        writeln!(
            out,
            "{}:{}: {} 条节点 / {} 条关系",
            args.target.kind,
            args.target.id,
            query.nodes.len(),
            query.edges.len()
        )
        .map_err(|e| e.to_string())?;
        for edge in query.edges {
            writeln!(
                out,
                "{}  {} -> {}  {}",
                edge.id, edge.from_ref.id, edge.to_ref.id, edge.label
            )
            .map_err(|e| e.to_string())?;
        }
        if query.truncated {
            writeln!(out, "结果已截断，请使用 continuation 继续查询").map_err(|e| e.to_string())?;
        }
    }
    Ok(0)
}

pub(super) fn cmd_topic_projection(
    args: &TopicProjectionArgs,
    out: &mut impl Write,
) -> Result<i32, String> {
    let snapshot = match open_workspace_snapshot(&args.path) {
        Ok(snapshot) => snapshot,
        Err(error) => return write_query_failure(&args.path, args.json, &error, out),
    };
    if snapshot.result.has_errors() {
        if args.json {
            let mut payload = query_payload_base(&snapshot);
            payload.insert("ok".into(), json!(false));
            payload.insert("target".into(), json!(&args.target));
            payload.insert("relations".into(), Value::Null);
            payload.insert("history".into(), Value::Null);
            writeln!(out, "{}", Value::Object(payload)).map_err(|e| e.to_string())?;
        } else {
            print_errors_hint(&snapshot.result.diagnostics, out);
        }
        return Ok(1);
    }
    let projection = match snapshot.result.analysis.query_topic_projection(
        &args.target,
        worldline_core::TopicProjectionOptions {
            role_mapping: args.role_mapping.clone(),
            offset: args.offset,
            history_offset: args.history_offset,
            depth: args.depth,
            direction: args.direction,
            scope_refs: args.scope_refs.clone(),
            include_unscoped: args.include_unscoped,
            include_period_children: args.include_period_children,
            max_nodes: args.max_nodes,
            max_edges: args.max_edges,
        },
    ) {
        Ok(projection) => projection,
        Err(error) => {
            return write_query_error(
                &args.path,
                args.json,
                error.code(),
                &error.to_string(),
                out,
                2,
            )
        }
    };
    if args.json {
        let mut payload = query_payload_base(&snapshot);
        payload.insert("ok".into(), json!(true));
        if let Value::Object(fields) =
            serde_json::to_value(&projection).expect("专题投影结果可序列化")
        {
            payload.extend(fields);
        }
        writeln!(out, "{}", Value::Object(payload)).map_err(|e| e.to_string())?;
    } else {
        writeln!(
            out,
            "{}:{}: {} 条关系边 / {} 条历史关联",
            args.target.kind,
            args.target.id,
            projection.relations.edges.len(),
            projection.history.items.len()
        )
        .map_err(|e| e.to_string())?;
        for edge in &projection.relations.edges {
            writeln!(
                out,
                "{} [{}]  {} -> {}  {}",
                edge.id, edge.role, edge.from_ref.id, edge.to_ref.id, edge.label
            )
            .map_err(|e| e.to_string())?;
        }
        for item in &projection.history.items {
            writeln!(out, "历史: event:{} ({})", item.event.id, item.file)
                .map_err(|e| e.to_string())?;
        }
        if projection.truncated {
            writeln!(
                out,
                "投影结果已截断，请使用 relations/history continuation 继续查询"
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(0)
}

pub(super) fn relation_failure(
    json_mode: bool,
    out: &mut impl Write,
    code: &str,
    message: String,
    result: Option<&CompileResult>,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
) -> Result<i32, String> {
    if json_mode {
        let payload = json!({
            "ok": false,
            "error": {"code": code, "message": message},
            "diagnostics": result.map_or_else(Vec::new, |value| value.diagnostics.clone()),
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": !workspace_diagnostics.is_empty(),
            "language_version": result.map(|value| value.options.language_version.as_str()),
            "baseline": baseline,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "{code}: {message}").map_err(|e| e.to_string())?;
    }
    Ok(1)
}

pub(super) fn relation_project_state(
    path: &Path,
    json_mode: bool,
    out: &mut impl Write,
) -> Result<(Project, CompileResult, String, Vec<Diagnostic>), i32> {
    let mut project = match Project::open(path) {
        Ok(project) => project,
        Err(error) => {
            let _ = relation_failure(json_mode, out, "IO_ERROR", error, None, String::new(), &[]);
            return Err(2);
        }
    };
    let conflicts = match project.refresh() {
        Ok(conflicts) => conflicts,
        Err(error) => {
            let result = project.compile();
            let baseline = project.content_baseline();
            let diagnostics = project.authoring_diagnostics().to_vec();
            let _ = relation_failure(
                json_mode,
                out,
                "IO_ERROR",
                format!("刷新工程失败：{error}"),
                Some(&result),
                baseline,
                &diagnostics,
            );
            return Err(2);
        }
    };
    let result = project.compile();
    let baseline = project.content_baseline();
    let diagnostics = project.authoring_diagnostics().to_vec();
    if !conflicts.is_empty() {
        let _ = relation_failure(
            json_mode,
            out,
            "CONFLICT",
            format!(
                "工程存在外部修改冲突，拒绝覆盖：{}",
                conflicts
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            ),
            Some(&result),
            baseline,
            &diagnostics,
        );
        return Err(1);
    }
    Ok((project, result, baseline, diagnostics))
}

pub(super) fn relation_common_checks(
    json_mode: bool,
    out: &mut impl Write,
    args_baseline: Option<&str>,
    project: &Project,
    result: &CompileResult,
    baseline: &str,
    workspace_diagnostics: &[Diagnostic],
) -> Result<(), i32> {
    if args_baseline.is_some_and(|expected| expected != baseline) {
        let _ = relation_failure(
            json_mode,
            out,
            "STALE_BASELINE",
            format!("工程基线已变化，拒绝覆盖；当前基线为 {baseline}"),
            Some(result),
            baseline.to_string(),
            workspace_diagnostics,
        );
        return Err(1);
    }
    if result.has_errors() {
        let _ = relation_failure(
            json_mode,
            out,
            "COMPILE_FAILED",
            "当前工程存在错误诊断，关系编辑未提交".into(),
            Some(result),
            baseline.to_string(),
            workspace_diagnostics,
        );
        return Err(1);
    }
    if !workspace_diagnostics.is_empty() {
        let _ = relation_failure(
            json_mode,
            out,
            "READ_ONLY",
            "工程清单或展示文档包含当前工具不支持的格式，只能只读查看".into(),
            Some(result),
            baseline.to_string(),
            workspace_diagnostics,
        );
        return Err(1);
    }
    if project.language_version_kind() != LanguageVersion::V1_10 {
        let _ = relation_failure(
            json_mode,
            out,
            "LANGUAGE_VERSION_REQUIRED",
            "关系编辑要求工程清单明确选择语言版本 1.10".into(),
            Some(result),
            baseline.to_string(),
            workspace_diagnostics,
        );
        return Err(1);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn relation_result(
    operation: &str,
    relation: Value,
    catalog: &worldline_core::catalog::Catalog,
    result: &CompileResult,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
    json_mode: bool,
    out: &mut impl Write,
) -> Result<i32, String> {
    let payload = json!({
        "ok": true,
        "operation": operation,
        "relation": relation,
        "catalog": catalog,
        "diagnostics": result.diagnostics,
        "language_version": result.options.language_version.as_str(),
        "baseline": baseline,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    });
    if json_mode {
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "关系 {operation} 成功").map_err(|e| e.to_string())?;
    }
    Ok(0)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn relation_type_result(
    operation: &str,
    relation_type: Value,
    catalog: &worldline_core::catalog::Catalog,
    result: &CompileResult,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
    json_mode: bool,
    out: &mut impl Write,
) -> Result<i32, String> {
    let payload = json!({
        "ok": true,
        "operation": operation,
        "relation_type": relation_type,
        "catalog": catalog,
        "diagnostics": result.diagnostics,
        "language_version": result.options.language_version.as_str(),
        "baseline": baseline,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    });
    if json_mode {
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "关系类型 {operation} 成功").map_err(|e| e.to_string())?;
    }
    Ok(0)
}

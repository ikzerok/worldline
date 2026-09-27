use super::relation_query::*;
use super::*;

pub(super) fn cmd_relation_type_edit(
    args: &RelationTypeEditArgs,
    out: &mut impl Write,
) -> Result<i32, String> {
    let (mut project, before, baseline, workspace_diagnostics) =
        match relation_project_state(&args.path, args.json, out) {
            Ok(state) => state,
            Err(code) => return Ok(code),
        };
    if let Err(code) = relation_common_checks(
        args.json,
        out,
        args.baseline.as_deref(),
        &project,
        &before,
        &baseline,
        &workspace_diagnostics,
    ) {
        return Ok(code);
    }
    let existing = before
        .analysis
        .catalog
        .relation_types
        .get(&args.id)
        .cloned();
    match args.operation {
        RelationEditOperation::Create if existing.is_some() => {
            return relation_failure(
                args.json,
                out,
                "RELATION_TYPE_EXISTS",
                format!("关系类型 `{}` 已存在", args.id),
                Some(&before),
                baseline,
                &workspace_diagnostics,
            )
        }
        RelationEditOperation::Update | RelationEditOperation::Delete if existing.is_none() => {
            return relation_failure(
                args.json,
                out,
                "RELATION_TYPE_NOT_FOUND",
                format!("关系类型 `{}` 不存在", args.id),
                Some(&before),
                baseline,
                &workspace_diagnostics,
            )
        }
        _ => {}
    }
    if args.operation == RelationEditOperation::Delete {
        if let Err(error) = project.remove_relation_type(&args.id) {
            return relation_failure(
                args.json,
                out,
                "EDIT_FAILED",
                error,
                Some(&before),
                baseline,
                &workspace_diagnostics,
            );
        }
        if let Err(error) = project.save() {
            return relation_failure(
                args.json,
                out,
                "CONFLICT",
                error,
                Some(&before),
                baseline,
                &workspace_diagnostics,
            );
        }
        let after = project.compile();
        return relation_type_result(
            "delete",
            Value::Null,
            &after.analysis.catalog,
            &after,
            project.content_baseline(),
            &workspace_diagnostics,
            args.json,
            out,
        );
    }
    let draft = if let Some(existing) = existing {
        RelationTypeDraft {
            id: args.id.clone(),
            display: args.display.clone().unwrap_or(existing.display),
            inverse_display: if args.clear_inverse_display {
                None
            } else {
                args.inverse_display.clone().or(existing.inverse_display)
            },
            direction: args.direction.unwrap_or(existing.direction),
            from_kind: if args.clear_from_kind {
                None
            } else {
                args.from_kind.clone().or(existing.from_kind)
            },
            to_kind: if args.clear_to_kind {
                None
            } else {
                args.to_kind.clone().or(existing.to_kind)
            },
        }
    } else {
        let Some(display) = args.display.clone() else {
            return relation_failure(
                args.json,
                out,
                "INVALID_ARGUMENT",
                "创建关系类型需要 --display".into(),
                Some(&before),
                baseline,
                &workspace_diagnostics,
            );
        };
        RelationTypeDraft {
            id: args.id.clone(),
            display,
            inverse_display: args.inverse_display.clone(),
            direction: args.direction.unwrap_or_default(),
            from_kind: args.from_kind.clone(),
            to_kind: args.to_kind.clone(),
        }
    };
    let original = (args.operation == RelationEditOperation::Update).then_some(args.id.as_str());
    let edit = project.write_relation_type(original, &draft);
    if let Err(error) = edit {
        return relation_failure(
            args.json,
            out,
            "EDIT_FAILED",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if let Err(error) = project.save() {
        return relation_failure(
            args.json,
            out,
            "CONFLICT",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    let after = project.compile();
    relation_type_result(
        match args.operation {
            RelationEditOperation::Create => "create",
            RelationEditOperation::Update => "update",
            RelationEditOperation::Delete => "delete",
        },
        serde_json::to_value(after.analysis.catalog.relation_types.get(&args.id))
            .expect("RelationTypeInfo 可序列化"),
        &after.analysis.catalog,
        &after,
        project.content_baseline(),
        &workspace_diagnostics,
        args.json,
        out,
    )
}

pub(super) fn cmd_relation_edit(
    args: &RelationEditArgs,
    out: &mut impl Write,
) -> Result<i32, String> {
    let (mut project, before, baseline, workspace_diagnostics) =
        match relation_project_state(&args.path, args.json, out) {
            Ok(state) => state,
            Err(code) => return Ok(code),
        };
    if let Err(code) = relation_common_checks(
        args.json,
        out,
        args.baseline.as_deref(),
        &project,
        &before,
        &baseline,
        &workspace_diagnostics,
    ) {
        return Ok(code);
    }
    let existing = before.analysis.catalog.relations.get(&args.id).cloned();
    match args.operation {
        RelationEditOperation::Create if existing.is_some() => {
            return relation_failure(
                args.json,
                out,
                "RELATION_EXISTS",
                format!("关系 `{}` 已存在", args.id),
                Some(&before),
                baseline,
                &workspace_diagnostics,
            )
        }
        RelationEditOperation::Update | RelationEditOperation::Delete if existing.is_none() => {
            return relation_failure(
                args.json,
                out,
                "RELATION_NOT_FOUND",
                format!("关系 `{}` 不存在", args.id),
                Some(&before),
                baseline,
                &workspace_diagnostics,
            )
        }
        _ => {}
    }
    if args.operation == RelationEditOperation::Delete {
        if let Err(error) = project.remove_relation(&args.id) {
            return relation_failure(
                args.json,
                out,
                "EDIT_FAILED",
                error,
                Some(&before),
                baseline,
                &workspace_diagnostics,
            );
        }
        if let Err(error) = project.save() {
            return relation_failure(
                args.json,
                out,
                "CONFLICT",
                error,
                Some(&before),
                baseline,
                &workspace_diagnostics,
            );
        }
        let after = project.compile();
        return relation_result(
            "delete",
            Value::Null,
            &after.analysis.catalog,
            &after,
            project.content_baseline(),
            &workspace_diagnostics,
            args.json,
            out,
        );
    }
    let draft = if let Some(existing) = existing {
        RelationDraft {
            id: args.id.clone(),
            relation_type: args.relation_type.clone().unwrap_or(existing.relation_type),
            from: args.from.clone().unwrap_or(existing.from_ref),
            to: args.to.clone().unwrap_or(existing.to_ref),
            description: args.description.clone().unwrap_or(existing.description),
            source_note: if args.clear_source_note {
                None
            } else {
                args.source_note.clone().or(existing.source_note)
            },
            scope_refs: if args.clear_scope_refs {
                Vec::new()
            } else if args.scope_refs.is_empty() {
                existing.scope_refs
            } else {
                args.scope_refs.clone()
            },
            properties: if args.clear_properties {
                Vec::new()
            } else if args.properties.is_empty() {
                existing.properties.into_iter().collect()
            } else {
                args.properties.clone()
            },
        }
    } else {
        RelationDraft {
            id: args.id.clone(),
            relation_type: args.relation_type.clone().ok_or("创建关系需要 --type")?,
            from: args.from.clone().ok_or("创建关系需要 --from KIND:ID")?,
            to: args.to.clone().ok_or("创建关系需要 --to KIND:ID")?,
            description: args.description.clone().unwrap_or_default(),
            source_note: args.source_note.clone(),
            scope_refs: args.scope_refs.clone(),
            properties: args.properties.clone(),
        }
    };
    let original = (args.operation == RelationEditOperation::Update).then_some(args.id.as_str());
    let edit = project.write_relation(original, &draft);
    if let Err(error) = edit {
        return relation_failure(
            args.json,
            out,
            "EDIT_FAILED",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if let Err(error) = project.save() {
        return relation_failure(
            args.json,
            out,
            "CONFLICT",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    let after = project.compile();
    relation_result(
        match args.operation {
            RelationEditOperation::Create => "create",
            RelationEditOperation::Update => "update",
            RelationEditOperation::Delete => "delete",
        },
        serde_json::to_value(after.analysis.catalog.relations.get(&args.id))
            .expect("SemanticRelationInfo 可序列化"),
        &after.analysis.catalog,
        &after,
        project.content_baseline(),
        &workspace_diagnostics,
        args.json,
        out,
    )
}

pub(super) fn cmd_promotion(args: &PromotionArgs, out: &mut impl Write) -> Result<i32, String> {
    let (mut project, before, baseline, workspace_diagnostics) =
        match relation_project_state(&args.path, args.json, out) {
            Ok(state) => state,
            Err(code) => return Ok(code),
        };
    if let Err(code) = relation_common_checks(
        args.json,
        out,
        args.baseline.as_deref(),
        &project,
        &before,
        &baseline,
        &workspace_diagnostics,
    ) {
        return Ok(code);
    }
    let Some(handle) = before
        .analysis
        .catalog
        .legacy_relation_handles()
        .into_iter()
        .find(|handle| {
            handle.source == args.source
                && handle.target == args.target
                && handle.label == args.label
                && handle.occurrence == args.occurrence
        })
    else {
        return relation_failure(
            args.json,
            out,
            "LEGACY_RELATION_NOT_FOUND",
            "指定的旧人物关系句柄不存在".into(),
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    };
    let draft = RelationDraft {
        id: args.relation_id.clone(),
        relation_type: args.relation_type.clone(),
        from: handle.source.clone(),
        to: handle.target.clone(),
        description: args
            .description
            .clone()
            .unwrap_or_else(|| handle.label.clone()),
        source_note: args.source_note.clone(),
        scope_refs: args.scope_refs.clone(),
        properties: args.properties.clone(),
    };
    let preview = match project.preview_promote_legacy_relation(&handle, &draft) {
        Ok(preview) => preview,
        Err(error) => {
            return relation_failure(
                args.json,
                out,
                "EDIT_FAILED",
                error,
                Some(&before),
                baseline,
                &workspace_diagnostics,
            )
        }
    };
    if args.operation == PromotionOperation::Preview {
        let payload = json!({
            "ok": true,
            "operation": "preview",
            "preview": preview,
            "catalog": &before.analysis.catalog,
            "diagnostics": before.diagnostics,
            "language_version": before.options.language_version.as_str(),
            "baseline": baseline,
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": false,
        });
        if args.json {
            writeln!(out, "{payload}").map_err(|e| e.to_string())?;
        } else {
            writeln!(out, "关系提升预览成功").map_err(|e| e.to_string())?;
        }
        return Ok(0);
    }
    if let Err(error) = project.apply_legacy_relation_promotion(&preview) {
        return relation_failure(
            args.json,
            out,
            "EDIT_FAILED",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if let Err(error) = project.save() {
        return relation_failure(
            args.json,
            out,
            "CONFLICT",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    let after = project.compile();
    let payload = json!({
        "ok": true,
        "operation": "commit",
        "preview": preview,
        "catalog": &after.analysis.catalog,
        "diagnostics": after.diagnostics,
        "language_version": after.options.language_version.as_str(),
        "baseline": project.content_baseline(),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    });
    if args.json {
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "关系提升提交成功").map_err(|e| e.to_string())?;
    }
    Ok(0)
}

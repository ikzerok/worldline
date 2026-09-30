use super::*;

pub(super) fn cmd_entity(args: &EntityArgs, out: &mut impl Write) -> Result<i32, String> {
    let mut project = Project::open(&args.path)?;
    let before = project.compile();
    let baseline = project.content_baseline();
    let workspace_diagnostics = project.authoring_diagnostics().to_vec();
    if let Some(expected) = &args.baseline {
        if expected != &baseline {
            return entity_failure(
                args,
                out,
                "STALE_BASELINE",
                format!("工程基线已变化，拒绝覆盖；当前基线为 {baseline}"),
                Some(&before),
                baseline,
                &workspace_diagnostics,
            );
        }
    }
    if before.has_errors() {
        return entity_failure(
            args,
            out,
            "COMPILE_FAILED",
            "当前工程存在错误诊断，实体编辑未提交".into(),
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if !project.authoring_diagnostics().is_empty() {
        return entity_failure(
            args,
            out,
            "READ_ONLY",
            "工程清单包含当前工具不支持的格式或必需能力，只能只读查看".into(),
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if !project.language_version_kind().supports_entities() {
        return entity_failure(
            args,
            out,
            "LANGUAGE_VERSION_REQUIRED",
            "实体编辑要求工程清单明确选择语言版本 1.10 或 1.11".into(),
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    let entry = canonical_entry(&args.path)?;
    let id = args.id.as_deref().ok_or("entity 操作需要 --id")?;
    let existing = before.analysis.catalog.entities.get(id).cloned();
    let original = match args.operation {
        EntityOperation::Create => None,
        EntityOperation::Update | EntityOperation::Delete => Some(id),
    };
    if matches!(args.operation, EntityOperation::Create) && existing.is_some() {
        return entity_failure(
            args,
            out,
            "ENTITY_EXISTS",
            format!("实体 `{id}` 已存在"),
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if matches!(
        args.operation,
        EntityOperation::Update | EntityOperation::Delete
    ) && existing.is_none()
    {
        return entity_failure(
            args,
            out,
            "ENTITY_NOT_FOUND",
            format!("实体 `{id}` 不存在"),
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    let draft = if args.operation == EntityOperation::Delete {
        None
    } else {
        Some(entity_draft_from_args(args, existing.as_ref())?)
    };
    let operation = match args.operation {
        EntityOperation::Create => "create",
        EntityOperation::Update => "update",
        EntityOperation::Delete => "delete",
    };
    let edit = project.edit(|candidate| match args.operation {
        EntityOperation::Create => candidate.write_entity(&entry, None, draft.as_ref().unwrap()),
        EntityOperation::Update => {
            candidate.write_entity(&entry, original, draft.as_ref().unwrap())
        }
        EntityOperation::Delete => candidate.remove_entity(id),
    });
    if let Err(error) = edit {
        return entity_failure(
            args,
            out,
            "EDIT_FAILED",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if let Err(error) = project.save() {
        return entity_failure(
            args,
            out,
            "CONFLICT",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    let after = project.compile();
    let after_baseline = project.content_baseline();
    let entity = if id.is_empty() {
        None
    } else {
        after.analysis.catalog.entities.get(id)
    };
    let entity_value =
        entity.map(|value| serde_json::to_value(value).expect("EntityInfo 可序列化"));
    let payload = json!({
        "ok": true,
        "operation": operation,
        "entity": entity_value,
        "catalog": &after.analysis.catalog,
        "language_version": after.options.language_version.as_str(),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
        "baseline": after_baseline,
    });
    if args.json {
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "实体 {operation} 成功: {id}").map_err(|e| e.to_string())?;
        writeln!(out, "基线: {after_baseline}").map_err(|e| e.to_string())?;
    }
    Ok(0)
}

pub(super) fn entity_draft_from_args(
    args: &EntityArgs,
    existing: Option<&worldline_core::catalog::EntityInfo>,
) -> Result<EntityDraft, String> {
    let id = args.id.clone().ok_or("entity 操作需要 --id")?;
    let entity_type = args
        .entity_type
        .clone()
        .or_else(|| existing.map(|entity| entity.entity_type.clone()))
        .ok_or("创建实体需要 --kind")?;
    let display = args
        .display
        .clone()
        .or_else(|| existing.map(|entity| entity.display.clone()))
        .unwrap_or_else(|| id.clone());
    let description = args
        .description
        .clone()
        .or_else(|| existing.map(|entity| entity.description.clone()))
        .unwrap_or_default();
    let properties = if args.properties.is_empty() {
        existing
            .map(|entity| {
                entity
                    .properties
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        let mut merged = existing
            .map(|entity| {
                entity
                    .properties
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<std::collections::BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        for (key, value) in &args.properties {
            merged.insert(key.clone(), value.clone());
        }
        merged.into_iter().collect()
    };
    Ok(EntityDraft {
        id,
        entity_type,
        display,
        description,
        properties,
    })
}

pub(super) fn canonical_entry(path: &Path) -> Result<PathBuf, String> {
    let entry = if path.is_dir() {
        path.join("world.wl")
    } else {
        path.to_path_buf()
    };
    std::fs::canonicalize(&entry)
        .map_err(|error| format!("无法定位工程入口 {}: {error}", entry.display()))
}

pub(super) fn entity_failure(
    args: &EntityArgs,
    out: &mut impl Write,
    code: &str,
    message: String,
    result: Option<&CompileResult>,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
) -> Result<i32, String> {
    if args.json {
        let payload = json!({
            "ok": false,
            "error": { "code": code, "message": message },
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

pub(super) fn parse_entity_args(args: &[String]) -> Result<EntityArgs, String> {
    let mut args = args.iter();
    let operation = match args
        .next()
        .ok_or("entity 需要 create / update / delete 和目录或入口")?
        .as_str()
    {
        "create" => EntityOperation::Create,
        "update" => EntityOperation::Update,
        "delete" => EntityOperation::Delete,
        other => return Err(format!("未知 entity 操作 `{other}`")),
    };
    let path = PathBuf::from(args.next().ok_or("entity 操作需要一个目录或入口")?);
    let mut id = None;
    let mut entity_type = None;
    let mut display = None;
    let mut description = None;
    let mut properties = Vec::new();
    let mut baseline = None;
    let mut json = false;
    while let Some(arg) = args.next() {
        let (key, inline) = arg
            .split_once('=')
            .map(|(key, value)| (key, Some(value)))
            .unwrap_or((arg.as_str(), None));
        match key {
            "--json" => {
                if inline.is_some() {
                    return Err("--json 不接受值".into());
                }
                json = true;
            }
            "--id" => id = Some(entity_value(key, inline, &mut args)?),
            "--kind" => {
                entity_type = Some(entity_value(key, inline, &mut args)?);
            }
            "--display" => display = Some(entity_value(key, inline, &mut args)?),
            "--description" => description = Some(entity_value(key, inline, &mut args)?),
            "--baseline" => baseline = Some(entity_value(key, inline, &mut args)?),
            "--property" => {
                let value = entity_value(key, inline, &mut args)?;
                properties.push(parse_property_arg(&value)?);
            }
            other if other.starts_with("--") => return Err(format!("未知参数 {other}")),
            other => return Err(format!("未知 entity 参数 `{other}`")),
        }
    }
    Ok(EntityArgs {
        path,
        operation,
        id,
        entity_type,
        display,
        description,
        properties,
        baseline,
        json,
    })
}

pub(super) fn entity_value<'a>(
    key: &str,
    inline: Option<&str>,
    args: &mut impl Iterator<Item = &'a String>,
) -> Result<String, String> {
    let value = inline
        .map(str::to_string)
        .or_else(|| args.next().cloned())
        .ok_or_else(|| format!("参数 `{key}` 需要一个值"))?;
    if value.trim().is_empty() {
        return Err(format!("参数 `{key}` 不能为空"));
    }
    Ok(value)
}

pub(super) fn parse_property_arg(value: &str) -> Result<(String, PropertyValue), String> {
    let (name, raw) = value
        .split_once('=')
        .ok_or("--property 格式必须为 name=value")?;
    let name = name.trim();
    if name.is_empty() {
        return Err("property 名称不能为空".into());
    }
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("property 值不能为空".into());
    }
    let parsed = serde_json::from_str::<serde_json::Value>(raw)
        .unwrap_or_else(|_| serde_json::Value::String(raw.trim_matches('"').to_string()));
    let property = match parsed {
        serde_json::Value::String(value) => PropertyValue::Str(value),
        serde_json::Value::Number(value) => {
            PropertyValue::Num(value.as_f64().ok_or("property 数值必须是有限数值")?)
        }
        serde_json::Value::Bool(value) => PropertyValue::Bool(value),
        _ => return Err("property 值只能是字符串、数值或布尔值".into()),
    };
    Ok((name.to_string(), property))
}

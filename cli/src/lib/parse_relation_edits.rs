use super::entity::parse_property_arg;
use super::*;

pub(super) fn parse_target_ref(value: &str) -> Result<TargetRef, String> {
    let (kind, id) = value.split_once(':').ok_or("--target 格式必须为 KIND:ID")?;
    let kind = kind.trim();
    let id = id.trim();
    if kind.is_empty() || id.is_empty() || (kind != "file" && id.contains(':')) {
        return Err("--target 格式必须为非空 KIND:ID".into());
    }
    Ok(TargetRef::new(kind, id))
}

pub(super) fn parse_relation_edit_operation(value: &str) -> Result<RelationEditOperation, String> {
    match value {
        "create" => Ok(RelationEditOperation::Create),
        "update" => Ok(RelationEditOperation::Update),
        "delete" => Ok(RelationEditOperation::Delete),
        other => Err(format!(
            "未知 relation 操作 `{other}`(可用: create / update / delete)"
        )),
    }
}

pub(super) fn relation_flag_value<'a>(
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

pub(super) fn relation_clear_flag(key: &str, inline: Option<&str>) -> Result<(), String> {
    if inline.is_some() {
        return Err(format!("参数 `{key}` 不接受值"));
    }
    Ok(())
}

pub(super) fn parse_relation_type_edit_args(
    args: &[String],
) -> Result<RelationTypeEditArgs, String> {
    let mut iter = args.iter();
    let operation = parse_relation_edit_operation(
        iter.next()
            .ok_or("relation-type 需要 create / update / delete 和目录")?,
    )?;
    let path = PathBuf::from(iter.next().ok_or("relation-type 操作需要一个目录或入口")?);
    let mut id = None;
    let mut display = None;
    let mut inverse_display = None;
    let mut clear_inverse_display = false;
    let mut direction = None;
    let mut from_kind = None;
    let mut clear_from_kind = false;
    let mut to_kind = None;
    let mut clear_to_kind = false;
    let mut baseline = None;
    let mut json = false;
    while let Some(arg) = iter.next() {
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
            "--id" => id = Some(relation_flag_value(key, inline, &mut iter)?),
            "--display" => display = Some(relation_flag_value(key, inline, &mut iter)?),
            "--inverse" | "--inverse-display" => {
                inverse_display = Some(relation_flag_value(key, inline, &mut iter)?)
            }
            "--clear-inverse-display" => {
                relation_clear_flag(key, inline)?;
                clear_inverse_display = true;
            }
            "--direction" => {
                direction = Some(
                    match relation_flag_value(key, inline, &mut iter)?.as_str() {
                        "directed" => RelationDirection::Directed,
                        "undirected" => RelationDirection::Undirected,
                        _ => return Err("参数 `--direction` 需要 directed / undirected".into()),
                    },
                );
            }
            "--from-kind" => from_kind = Some(relation_flag_value(key, inline, &mut iter)?),
            "--clear-from-kind" => {
                relation_clear_flag(key, inline)?;
                clear_from_kind = true;
            }
            "--to-kind" => to_kind = Some(relation_flag_value(key, inline, &mut iter)?),
            "--clear-to-kind" => {
                relation_clear_flag(key, inline)?;
                clear_to_kind = true;
            }
            "--baseline" => baseline = Some(relation_flag_value(key, inline, &mut iter)?),
            key if key.starts_with("--") => return Err(format!("未知参数 {key}")),
            value => return Err(format!("未知 relation-type 参数 `{value}`")),
        }
    }
    if operation != RelationEditOperation::Update
        && (clear_inverse_display || clear_from_kind || clear_to_kind)
    {
        return Err("relation-type 的 --clear-* 参数只能用于 update".into());
    }
    if (clear_inverse_display && inverse_display.is_some())
        || (clear_from_kind && from_kind.is_some())
        || (clear_to_kind && to_kind.is_some())
    {
        return Err("同一关系类型字段不能同时设置和清空".into());
    }
    Ok(RelationTypeEditArgs {
        path,
        operation,
        id: id.ok_or("relation-type 操作需要 --id")?,
        display,
        inverse_display,
        clear_inverse_display,
        direction,
        from_kind,
        clear_from_kind,
        to_kind,
        clear_to_kind,
        baseline,
        json,
    })
}

pub(super) fn parse_relation_edit_args(args: &[String]) -> Result<RelationEditArgs, String> {
    let mut iter = args.iter();
    let operation = parse_relation_edit_operation(
        iter.next()
            .ok_or("relation 需要 create / update / delete 和目录")?,
    )?;
    let path = PathBuf::from(iter.next().ok_or("relation 操作需要一个目录或入口")?);
    let mut id = None;
    let mut relation_type = None;
    let mut from = None;
    let mut to = None;
    let mut description = None;
    let mut source_note = None;
    let mut clear_source_note = false;
    let mut scope_refs = Vec::new();
    let mut clear_scope_refs = false;
    let mut properties = Vec::new();
    let mut clear_properties = false;
    let mut baseline = None;
    let mut json = false;
    while let Some(arg) = iter.next() {
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
            "--id" => id = Some(relation_flag_value(key, inline, &mut iter)?),
            "--type" | "--relation-type" => {
                relation_type = Some(relation_flag_value(key, inline, &mut iter)?)
            }
            "--from" => {
                from = Some(parse_target_ref(&relation_flag_value(
                    key, inline, &mut iter,
                )?)?)
            }
            "--to" => {
                to = Some(parse_target_ref(&relation_flag_value(
                    key, inline, &mut iter,
                )?)?)
            }
            "--description" => description = Some(relation_flag_value(key, inline, &mut iter)?),
            "--source-note" => source_note = Some(relation_flag_value(key, inline, &mut iter)?),
            "--clear-source-note" => {
                relation_clear_flag(key, inline)?;
                clear_source_note = true;
            }
            "--scope" | "--scope-ref" => scope_refs.push(parse_target_ref(&relation_flag_value(
                key, inline, &mut iter,
            )?)?),
            "--clear-scope" => {
                relation_clear_flag(key, inline)?;
                clear_scope_refs = true;
            }
            "--property" => properties.push(parse_property_arg(&relation_flag_value(
                key, inline, &mut iter,
            )?)?),
            "--clear-properties" => {
                relation_clear_flag(key, inline)?;
                clear_properties = true;
            }
            "--baseline" => baseline = Some(relation_flag_value(key, inline, &mut iter)?),
            key if key.starts_with("--") => return Err(format!("未知参数 {key}")),
            value => return Err(format!("未知 relation 参数 `{value}`")),
        }
    }
    if operation != RelationEditOperation::Update
        && (clear_source_note || clear_scope_refs || clear_properties)
    {
        return Err("relation 的 --clear-* 参数只能用于 update".into());
    }
    if (clear_source_note && source_note.is_some())
        || (clear_scope_refs && !scope_refs.is_empty())
        || (clear_properties && !properties.is_empty())
    {
        return Err("同一关系字段不能同时设置和清空".into());
    }
    Ok(RelationEditArgs {
        path,
        operation,
        id: id.ok_or("relation 操作需要 --id")?,
        relation_type,
        from,
        to,
        description,
        source_note,
        clear_source_note,
        scope_refs,
        clear_scope_refs,
        properties,
        clear_properties,
        baseline,
        json,
    })
}

pub(super) fn parse_promotion_args(args: &[String]) -> Result<PromotionArgs, String> {
    let mut iter = args.iter();
    if iter.next().map(String::as_str) != Some("promote") {
        return Err("relations promote 需要 preview 或 commit".into());
    }
    let operation = match iter.next().map(String::as_str) {
        Some("preview") => PromotionOperation::Preview,
        Some("commit") => PromotionOperation::Commit,
        Some(other) => {
            return Err(format!(
                "未知 promotion 操作 `{other}`(可用: preview / commit)"
            ))
        }
        None => return Err("relations promote 需要 preview 或 commit".into()),
    };
    let path = PathBuf::from(iter.next().ok_or("relations promote 操作需要一个目录")?);
    let mut source = None;
    let mut target = None;
    let mut label = None;
    let mut occurrence = 1;
    let mut relation_id = None;
    let mut relation_type = None;
    let mut description = None;
    let mut source_note = None;
    let mut scope_refs = Vec::new();
    let mut properties = Vec::new();
    let mut baseline = None;
    let mut json = false;
    while let Some(arg) = iter.next() {
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
            "--source" => {
                source = Some(parse_target_ref(&relation_flag_value(
                    key, inline, &mut iter,
                )?)?)
            }
            "--target" => {
                target = Some(parse_target_ref(&relation_flag_value(
                    key, inline, &mut iter,
                )?)?)
            }
            "--label" => label = Some(relation_flag_value(key, inline, &mut iter)?),
            "--occurrence" => {
                occurrence = relation_flag_value(key, inline, &mut iter)?
                    .parse::<u32>()
                    .map_err(|_| "参数 `--occurrence` 需要正整数".to_string())?;
                if occurrence == 0 {
                    return Err("参数 `--occurrence` 需要正整数".into());
                }
            }
            "--id" | "--relation-id" => {
                relation_id = Some(relation_flag_value(key, inline, &mut iter)?)
            }
            "--type" | "--relation-type" => {
                relation_type = Some(relation_flag_value(key, inline, &mut iter)?)
            }
            "--description" => description = Some(relation_flag_value(key, inline, &mut iter)?),
            "--source-note" => source_note = Some(relation_flag_value(key, inline, &mut iter)?),
            "--scope" | "--scope-ref" => scope_refs.push(parse_target_ref(&relation_flag_value(
                key, inline, &mut iter,
            )?)?),
            "--property" => properties.push(parse_property_arg(&relation_flag_value(
                key, inline, &mut iter,
            )?)?),
            "--baseline" => baseline = Some(relation_flag_value(key, inline, &mut iter)?),
            key if key.starts_with("--") => return Err(format!("未知参数 {key}")),
            value => return Err(format!("未知 promotion 参数 `{value}`")),
        }
    }
    Ok(PromotionArgs {
        path,
        operation,
        source: source.ok_or("promotion 操作需要 --source KIND:ID")?,
        target: target.ok_or("promotion 操作需要 --target KIND:ID")?,
        label: label.ok_or("promotion 操作需要 --label 标签")?,
        occurrence,
        relation_id: relation_id.ok_or("promotion 操作需要 --id RELATION_ID")?,
        relation_type: relation_type.ok_or("promotion 操作需要 --type TYPE")?,
        description,
        source_note,
        scope_refs,
        properties,
        baseline,
        json,
    })
}

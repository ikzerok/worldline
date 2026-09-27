use super::parse_relation_edits::parse_target_ref;
use super::*;

pub(super) fn parse_relations_args(args: &[String]) -> Result<RelationsArgs, String> {
    let mut path = None;
    let mut target = None;
    let mut depth = 1;
    let mut offset = 0;
    let mut direction = RelationQueryDirection::Both;
    let mut relation_type = None;
    let mut scope_refs = Vec::new();
    let mut include_unscoped = false;
    let mut include_period_children = false;
    let mut json = false;
    let mut iter = args.iter();
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
            "--target" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--target` 需要 KIND:ID")?;
                if target.replace(parse_target_ref(&value)?).is_some() {
                    return Err("参数 `--target` 只能提供一次".into());
                }
            }
            "--offset" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--offset` 需要非负整数")?;
                offset = value
                    .parse::<usize>()
                    .map_err(|_| "参数 `--offset` 需要非负整数")?;
            }
            "--depth" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--depth` 需要 1 或 2")?;
                depth = value
                    .parse::<u8>()
                    .map_err(|_| "参数 `--depth` 需要 1 或 2".to_string())?;
                if !matches!(depth, 1 | 2) {
                    return Err("参数 `--depth` 只能是 1 或 2".into());
                }
            }
            "--direction" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--direction` 需要 outgoing / incoming / both")?;
                direction = match value.as_str() {
                    "outgoing" => RelationQueryDirection::Outgoing,
                    "incoming" => RelationQueryDirection::Incoming,
                    "both" => RelationQueryDirection::Both,
                    _ => return Err("参数 `--direction` 需要 outgoing / incoming / both".into()),
                };
            }
            "--type" | "--relation-type" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--type` 需要关系类型 ID")?;
                if value.trim().is_empty() {
                    return Err("参数 `--type` 不能为空".into());
                }
                if relation_type.replace(value).is_some() {
                    return Err("参数 `--type` 只能提供一次".into());
                }
            }
            "--scope" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--scope` 需要 KIND:ID")?;
                scope_refs.push(parse_target_ref(&value)?);
            }
            "--include-unscoped" => {
                if inline.is_some() {
                    return Err("--include-unscoped 不接受值".into());
                }
                include_unscoped = true;
            }
            "--include-period-children" => {
                if inline.is_some() {
                    return Err("--include-period-children 不接受值".into());
                }
                include_period_children = true;
            }
            key if key.starts_with("--") => return Err(format!("未知参数 {key}")),
            value => {
                if path.replace(PathBuf::from(value)).is_some() {
                    return Err("relations 只能提供一个目录".into());
                }
            }
        }
    }
    Ok(RelationsArgs {
        offset,
        path: path.ok_or("relations 需要一个目录")?,
        target: target.ok_or("relations 需要 --target KIND:ID")?,
        depth,
        direction,
        relation_type,
        scope_refs,
        include_unscoped,
        include_period_children,
        json,
    })
}

pub(super) fn parse_topic_projection_args(args: &[String]) -> Result<TopicProjectionArgs, String> {
    if args.first().map(String::as_str) != Some("project") {
        return Err("relations 需要 project 子命令".into());
    }
    let path = PathBuf::from(args.get(1).ok_or("relations project 需要一个目录或入口")?);
    let mut target = None;
    let mut role_mapping = std::collections::BTreeMap::new();
    let mut mapping_seen = false;
    let mut offset = 0;
    let mut history_offset = 0;
    let mut depth = 1;
    let mut direction = RelationQueryDirection::Both;
    let mut scope_refs = Vec::new();
    let mut include_unscoped = false;
    let mut include_period_children = false;
    let mut max_nodes = 250;
    let mut max_edges = 500;
    let mut json = false;
    let mut iter = args[2..].iter();
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
            "--target" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--target` 需要 KIND:ID")?;
                if target.replace(parse_target_ref(&value)?).is_some() {
                    return Err("参数 `--target` 只能提供一次".into());
                }
            }
            "--role-mapping-json" => {
                if mapping_seen {
                    return Err("参数 `--role-mapping-json` 只能提供一次".into());
                }
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--role-mapping-json` 需要 JSON 对象")?;
                role_mapping = serde_json::from_str(&value)
                    .map_err(|error| format!("参数 `--role-mapping-json` 无效：{error}"))?;
                mapping_seen = true;
            }
            "--offset" | "--history-offset" | "--depth" | "--max-nodes" | "--max-edges" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or_else(|| format!("参数 `{key}` 需要非负整数"))?;
                match key {
                    "--offset" => {
                        offset = value.parse().map_err(|_| "参数 `--offset` 需要非负整数")?
                    }
                    "--history-offset" => {
                        history_offset = value
                            .parse()
                            .map_err(|_| "参数 `--history-offset` 需要非负整数")?
                    }
                    "--depth" => {
                        depth = value
                            .parse::<u8>()
                            .map_err(|_| "参数 `--depth` 需要 1 或 2")?;
                        if !matches!(depth, 1 | 2) {
                            return Err("参数 `--depth` 只能是 1 或 2".into());
                        }
                    }
                    "--max-nodes" => {
                        max_nodes = value
                            .parse()
                            .map_err(|_| "参数 `--max-nodes` 需要非负整数")?
                    }
                    "--max-edges" => {
                        max_edges = value
                            .parse()
                            .map_err(|_| "参数 `--max-edges` 需要非负整数")?
                    }
                    _ => unreachable!(),
                }
            }
            "--direction" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--direction` 需要 outgoing / incoming / both")?;
                direction = match value.as_str() {
                    "outgoing" => RelationQueryDirection::Outgoing,
                    "incoming" => RelationQueryDirection::Incoming,
                    "both" => RelationQueryDirection::Both,
                    _ => return Err("参数 `--direction` 需要 outgoing / incoming / both".into()),
                };
            }
            "--scope" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--scope` 需要 KIND:ID")?;
                scope_refs.push(parse_target_ref(&value)?);
            }
            "--include-unscoped" => {
                if inline.is_some() {
                    return Err("--include-unscoped 不接受值".into());
                }
                include_unscoped = true;
            }
            "--include-period-children" => {
                if inline.is_some() {
                    return Err("--include-period-children 不接受值".into());
                }
                include_period_children = true;
            }
            key if key.starts_with("--") => return Err(format!("未知参数 {key}")),
            value => return Err(format!("relations project 不接受额外位置参数 `{value}`")),
        }
    }
    Ok(TopicProjectionArgs {
        path,
        target: target.ok_or("relations project 需要 --target KIND:ID")?,
        role_mapping,
        offset,
        history_offset,
        depth,
        direction,
        scope_refs,
        include_unscoped,
        include_period_children,
        max_nodes,
        max_edges,
        json,
    })
}

use super::parse_relation_edits::parse_target_ref;
use super::support::{open_workspace_snapshot, query_payload_base};
use super::*;
use worldline_core::WorldContextOptions;

struct Args {
    path: PathBuf,
    target: Option<TargetRef>,
    options: WorldContextOptions,
    left: Option<String>,
    right: Option<String>,
    expected_baseline: Option<String>,
    json: bool,
}

pub(super) fn command(args: &[String], out: &mut impl Write, mode: &str) -> Result<i32, String> {
    let args = parse(args, mode)?;
    let snapshot = match open_workspace_snapshot(&args.path) {
        Ok(snapshot) => snapshot,
        Err(message) => {
            output(
                out,
                args.json,
                json!({"ok":false,"schema_version":1,"language_version":null,
                "workspace_revision":null,"diagnostics":[],"workspace_diagnostics":[],"read_only":false,
                "truncated":false,"continuation":null,"error":{"code":"IO_ERROR","message":message}}),
            )?;
            return Ok(2);
        }
    };
    let mut payload = query_payload_base(&snapshot);
    let mut ok = !snapshot.result.has_errors();
    match mode {
        "object" => match snapshot
            .result
            .lookup_world_object(args.target.as_ref().unwrap())
        {
            Ok(object) => {
                payload.insert("object".into(), json!(object));
                payload.insert(
                    "snapshot".into(),
                    json!(snapshot.result.world_context_snapshot()),
                );
            }
            Err(error) => {
                ok = false;
                payload.insert("object".into(), Value::Null);
                payload.insert(
                    "error".into(),
                    json!({"code":error.code(),"message":error.to_string()}),
                );
            }
        },
        "context" => match snapshot
            .result
            .query_world_context(args.target.as_ref().unwrap(), args.options)
        {
            Ok(mut context) => {
                context.content_baseline = Some(snapshot.baseline.clone());
                payload.insert("truncated".into(), json!(context.truncated));
                payload.insert("context".into(), json!(context));
            }
            Err(error) => {
                ok = false;
                payload.insert("context".into(), Value::Null);
                payload.insert(
                    "error".into(),
                    json!({"code":error.code(),"message":error.to_string()}),
                );
            }
        },
        "temporal" => {
            if args
                .expected_baseline
                .as_ref()
                .is_some_and(|value| value != &snapshot.baseline)
            {
                ok = false;
                payload.insert("comparison".into(), Value::Null);
                payload.insert(
                    "error".into(),
                    json!({"code":"STALE_BASELINE","message":"稿件已变化，请重新查询时间证据"}),
                );
            } else {
                let comparison = snapshot.result.analysis.timeline.compare(
                    args.left.as_deref().unwrap(),
                    args.right.as_deref().unwrap(),
                );
                ok &= !matches!(
                    comparison.relation,
                    worldline_core::timeline::TemporalRelation::Invalid
                        | worldline_core::timeline::TemporalRelation::Unknown
                );
                payload.insert("comparison".into(), json!(comparison));
            }
        }
        _ => unreachable!(),
    }
    payload.insert("ok".into(), json!(ok));
    output(out, args.json, Value::Object(payload))?;
    Ok(if ok { 0 } else { 1 })
}

fn output(out: &mut impl Write, json_mode: bool, value: Value) -> Result<(), String> {
    if json_mode {
        return writeln!(out, "{value}").map_err(|error| error.to_string());
    }
    if let Some(error) = value.get("error") {
        writeln!(
            out,
            "{}: {}",
            error["code"].as_str().unwrap_or_default(),
            error["message"].as_str().unwrap_or_default()
        )
        .map_err(|e| e.to_string())?;
    } else if let Some(context) = value.get("context") {
        writeln!(
            out,
            "{}:{}  已返回 {} / {} 条；完整={}，截断={}",
            context["target"]["kind"].as_str().unwrap_or_default(),
            context["target"]["id"].as_str().unwrap_or_default(),
            context["returned"],
            context["total"],
            context["complete"],
            context["truncated"]
        )
        .map_err(|e| e.to_string())?;
        for record in context["records"].as_array().into_iter().flatten() {
            writeln!(
                out,
                "{}:{} → {}:{}  {} [{}]  {}:{}",
                record["from_ref"]["kind"].as_str().unwrap_or_default(),
                record["from_ref"]["id"].as_str().unwrap_or_default(),
                record["to_ref"]["kind"].as_str().unwrap_or_default(),
                record["to_ref"]["id"].as_str().unwrap_or_default(),
                record["role"].as_str().unwrap_or_default(),
                record["kind"].as_str().unwrap_or_default(),
                record["source"]["file"].as_str().unwrap_or_default(),
                record["source"]["line"]
            )
            .map_err(|e| e.to_string())?;
        }
        if context["complete"] != true {
            writeln!(
                out,
                "结果不完整：{}；请修复诊断或收窄查询范围",
                context["reasons"]
            )
            .map_err(|e| e.to_string())?;
        }
    } else if let Some(object) = value.get("object") {
        writeln!(
            out,
            "{}  {}:{}  {}:{}",
            object["display"].as_str().unwrap_or_default(),
            object["target"]["kind"].as_str().unwrap_or_default(),
            object["target"]["id"].as_str().unwrap_or_default(),
            object["file"].as_str().unwrap_or_default(),
            object["line"]
        )
        .map_err(|e| e.to_string())?;
    } else if let Some(comparison) = value.get("comparison") {
        writeln!(
            out,
            "{} / {}：{}",
            comparison["left"].as_str().unwrap_or_default(),
            comparison["right"].as_str().unwrap_or_default(),
            comparison["relation"].as_str().unwrap_or_default()
        )
        .map_err(|e| e.to_string())?;
        writeln!(out, "证据：{}", comparison["evidence"]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn parse(args: &[String], mode: &str) -> Result<Args, String> {
    let mut path = None;
    let mut values = std::collections::BTreeMap::<String, String>::new();
    let mut json_mode = false;
    let mut iter = args.iter();
    while let Some(argument) = iter.next() {
        let (key, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(key, value)| (key, Some(value)));
        if key == "--json" && inline.is_none() && !json_mode {
            json_mode = true;
        } else if key.starts_with("--") {
            let allowed = match mode {
                "object" => &["--target"][..],
                "context" => &["--target", "--options-json"][..],
                "temporal" => &["--left", "--right", "--expected-baseline"][..],
                _ => unreachable!(),
            };
            if !allowed.contains(&key) {
                return Err(format!("未知或不适用参数 {key}"));
            }
            let value = inline
                .map(str::to_string)
                .or_else(|| iter.next().cloned())
                .ok_or("参数缺少值")?;
            if value.trim().is_empty() || values.insert(key.into(), value).is_some() {
                return Err(format!("参数 {key} 不能为空或重复"));
            }
        } else if path.replace(PathBuf::from(argument)).is_some() {
            return Err("只能提供一个工程目录或入口".into());
        }
    }
    let target = if mode != "temporal" {
        Some(parse_target_ref(
            values.get("--target").ok_or("需要 --target KIND:ID")?,
        )?)
    } else {
        None
    };
    let options = if let Some(value) = values.get("--options-json") {
        let value = worldline_core::parse_unique_json(value.as_bytes())
            .map_err(|e| format!("无效JSON：{e}"))?;
        serde_json::from_value::<WorldContextOptions>(value)
            .map_err(|e| format!("无效上下文选项：{e}"))?
    } else {
        WorldContextOptions::default()
    };
    options.validate().map_err(|e| e.to_string())?;
    if mode == "temporal" && (!values.contains_key("--left") || !values.contains_key("--right")) {
        return Err("时间比较需要 --left 与 --right".into());
    }
    Ok(Args {
        path: path.ok_or("需要工程目录或入口")?,
        target,
        options,
        json: json_mode,
        left: values.remove("--left"),
        right: values.remove("--right"),
        expected_baseline: values.remove("--expected-baseline"),
    })
}

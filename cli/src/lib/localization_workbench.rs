//! Thin CLI routing for the core localization authoring plans.
use super::*;
use worldline_core::localization::{
    LocalizationCatalogQuery, LocalizationEditDraft, LocalizationError, LocalizationIdDraft,
    LocalizationImportDraft,
};

const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;

struct Args {
    kind: String,
    apply: bool,
    path: PathBuf,
    request: Value,
    digest: Option<String>,
    save: bool,
    json: bool,
}

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    let json_mode = args.iter().any(|arg| arg == "--json");
    let parsed = match parse(args) {
        Ok(value) => value,
        Err(message) => return emit(failure("INVALID_PARAMS", message), json_mode, 2, out),
    };
    let input = match Input::decode(&parsed.kind, parsed.request.clone()) {
        Ok(value) => value,
        Err(message) => return emit(failure("INVALID_PARAMS", message), parsed.json, 2, out),
    };
    let mut project = match Project::open_read_only(&parsed.path) {
        Ok(project) => project,
        Err(message) => return emit(failure("IO_ERROR", message), parsed.json, 2, out),
    };
    let mut payload = match input.execute(&mut project, parsed.digest.as_deref()) {
        Ok(value) => value,
        Err(error) => json!({"ok":false,"error":error,"applied":false,"saved":false}),
    };
    if parsed.apply && payload["ok"] == true {
        payload["notice"] = json!("仅内存应用；CLI 退出会丢弃候选，请使用 --save 持久化");
    }
    if parsed.save && payload["ok"] == true {
        match project.save() {
            Ok(()) => {
                payload["saved"] = json!(true);
                payload["notice"] = json!("已通过工程保存事务保存");
            }
            Err(message) => {
                payload["ok"] = json!(false);
                payload["stage"] = json!("save");
                payload["error"] = json!({"code":"SAVE_FAILED","message":message});
                payload["notice"] = json!("内存已应用，保存失败；请保留输入并重新打开处理保存事务");
            }
        }
    }
    payload["baseline"] = json!(project.content_baseline());
    payload["workspace_diagnostics"] = json!(project.authoring_diagnostics());
    payload["read_only"] = payload
        .get("page")
        .and_then(|page| page.get("read_only"))
        .cloned()
        .unwrap_or_else(|| json!(!project.authoring_diagnostics().is_empty()));
    let code = if payload["ok"] == true { 0 } else { 1 };
    emit(payload, parsed.json, code, out)
}

enum Input {
    Catalog(LocalizationCatalogQuery),
    Ids(LocalizationIdDraft),
    Edit(LocalizationEditDraft),
    Import(LocalizationImportDraft),
}

impl Input {
    fn decode(kind: &str, value: Value) -> Result<Self, String> {
        match kind {
            "catalog" => serde_json::from_value(value).map(Self::Catalog),
            "ids" => serde_json::from_value(value).map(Self::Ids),
            "edit" => serde_json::from_value(value).map(Self::Edit),
            "import-candidate" => serde_json::from_value(value).map(Self::Import),
            _ => unreachable!("validated operation"),
        }
        .map_err(|error| format!("本地化请求 DTO 无效：{error}"))
    }

    fn execute(
        self,
        project: &mut Project,
        digest: Option<&str>,
    ) -> Result<Value, LocalizationError> {
        let applied = digest.is_some();
        let operation = if applied { "apply" } else { "preview" };
        let value = match self {
            Self::Catalog(query) => {
                return project.query_localization_catalog(&query)
                    .map(|page| json!({"ok":true,"page":page,"applied":false,"saved":false}));
            }
            Self::Ids(draft) => match digest {
                Some(digest) => project.apply_localization_ids(&draft, digest)
                    .map(|result| json!({"plan":result.plan,"changed_files":result.changed_files,"new_baseline":result.new_baseline})),
                None => project.preview_localization_ids(&draft).map(|plan| json!({"plan":plan})),
            },
            Self::Edit(draft) => match digest {
                Some(digest) => project.apply_localization_edit(&draft, digest)
                    .map(|result| json!({"plan":result.plan,"changed_files":result.changed_files,"new_baseline":result.new_baseline})),
                None => project.preview_localization_edit(&draft).map(|plan| json!({"plan":plan})),
            },
            Self::Import(draft) => match digest {
                Some(digest) => project.apply_localization_import_candidate(&draft.selection, &draft.exchange, digest)
                    .map(|result| json!({"plan":result.plan,"changed_files":result.changed_files,"new_baseline":result.new_baseline})),
                None => project.preview_localization_import_candidate(&draft.selection, &draft.exchange)
                    .map(|plan| json!({"plan":plan})),
            },
        }?;
        let mut payload = value;
        payload["ok"] = json!(true);
        payload["operation"] = json!(operation);
        payload["applied"] = json!(applied);
        payload["saved"] = json!(false);
        Ok(payload)
    }
}

fn parse(args: &[String]) -> Result<Args, String> {
    let kind = args.first().ok_or("localization 需要操作")?.as_str();
    if !matches!(kind, "catalog" | "ids" | "edit" | "import-candidate") {
        return Err("未知本地化工作台操作".into());
    }
    let catalog = kind == "catalog";
    let apply = if catalog {
        false
    } else {
        match args.get(1).map(String::as_str) {
            Some("preview") => false,
            Some("apply") => true,
            _ => return Err("本地化创作需要 preview 或 apply".into()),
        }
    };
    let path_index = if catalog { 1 } else { 2 };
    let path = args.get(path_index).ok_or("需要工程目录或入口")?;
    let mut request = None;
    let mut digest = None;
    let mut save = false;
    let mut json_mode = false;
    let mut iter = args[path_index + 1..].iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        match key {
            "--json" if inline.is_none() && !json_mode => json_mode = true,
            "--save" if inline.is_none() && !save => save = true,
            "--request-json" | "--plan-digest" => {
                let value = inline
                    .map(str::to_owned)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数缺少值")?;
                let slot = if key == "--request-json" {
                    &mut request
                } else {
                    &mut digest
                };
                if value.is_empty() || slot.replace(value).is_some() {
                    return Err(format!("{key} 不能为空或重复"));
                }
            }
            _ => return Err(format!("未知或重复参数：{arg}")),
        }
    }
    if apply != digest.is_some() || (!apply && save) {
        return Err("apply 必须提供 --plan-digest；查询/preview 不能提供摘要或 --save".into());
    }
    let request = request.ok_or("需要 --request-json")?;
    if request.len() > MAX_REQUEST_BYTES {
        return Err("本地化请求超过 8 MiB 限制".into());
    }
    let request = worldline_core::parse_unique_json(request.as_bytes())?;
    Ok(Args {
        kind: kind.into(),
        apply,
        path: path.into(),
        request,
        digest,
        save,
        json: json_mode,
    })
}

fn failure(code: &str, message: String) -> Value {
    json!({"ok":false,"applied":false,"saved":false,"error":{"code":code,"message":message}})
}

fn emit(value: Value, json_mode: bool, code: i32, out: &mut impl Write) -> Result<i32, String> {
    let text = if json_mode {
        value.to_string()
    } else {
        serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
    };
    writeln!(out, "{text}").map_err(|e| e.to_string())?;
    Ok(code)
}

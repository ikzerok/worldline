use super::*;

/// 解析 `<文件.wl> [--json]`;`--load=`/`--save=` 仅 play 接受。
pub(super) fn parse_file_args(
    cmd: &str,
    args: &[String],
    session_flags: bool,
) -> Result<FileArgs, String> {
    let mut path: Option<PathBuf> = None;
    let mut json = false;
    let mut load = None;
    let mut save = None;
    let mut seed = None;
    let mut trace_output = None;
    let mut choice_presentation = false;
    let mut language_version = None;
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--json" => json = true,
            "--choice-presentation" if session_flags => choice_presentation = true,
            other if session_flags && other.starts_with("--load=") => {
                load = Some(PathBuf::from(other.trim_start_matches("--load=")));
            }
            other if session_flags && other.starts_with("--save=") => {
                save = Some(PathBuf::from(other.trim_start_matches("--save=")));
            }
            "--seed" if session_flags => {
                let value = iter.next().ok_or("参数 `--seed` 需要非负整数")?;
                seed = Some(value.parse().map_err(|_| "参数 `--seed` 需要非负整数")?);
            }
            other if session_flags && other.starts_with("--seed=") => {
                seed = Some(
                    other
                        .trim_start_matches("--seed=")
                        .parse()
                        .map_err(|_| "参数 `--seed` 需要非负整数")?,
                );
            }
            "--trace-output" if session_flags => {
                let value = iter.next().ok_or("参数 `--trace-output` 需要文件路径")?;
                trace_output = Some(PathBuf::from(value));
            }
            other if session_flags && other.starts_with("--trace-output=") => {
                trace_output = Some(PathBuf::from(other.trim_start_matches("--trace-output=")));
            }
            "--language-version" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "参数 `--language-version` 需要一个值".to_string())?;
                language_version = Some(parse_language_version(value)?);
            }
            other if other.starts_with("--language-version=") => {
                language_version = Some(parse_language_version(
                    other.trim_start_matches("--language-version="),
                )?);
            }
            other if other.starts_with("--") => return Err(format!("未知参数 {other}")),
            other => {
                if path.replace(PathBuf::from(other)).is_some() {
                    return Err("只能提供一个故事文件".into());
                }
            }
        }
    }
    let Some(path) = path else {
        return Err(format!("子命令 `{cmd}` 需要一个 .wl 故事文件"));
    };
    if load.is_some() && seed.is_some() {
        return Err("`--seed` 只能用于新故事，不能与 `--load` 同时使用".into());
    }
    Ok(FileArgs {
        path,
        json,
        load,
        save,
        seed,
        trace_output,
        choice_presentation,
        language_version,
    })
}

pub(super) fn parse_language_version(value: &str) -> Result<LanguageVersion, String> {
    LanguageVersion::from_supported_str(value).ok_or_else(|| {
        format!(
            "不支持的语言版本 `{value}`(可用: {})",
            LanguageVersion::SUPPORTED
                .map(LanguageVersion::as_str)
                .join(" / ")
        )
    })
}

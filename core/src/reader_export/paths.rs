use std::path::{Component, Path};

/// 将已经作为原生相对文件路径处理的输出名转换为跨平台的 `/` 形式。
/// 仅 Windows 接受原生反斜线分隔符；URL 和 profile 路由字符串应保持严格的 `/` 语法。
pub fn portable_output_path(path: &Path) -> Result<String, String> {
    let raw = path.to_str().ok_or("阅读包输出路径必须为 UTF-8")?;
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!("阅读包输出路径无效：{raw}"));
    }
    portable_text(raw, cfg!(windows))
}

pub(super) fn portable_text(raw: &str, windows_separators: bool) -> Result<String, String> {
    if raw.is_empty()
        || raw.contains([':', '?', '#', '%', '&', '<', '>', '"', '|', '*'])
        || raw.chars().any(char::is_control)
        || (!windows_separators && raw.contains('\\'))
    {
        return Err(format!("阅读包输出路径无效：{raw}"));
    }
    let parts: Vec<_> = raw
        .split(|character| character == '/' || (windows_separators && character == '\\'))
        .collect();
    if parts.iter().any(|part| {
        part.is_empty()
            || matches!(*part, "." | "..")
            || part.ends_with(['.', ' '])
            || device_name(part)
    }) {
        return Err(format!("阅读包输出路径无效：{raw}"));
    }
    Ok(parts.join("/"))
}

fn device_name(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or(part)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) || stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            matches!(
                suffix,
                "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_join_round_trips_to_portable_names_on_every_platform() {
        let native = Path::new("objects").join("index.html");
        assert_eq!(portable_output_path(&native).unwrap(), "objects/index.html");
        assert_eq!(
            portable_text("objects\\index.html", true).unwrap(),
            "objects/index.html"
        );
        assert!(portable_text("objects\\index.html", false).is_err());
    }

    #[test]
    fn normalization_never_accepts_prefixes_devices_escape_or_empty_segments() {
        for path in [
            "C:page.html",
            "C:/page.html",
            "C:\\page.html",
            "\\\\server\\share\\page.html",
            "\\\\?\\C:\\page.html",
            "\\\\.\\NUL",
            "/index.html",
            "\\index.html",
            "../index.html",
            "objects/../index.html",
            "objects\\..\\index.html",
            "objects/./index.html",
            "objects//index.html",
            "objects\\\\index.html",
            "objects/",
            "objects\\",
            "objects/NUL.html",
            "objects/COM1.txt",
            "objects/NUL .html",
            "objects/COM1 .txt",
            "CONIN$",
            "CONOUT$.txt",
            "objects/LPT9.png",
            "objects/con",
            "objects/index.html.",
            "objects/index.html ",
            "objects/%2e%2e/index.html",
            "objects/name:stream",
            "objects/index.html?x",
            "objects/index.html#x",
            "",
        ] {
            assert!(
                portable_text(path, true).is_err(),
                "Windows 接受了 {path:?}"
            );
            assert!(
                portable_text(path, false).is_err(),
                "portable 接受了 {path:?}"
            );
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn unix_literal_backslash_is_a_forbidden_filename_character() {
        assert!(portable_output_path(Path::new("objects\\index.html")).is_err());
    }

    #[test]
    fn profile_route_strings_do_not_use_native_separator_rules() {
        for name in [
            "objects\\o0001.html",
            "C:/objects/o0001.html",
            "//host/objects/o0001.html",
            "objects/../o0001.html",
            "objects//o0001.html",
            "objects/NUL.html",
        ] {
            let route = crate::reader_export::ReaderProfileRoute {
                target: Some(crate::catalog::TargetRef::new("entity", "public")),
                manuscript_id: None,
                chapter_id: None,
                output_path: name.into(),
            };
            assert!(super::super::routes::validate_profile_routes(&[route]).is_err());
        }
    }
}

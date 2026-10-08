//! 已暂停真实试玩的只读查询；消费此行后继续等待输入，不做隐式选择。
use std::io::{BufRead, Write};
use worldline_runtime::{StateInspectionQuery, Story};
pub(super) fn inspect_input(
    story: &Story<'_>,
    line: &str,
    out: &mut impl Write,
) -> Result<bool, String> {
    let line = line.trim();
    let query = if line == "inspect" {
        Some(Ok(StateInspectionQuery::default()))
    } else {
        line.strip_prefix("inspect ").map(serde_json::from_str)
    };
    let Some(query) = query else {
        return Ok(false);
    };
    let payload = match query {
        Ok(query) => match story.inspect_state(&query) {
            Ok(page) => serde_json::json!({"type":"inspection","ok":true,"inspection":page}),
            Err(error) => serde_json::json!({"type":"inspection","ok":false,"error":error}),
        },
        Err(error) => {
            serde_json::json!({"type":"inspection","ok":false,"error":{"code":"INVALID_INSPECTION_QUERY","message":error.to_string()}})
        }
    };
    writeln!(out, "{payload}").map_err(|error| error.to_string())?;
    out.flush().map_err(|error| error.to_string())?;
    Ok(true)
}
pub(super) fn read_input(
    story: &Story<'_>,
    buf: &mut String,
    out: &mut impl Write,
    input: &mut impl BufRead,
) -> Result<bool, String> {
    loop {
        buf.clear();
        if input.read_line(buf).map_err(|error| error.to_string())? == 0 {
            return Ok(false);
        }
        if !inspect_input(story, buf, out)? {
            return Ok(true);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_inspection_reads_without_changing_save_or_trace() {
        let c = worldline_core::compile_source(
            "inspect.wl",
            "let n = 0\nevent start\n  choice \"结束\"\n    -> END\n",
        );
        let mut s = Story::new_with_seed(&c.program, &c.analysis, 5).unwrap();
        s.continue_story().unwrap();
        let save = s.save().unwrap();
        let trace = s.replay_trace();
        let mut out = Vec::new();
        assert!(inspect_input(&s, "inspect {\"text\":\"n\"}", &mut out).unwrap());
        let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value["inspection"]["total_matches"], 1);
        assert_eq!(s.save().unwrap(), save);
        assert_eq!(s.replay_trace(), trace);
    }
}

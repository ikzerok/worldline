//! stdio原始字节分帧；不能将非法UTF-8或真实IO失败静默当作EOF。
use serde_json::{json, Value};
use std::io::{self, BufRead, Cursor, Read, Write};

fn line(id: u64, method: &str, params: Value) -> Vec<u8> {
    let mut bytes =
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .unwrap();
    bytes.push(b'\n');
    bytes
}
fn exchange(bytes: Vec<u8>) -> (i32, Vec<Value>) {
    let mut output = Vec::new();
    let code = worldline_agent::run(&mut Cursor::new(bytes), &mut output);
    let text = String::from_utf8(output).expect("响应必须仍为完整UTF-8");
    (
        code,
        text.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect(),
    )
}

#[test]
fn invalid_utf8_line_returns_parse_error_and_continues_valid_next_line() {
    let mut input = line(1, "initialize", json!({}));
    input.extend_from_slice(
        b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"project.draft_rehearsal\",\"params\":\xff}\n",
    );
    input.extend(line(3, "initialize", json!({})));
    let (code, responses) = exchange(input);
    assert_eq!(code, 0);
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["id"], 1);
    assert!(responses[1]["id"].is_null());
    assert_eq!(responses[1]["error"]["code"], -32700);
    assert_eq!(responses[2]["id"], 3);
}

#[test]
fn invalid_utf8_does_not_execute_or_contaminate_an_existing_live_session() {
    let mut input = Vec::new();
    for message in [
        line(
            1,
            "compile",
            json!({"source":"let n = 0\nevent start\n  choice \"继续🌦️\"\n    set n = 7\n    -> END\n"}),
        ),
        line(2, "session.open", json!({"story_id":"s1","seed":17})),
        line(3, "session.continue", json!({"session_id":"c1"})),
        line(4, "session.state", json!({"session_id":"c1"})),
        line(5, "session.trace", json!({"session_id":"c1"})),
        line(6, "session.save", json!({"session_id":"c1"})),
    ] {
        input.extend(message);
    }
    input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"session.choose\",\"params\":{\"session_id\":\"c1\",\"index\":0,\"invalid\":\xff}}\n");
    for (id, method) in [
        (8, "session.state"),
        (9, "session.trace"),
        (10, "session.save"),
    ] {
        input.extend(line(id, method, json!({"session_id":"c1"})));
    }
    let (code, responses) = exchange(input);
    assert_eq!(code, 0);
    assert_eq!(responses.len(), 10);
    assert_eq!(responses[6]["error"]["code"], -32700);
    assert!(responses[6]["id"].is_null());
    for (before, after) in [(3, 7), (4, 8), (5, 9)] {
        assert_eq!(responses[before]["result"], responses[after]["result"]);
    }
    assert_eq!(responses[9]["id"], 10);
}

#[test]
fn invalid_final_line_and_split_multibyte_input_preserve_framing() {
    let (code, responses) = exchange(vec![0xf0, 0x9f]);
    assert_eq!(code, 0);
    assert_eq!(responses.len(), 1);
    assert!(responses[0]["id"].is_null());
    assert_eq!(responses[0]["error"]["code"], -32700);
    let input = line(
        1,
        "compile",
        json!({"source":"event start\n  完整中文🌦️\n  -> END\n"}),
    );
    let mut reader = io::BufReader::with_capacity(1, Cursor::new(input));
    let mut output = Vec::new();
    assert_eq!(worldline_agent::run(&mut reader, &mut output), 0);
    let response: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(response["result"]["ok"], true);
}

struct FailingReader {
    bytes: Cursor<Vec<u8>>,
    interrupted: bool,
}
impl Read for FailingReader {
    fn read(&mut self, _out: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("底层读取失败"))
    }
}
impl BufRead for FailingReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.interrupted {
            self.interrupted = false;
            return Err(io::Error::new(io::ErrorKind::Interrupted, "重试"));
        }
        if self.bytes.position() as usize == self.bytes.get_ref().len() {
            return Err(io::Error::other("真实输入IO失败"));
        }
        self.bytes.fill_buf()
    }
    fn consume(&mut self, amount: usize) {
        self.bytes.consume(amount);
    }
}

#[test]
fn real_read_failure_exits_nonzero_and_never_executes_a_partial_line() {
    for bytes in [
        Vec::new(),
        b"{\"jsonrpc\":\"2.0\",\"method\":\"shutdown\"".to_vec(),
    ] {
        let mut reader = FailingReader {
            bytes: Cursor::new(bytes),
            interrupted: false,
        };
        let mut output = Vec::new();
        assert_eq!(worldline_agent::run(&mut reader, &mut output), 2);
        assert!(output.is_empty());
    }
}

#[test]
fn interrupted_read_retries_before_real_io_failure_without_losing_valid_line() {
    let mut reader = FailingReader {
        bytes: Cursor::new(line(1, "initialize", json!({}))),
        interrupted: true,
    };
    let mut output = Vec::new();
    assert_eq!(worldline_agent::run(&mut reader, &mut output), 2);
    let response: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(response["id"], 1);
    assert!(response.get("result").is_some());
}

struct ClosedWriter;
impl Write for ClosedWriter {
    fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "对端关闭"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn broken_writer_still_ends_normally_for_valid_and_invalid_input() {
    for input in [line(1, "initialize", json!({})), vec![0xff, b'\n']] {
        assert_eq!(
            worldline_agent::run(&mut Cursor::new(input), &mut ClosedWriter),
            0
        );
    }
}

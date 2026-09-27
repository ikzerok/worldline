//! wl-agent 协议集成测试:直接驱动 lib 层 run(),不启动进程。

use std::io::{BufRead, Cursor, Read};
use std::path::Path;

use serde_json::{json, Value};

/// 发送一批请求行,返回 (退出码, 响应列表)。
fn exchange(lines: &[Value]) -> (i32, Vec<Value>) {
    let input = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    exchange_raw(&input)
}

/// 同 exchange,但接受原始文本(测坏 JSON 等非合法请求)。
fn exchange_raw(input: &str) -> (i32, Vec<Value>) {
    let mut out = Vec::new();
    let code = worldline_agent::run(&mut Cursor::new(input.to_string()), &mut out);
    let responses = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("每行响应都应为合法 JSON"))
        .collect();
    (code, responses)
}

fn req(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

const STORY: &str = "event start\n  开场。\n  choice \"甲\"\n    甲线。\n    -> END\n  choice \"乙\"\n    乙线。\n    -> END\n";

const RELATION_STORY: &str = "entity lighthouse kind place as \"灯塔\"\nentity keepers kind organization as \"守灯会\"\nrelation_type maintains as \"维护\"\n  inverse \"由其维护\"\n  direction directed\nrelation_def rel_keepers_lighthouse type maintains from entity keepers to entity lighthouse\n  description \"守灯会维护灯塔\"\nevent start\n  -> END\n";

fn temp_workspace(name: &str, manifest: &str, source: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir()
        .join("worldline_agent_workspace_tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(root.join(".world/project.json"), manifest).unwrap();
    std::fs::write(root.join("world.wl"), source).unwrap();
    root
}

fn temp_entity_project(name: &str, source: &str) -> std::path::PathBuf {
    temp_workspace(
        &format!("entity-{name}"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":[]}"#,
        source,
    )
}

fn temp_authoring_intent_project(name: &str, source: &str) -> std::path::PathBuf {
    let root = temp_workspace(
        &format!("authoring-intent-{name}"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["presentation.maps.v1"]}"#,
        source,
    );
    let manifest_path = root.join(".world/project.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["maps"] = json!({"overview":".world/maps/overview.json"});
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let map_path = root.join(".world/maps/overview.json");
    std::fs::create_dir_all(map_path.parent().unwrap()).unwrap();
    std::fs::write(
        &map_path,
        r#"{"schema_version":1,"id":"overview","title":"总览","canvas":{"width":100,"height":100,"unit":"normalized"},"layer_order":["places"],"layers":{"places":{"title":"地点","visible_default":true,"locked":false}},"placements":{},"extension":{"preserve":true}}"#.as_bytes(),
    )
    .unwrap();
    root
}

fn temp_relation_project(name: &str, source: &str) -> std::path::PathBuf {
    temp_workspace(
        &format!("relation-{name}"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.relations.v1"]}"#,
        source,
    )
}

fn temp_markdown_import_source(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir()
        .join("worldline_agent_markdown_import_tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("harbor.md"),
        "---\nid: harbor\ntitle: 雾港\nkind: place\n---\n# 雾港\n**潮汐**\n",
    )
    .unwrap();
    root
}
#[path = "protocol/authoring_imports.rs"]
mod authoring_imports;
#[path = "protocol/catalog_exports.rs"]
mod catalog_exports;
#[path = "protocol/project_mutations.rs"]
mod project_mutations;
#[path = "protocol/relation_queries.rs"]
mod relation_queries;
#[path = "protocol/sessions.rs"]
mod sessions;
#[path = "protocol/transport.rs"]
mod transport;
#[path = "protocol/workspace_maps.rs"]
mod workspace_maps;

fn register_entity_test_map(root: &std::path::Path) -> std::path::PathBuf {
    let manifest_path = root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["maps"] = serde_json::json!({"overview": ".world/maps/overview.json"});
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let map_path = root.join(".world/maps/overview.json");
    std::fs::create_dir_all(map_path.parent().unwrap()).unwrap();
    std::fs::write(
        &map_path,
        br#"{"schema_version":1,"required_features":[],"layers":[]}"#,
    )
    .unwrap();
    map_path
}

struct MapMutatingReader {
    lines: Vec<Vec<u8>>,
    next: usize,
    buffer: Vec<u8>,
    map_path: std::path::PathBuf,
    replacement: Vec<u8>,
    mutated: bool,
}

impl MapMutatingReader {
    fn new(lines: &[Value], map_path: &Path, replacement: &[u8]) -> Self {
        Self {
            lines: lines
                .iter()
                .map(|line| format!("{line}\n").into_bytes())
                .collect(),
            next: 0,
            buffer: Vec::new(),
            map_path: map_path.to_path_buf(),
            replacement: replacement.to_vec(),
            mutated: false,
        }
    }

    fn load_next(&mut self) {
        if self.next == 1 && !self.mutated {
            std::fs::write(&self.map_path, &self.replacement).unwrap();
            self.mutated = true;
        }
        if let Some(line) = self.lines.get(self.next) {
            self.buffer = line.clone();
            self.next += 1;
        }
    }
}

impl Read for MapMutatingReader {
    fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
        if self.buffer.is_empty() {
            self.load_next();
        }
        let size = target.len().min(self.buffer.len());
        target[..size].copy_from_slice(&self.buffer[..size]);
        self.consume(size);
        Ok(size)
    }
}

impl BufRead for MapMutatingReader {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.buffer.is_empty() {
            self.load_next();
        }
        Ok(&self.buffer)
    }

    fn consume(&mut self, amount: usize) {
        self.buffer.drain(..amount.min(self.buffer.len()));
    }
}

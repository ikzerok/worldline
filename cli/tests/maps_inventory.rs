//! 地图目录人类数量与权威 JSON 登记集合保持一致，不把显隐或绑定数当图元数。
use serde_json::{json, Value};
use std::io::Cursor;
use worldline_core::vector_scene::{SceneGeometry, SceneNode, SceneOp};
#[path = "../../core/tests/support/scene_protocol_fixture.rs"]
mod fixture;
use fixture::Fixture;

fn run(args: &[String]) -> (i32, String) {
    let mut output = Vec::new();
    let code = wl::run(args, &mut output, &mut Cursor::new("")).unwrap();
    (code, String::from_utf8(output).unwrap())
}

fn prepare(label: &str, legacy: bool, native: bool) -> Fixture {
    let fixture = Fixture::new(label);
    if legacy {
        let mut map: Value = serde_json::from_slice(&fixture.bytes()).unwrap();
        let entry = fixture.project().compile().program.entry;
        map["placements"]["legacy"] = json!({
            "layer_id":"places", "annotation":"旧标记", "role":"reference",
            "target_ref":{"kind":"event","id":entry},
            "geometry":{"kind":"point","position":[0.2,0.3]}
        });
        std::fs::write(&fixture.map, serde_json::to_vec(&map).unwrap()).unwrap();
    }
    if native {
        let mut batch = fixture.batch();
        let mut hidden = SceneNode::new(
            "hidden",
            "places",
            SceneGeometry::Point {
                position: [40.0, 60.0],
            },
        );
        hidden.visible = false;
        for node in [
            hidden,
            SceneNode::new("group", "places", SceneGeometry::Group { children: vec![] }),
        ] {
            batch.operations.push(SceneOp::Insert { node, index: None });
        }
        let (baseline, digest, _) = fixture.preview(&batch);
        let (code, output) = run(&[
            "scene".into(),
            "apply".into(),
            fixture.root.display().to_string(),
            "--request-json".into(),
            serde_json::to_string(&batch).unwrap(),
            "--baseline".into(),
            baseline,
            "--plan-digest".into(),
            digest,
            "--json".into(),
        ]);
        assert_eq!(code, 0, "{output}");
    }
    fixture
}

#[test]
fn human_map_counts_match_legacy_native_mixed_and_empty_json_without_writes() {
    for (name, legacy, native) in [
        ("empty", false, false),
        ("legacy", true, false),
        ("native", false, true),
        ("mixed", true, true),
    ] {
        let fixture = prepare(name, legacy, native);
        let before = fixture.bytes();
        let args = vec![
            "maps".into(),
            "list".into(),
            fixture.root.display().to_string(),
        ];
        let (code, human) = run(&args);
        assert_eq!(code, 0, "{name}: {human}");
        let mut machine_args = args;
        machine_args.push("--json".into());
        let (code, machine) = run(&machine_args);
        assert_eq!(code, 0, "{name}: {machine}");
        let data: Value = serde_json::from_str(&machine).unwrap();
        assert_eq!(data["read_only"], false, "{machine}");
        assert!(
            data["workspace_diagnostics"].as_array().unwrap().is_empty(),
            "{machine}"
        );
        let map = data["maps"]
            .get("atlas")
            .unwrap_or_else(|| panic!("{name}: {machine}"));
        let old_count = map["placements"].as_object().map_or(0, |items| items.len());
        let native_count = map["scene"]["nodes"]
            .as_object()
            .map_or(0, |nodes| nodes.len());
        assert_eq!(old_count, usize::from(legacy));
        assert_eq!(native_count, if native { 3 } else { 0 });
        assert!(
            human.contains(&format!(
                "atlas  雾港地图  ({old_count} 个旧标记 / {native_count} 个原生图元)"
            )),
            "{human}"
        );
        assert_eq!(fixture.bytes(), before, "只读列表不能改变地图");
        if native {
            assert_eq!(map["scene"]["nodes"]["hidden"]["visible"], false);
            assert_eq!(map["scene"]["nodes"]["group"]["geometry"]["kind"], "group");
        }
    }
}

//! 可显式运行的作者任务测量；不是跨平台 SLA，也不在普通回归中强加墙钟断言。
use serde_json::json;
use std::{fs, time::Instant};
use worldline_core::{project::Project, TargetRef, WorldContextKind, WorldContextOptions};

#[test]
#[ignore = "explicit measured author-task benchmark"]
fn world_context_author_task_measurements() {
    for (kind, count) in [
        ("entity", 100),
        ("entity", 1000),
        ("entity", 5000),
        ("character", 100),
        ("character", 1000),
    ] {
        let root = std::env::temp_dir().join(format!(
            "world-context-measure-{}-{kind}-{count}",
            std::process::id()
        ));
        fs::create_dir_all(root.join(".world")).unwrap();
        fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.13","required_features":["content.object_refs.v1","content.character_refs.v1","content.relations.v1"]}"#).unwrap();
        let mut source = String::from("relation_type next\n");
        for index in 0..count {
            if kind == "entity" {
                source.push_str(&format!("entity item_{index} kind place\n"));
            } else {
                source.push_str(&format!("character item_{index} as \"人物{index}\"\n"));
                if index + 1 < count {
                    source.push_str(&format!(
                        "  property mentor = ref(\"character\", \"item_{}\")\n",
                        index + 1
                    ));
                }
                if index > 0 {
                    source.push_str(&format!("  relation item_{} as \"伙伴\"\n", index - 1));
                }
            }
        }
        if kind == "entity" {
            for index in 0..count - 1 {
                source.push_str(&format!("relation_def edge_{index} type next from entity item_{index} to entity item_{}\n", index + 1));
            }
        }
        source.push_str("event start\n  -> END\n");
        fs::write(root.join("world.wl"), source).unwrap();
        let mut elapsed = Vec::new();
        let mut exact = Vec::new();
        let mut context = Vec::new();
        let mut first = None;
        for sample in 0..11 {
            let task_start = Instant::now();
            let mut project = Project::open(&root).unwrap();
            let compiled = project.compile();
            assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
            let selected = TargetRef::new(kind, &format!("item_{}", count / 2));
            let exact_start = Instant::now();
            assert_eq!(
                compiled.lookup_world_object(&selected).unwrap().target,
                selected
            );
            let exact_ms = exact_start.elapsed().as_secs_f64() * 1000.0;
            let context_start = Instant::now();
            let result = compiled
                .query_world_context(
                    &selected,
                    WorldContextOptions {
                        depth: 2,
                        ..Default::default()
                    },
                )
                .unwrap();
            let context_ms = context_start.elapsed().as_secs_f64() * 1000.0;
            assert!(result.complete);
            assert_eq!(result.nodes.len(), 5);
            assert_eq!(result.returned, if kind == "entity" { 4 } else { 8 });
            if kind == "character" {
                assert_eq!(
                    result
                        .records
                        .iter()
                        .filter(|record| record.kind == WorldContextKind::PropertyReference)
                        .count(),
                    4
                );
                assert_eq!(
                    result
                        .records
                        .iter()
                        .filter(|record| record.kind == WorldContextKind::LegacyCharacterRelation)
                        .count(),
                    4
                );
            }
            let task_ms = task_start.elapsed().as_secs_f64() * 1000.0;
            if sample == 0 {
                first = Some(task_ms);
            } else {
                elapsed.push(task_ms);
                exact.push(exact_ms);
                context.push(context_ms);
            }
        }
        println!(
            "{}",
            json!({"kind":kind,"objects":count,"samples":10,"cache":"fresh Project/compile, OS warm after first; no cache eviction", "first_observed_ms":first,"author_task_ms":summary(elapsed),"exact_only_ms":summary(exact),"context_only_ms":summary(context),"complete":true})
        );
        fs::remove_dir_all(root).unwrap();
    }
}
fn summary(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    json!({"p50":values[values.len()/2-1],"p95":values[(values.len()*95).div_ceil(100)-1],"max":values.last()})
}

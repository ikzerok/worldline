//! 可显式运行的同fixture规模采样；进程peak RSS由外部time工具记录。
use std::time::Instant;
use worldline_core::compile_source;
use worldline_runtime::{
    compare_routes, ReplayBudget, ReplayCancellation, RouteComparisonSession, Story,
};

#[test]
#[ignore = "资源采样需显式串行运行，并由宿主记录peak RSS"]
fn comparison_scale_sample() {
    let scale = std::env::var("WL_COMPARE_SCALE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(50);
    assert!([50, 200, 400].contains(&scale));
    let mut source = String::from("tag kept\ntag sent\nworld setting\n");
    for index in 0..scale {
        source += &format!("state s{index} on world setting with []\nlet v{index} = 0\n");
    }
    source += "event start\n  choice \"保留\"\n";
    for index in 0..scale {
        source += &format!("    become s{index} with kept\n    set v{index} = 1\n");
    }
    source += "    -> END\n  choice \"送出\"\n";
    for index in 0..scale {
        source += &format!("    become s{index} with sent\n    set v{index} = 2\n");
    }
    source += "    -> END\n";
    let compiled = compile_source("scale.wl", &source);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let traces = (0..2)
        .map(|index| {
            let mut story =
                Story::new_with_seed(&compiled.program, &compiled.analysis, 42).unwrap();
            story.continue_story().unwrap();
            story.choose(index).unwrap();
            story.continue_story().unwrap();
            story.replay_trace()
        })
        .collect::<Vec<_>>();
    let start = Instant::now();
    let synchronous = compare_routes(
        &compiled,
        &traces[0],
        &traces[1],
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    let sync_us = start.elapsed().as_micros();
    let start = Instant::now();
    let mut session = RouteComparisonSession::new(
        &compiled,
        traces[0].clone(),
        traces[1].clone(),
        Default::default(),
        ReplayCancellation::new(),
    )
    .unwrap();
    let mut slices = 0;
    let cooperative = loop {
        slices += 1;
        if let Some(result) = session
            .advance(&compiled, ReplayBudget::new(32, 10))
            .unwrap()
        {
            break result;
        }
    };
    let cooperative_us = start.elapsed().as_micros();
    assert_eq!(cooperative, synchronous);
    let cancel = ReplayCancellation::new();
    let mut session = RouteComparisonSession::new(
        &compiled,
        traces[0].clone(),
        traces[1].clone(),
        Default::default(),
        cancel.clone(),
    )
    .unwrap();
    session
        .advance(&compiled, ReplayBudget::new(8, 10))
        .unwrap();
    cancel.cancel();
    let start = Instant::now();
    let stopped = session
        .advance(&compiled, ReplayBudget::new(32, 10))
        .unwrap()
        .unwrap();
    let cancel_us = start.elapsed().as_micros();
    assert_eq!(
        stopped.left.status,
        worldline_runtime::RouteStatus::Cancelled
    );
    assert_eq!(
        stopped.right.status,
        worldline_runtime::RouteStatus::Cancelled
    );
    let observations = traces
        .iter()
        .flat_map(|trace| {
            trace.initial_observation.iter().chain(
                trace
                    .steps
                    .iter()
                    .filter_map(|step| step.observation.as_ref()),
            )
        })
        .collect::<Vec<_>>();
    let output_count = observations
        .iter()
        .map(|observation| observation.outputs.len())
        .sum::<usize>();
    let output_bytes = observations
        .iter()
        .flat_map(|observation| &observation.outputs)
        .map(|output| serde_json::to_vec(output).unwrap().len())
        .sum::<usize>();
    println!(
        "{}",
        serde_json::json!({
            "scale":scale,"source_bytes":source.len(),"states":scale,"vars":scale,"events":1,"choices":2,
            "trace_bytes":traces.iter().map(|trace|serde_json::to_vec(trace).unwrap().len()).collect::<Vec<_>>(),
            "trace_inputs":traces.iter().map(|trace|trace.steps.len()).collect::<Vec<_>>(),
            "executed_steps":synchronous.left.executed_steps+synchronous.right.executed_steps,
            "verified_output_count":output_count,"verified_output_bytes":output_bytes,
            "actions_total":synchronous.left.state_actions.total_actions+synchronous.right.state_actions.total_actions,
            "actions_retained":synchronous.left.state_actions.records.len()+synchronous.right.state_actions.records.len(),
            "omitted":synchronous.omitted,"result_bytes":serde_json::to_vec(&synchronous).unwrap().len(),
            "sync_us":sync_us,"cooperative_us":cooperative_us,"cooperative_slices":slices,"cancel_us":cancel_us,
            "cancel_steps":stopped.left.executed_steps+stopped.right.executed_steps,
            "os":std::env::consts::OS,"arch":std::env::consts::ARCH,"debug":cfg!(debug_assertions),"runtime":env!("CARGO_PKG_VERSION"),
            "samples":1,"peak_rss":"recorded by external process measurement"
        })
    );
}

use super::{
    cycles::analyze_cycles, evidence::EdgeGraph, PeriodInfo, TemporalCycle, TemporalEdge,
    TemporalEvent, TemporalOrderScope, Timeline, TimelineStatus,
};
use crate::{Diagnostic, Program, Span};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

pub(crate) fn analyze(program: &Program, diagnostics: &mut Vec<Diagnostic>) -> Timeline {
    let root_order = program.language_version.supports_language_113();
    let mut timeline = Timeline {
        order_scope: if root_order {
            TemporalOrderScope::RootPeriod
        } else {
            TemporalOrderScope::DirectPeriod
        },
        status: TimelineStatus::Complete,
        ..Timeline::default()
    };
    let mut periods = HashSet::new();
    let mut ambiguous = HashSet::new();
    for period in &program.periods {
        if !periods.insert(period.name.clone()) {
            ambiguous.insert(period.name.clone());
            diagnostics.push(Diagnostic::error(
                "A104",
                &period.file,
                Span::new(period.loc.line, 1, 6),
                format!("时段 `{}` 重复定义", period.name),
            ));
        } else {
            timeline.periods.push(PeriodInfo {
                parent: period.parent.clone(),
                root: None,
                id: period.name.clone(),
                display: period
                    .display
                    .clone()
                    .unwrap_or_else(|| period.name.clone()),
                file: period.file.clone(),
                line: period.loc.line,
            });
        }
    }
    let parents: HashMap<_, _> = timeline
        .periods
        .iter()
        .map(|p| (p.id.clone(), p.parent.clone()))
        .collect();
    for period in &mut timeline.periods {
        let mut seen = HashSet::new();
        let mut current = period.id.as_str();
        loop {
            if ambiguous.contains(current) {
                break;
            }
            let next = parents.get(current);
            if !seen.insert(current) || next.is_none() {
                diagnostics.push(Diagnostic::error(
                    "A219",
                    &period.file,
                    Span::new(period.line, 1, 6),
                    format!("时段 `{}` 的上级不存在或包含关系有环：{current}", period.id),
                ));
                break;
            }
            if let Some(parent) = next.unwrap() {
                current = parent;
            } else {
                period.root = Some(current.into());
                break;
            }
        }
    }
    let roots: HashMap<_, _> = timeline
        .periods
        .iter()
        .map(|p| (p.id.as_str(), p.root.as_deref()))
        .collect();
    let event_roots: Vec<_> = program
        .events
        .iter()
        .map(|e| {
            e.period
                .as_deref()
                .and_then(|p| roots.get(p).copied().flatten())
        })
        .collect();
    let ids: HashMap<_, _> = program
        .events
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name.as_str(), i))
        .collect();
    let mut adjacency = vec![Vec::new(); program.events.len()];
    let mut direct = vec![Vec::new(); program.events.len()];
    for (i, event) in program.events.iter().enumerate() {
        if event.period.as_ref().is_some_and(|p| !periods.contains(p)) {
            report(
                program,
                diagnostics,
                i,
                None,
                format!("事件 `{}` 引用未定义时段", event.name),
            );
        }
        let mut seen = HashSet::new();
        for predecessor in &event.predecessors {
            if !seen.insert(predecessor) {
                continue;
            }
            let Some(&before) = ids.get(predecessor.as_str()) else {
                report(
                    program,
                    diagnostics,
                    i,
                    None,
                    format!("前驱事件 `{predecessor}` 不存在"),
                );
                continue;
            };
            let same_period =
                event.period.is_some() && event.period == program.events[before].period;
            if let Some(required) = super::scope_rejection(
                timeline.order_scope,
                event.period.as_deref(),
                event_roots[i],
                program.events[before].period.as_deref(),
                event_roots[before],
            ) {
                report(
                    program,
                    diagnostics,
                    i,
                    Some(before),
                    format!("时间约束 `{predecessor}` → `{}` 必须{required}", event.name,),
                );
                continue;
            }
            adjacency[before].push(i);
            if same_period {
                direct[before].push(i);
            }
            timeline.edges.push(TemporalEdge {
                before: predecessor.clone(),
                after: event.name.clone(),
                root: event_roots[i].map(str::to_owned),
                order_scope: if root_order {
                    event_roots[i].unwrap().into()
                } else {
                    event.period.clone().unwrap()
                },
                file: program.event_files[i].clone(),
                line: event.loc.line,
            });
        }
    }
    (timeline.cycles, timeline.blocked) = analyze_cycles(&timeline.edges);
    report_cycles(program, diagnostics, &timeline);
    let root_rank = ranks(&adjacency);
    let direct_rank = ranks(&direct);
    for (i, event) in program.events.iter().enumerate() {
        if let Some(period) = &event.period {
            timeline.events.push(TemporalEvent {
                event: event.name.clone(),
                period: period.clone(),
                rank: direct_rank[i],
                root: event_roots[i].map(str::to_owned),
                order_scope: if root_order {
                    event_roots[i].map(str::to_owned)
                } else {
                    periods.contains(period).then(|| period.clone())
                },
                root_rank: (root_order && event_roots[i].is_some()).then_some(root_rank[i]),
                status: TimelineStatus::Complete,
            });
        } else {
            timeline.unplaced_events.push(event.name.clone());
        }
    }
    timeline.unplaced_events.sort();
    timeline.unplaced_events.dedup();
    timeline.mark_incomplete(diagnostics);
    timeline
}

fn report(
    program: &Program,
    diagnostics: &mut Vec<Diagnostic>,
    after: usize,
    before: Option<usize>,
    message: String,
) {
    let mut diagnostic = Diagnostic::error(
        "A213",
        &program.event_files[after],
        Span::new(program.events[after].loc.line, 1, 5),
        message,
    );
    if let Some(before) = before {
        diagnostic = diagnostic.with_related(
            &program.event_files[before],
            Span::new(program.events[before].loc.line, 1, 5),
        );
    }
    diagnostics.push(diagnostic);
}

fn report_cycles(program: &Program, diagnostics: &mut Vec<Diagnostic>, timeline: &Timeline) {
    let cycles: BTreeMap<_, _> = timeline.cycles.iter().map(|c| (c.id.as_str(), c)).collect();
    let members: BTreeMap<_, _> = timeline
        .cycles
        .iter()
        .flat_map(|c| c.members.iter().map(move |id| (id.as_str(), c)))
        .collect();
    let blocked: BTreeMap<_, _> = timeline
        .blocked
        .iter()
        .map(|b| (b.event.as_str(), &b.cycle_ids))
        .collect();
    let graph = EdgeGraph::new(&timeline.edges);
    for (i, event) in program.events.iter().enumerate() {
        let (message, causes): (_, Vec<&TemporalCycle>) =
            if let Some(cycle) = members.get(event.name.as_str()) {
                (
                    format!("事件 `{}` 属于时间约束环 `{}`", event.name, cycle.id),
                    vec![*cycle],
                )
            } else if let Some(ids) = blocked.get(event.name.as_str()) {
                (
                    format!(
                        "事件 `{}` 受时间约束环 {} 阻断（本事件不是环成员）",
                        event.name,
                        ids.join("、")
                    ),
                    ids.iter().map(|id| cycles[id.as_str()]).collect(),
                )
            } else {
                continue;
            };
        let mut diagnostic = Diagnostic::error(
            "A213",
            &program.event_files[i],
            Span::new(event.loc.line, 1, 5),
            message,
        );
        let mut notes = Vec::new();
        for cycle in causes {
            notes.push(format!(
                "环 `{}` 的闭环见证：{}",
                cycle.id,
                path_label(&cycle.witness)
            ));
            add_locations(&mut diagnostic, &cycle.witness);
            if !members.contains_key(event.name.as_str()) {
                if let Some(path) = graph.path(
                    graph.indices[cycle.id.as_str()],
                    graph.indices[event.name.as_str()],
                    None,
                ) {
                    notes.push(format!("受阻路径：{}", path_label(&path)));
                    add_locations(&mut diagnostic, &path);
                }
            }
        }
        diagnostic.note = Some(notes.join("；"));
        diagnostics.push(diagnostic);
    }
}

fn path_label(edges: &[TemporalEdge]) -> String {
    edges
        .first()
        .map(|first| {
            std::iter::once(first.before.as_str())
                .chain(edges.iter().map(|e| e.after.as_str()))
                .collect::<Vec<_>>()
                .join(" → ")
        })
        .unwrap_or_default()
}

fn add_locations(diagnostic: &mut Diagnostic, edges: &[TemporalEdge]) {
    for edge in edges {
        let location = (edge.file.clone(), Span::new(edge.line, 1, 5));
        if !diagnostic.related.contains(&location) {
            diagnostic.related.push(location);
        }
    }
}

fn ranks(adjacency: &[Vec<usize>]) -> Vec<u32> {
    let mut degree = vec![0usize; adjacency.len()];
    let mut rank = vec![0u32; adjacency.len()];
    for targets in adjacency {
        for &target in targets {
            degree[target] += 1;
        }
    }
    let mut queue: VecDeque<_> = degree
        .iter()
        .enumerate()
        .filter(|(_, d)| **d == 0)
        .map(|(i, _)| i)
        .collect();
    while let Some(i) = queue.pop_front() {
        for &next in &adjacency[i] {
            rank[next] = rank[next].max(rank[i] + 1);
            degree[next] -= 1;
            if degree[next] == 0 {
                queue.push_back(next);
            }
        }
    }
    rank
}

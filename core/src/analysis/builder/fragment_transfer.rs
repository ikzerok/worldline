//! 保守片段跃迁摘要：只裁去永远没有 Node 跃迁的展开，不求值任何条件。
use crate::ast::{DivertTarget, Program, Stmt};
use std::collections::{HashMap, HashSet, VecDeque};

pub(super) const MAX_EXPANSIONS: usize = 32_768;
pub(super) const MAX_EDGES: usize = 8_192;

pub(super) struct Projection {
    pub indices: HashMap<String, usize>,
    may_transfer: Vec<bool>,
    pub expansions: usize,
    pub edges: usize,
    pub stopped: bool,
}

impl Projection {
    pub fn new(program: &Program) -> Self {
        let mut indices = HashMap::new();
        for (index, fragment) in program.fragments.iter().enumerate() {
            // 与原投影 find 的首个声明选择相同；重复声明仍由统一诊断拒绝。
            indices.entry(fragment.name.clone()).or_insert(index);
        }
        let mut may_transfer = vec![false; program.fragments.len()];
        let mut callers = vec![Vec::new(); program.fragments.len()];
        let mut queue = VecDeque::new();
        for (index, fragment) in program.fragments.iter().enumerate() {
            let mut calls = HashSet::new();
            scan(&fragment.body, &mut may_transfer[index], &mut calls);
            if may_transfer[index] {
                queue.push_back(index);
            }
            for name in calls {
                if let Some(&callee) = indices.get(name) {
                    callers[callee].push(index);
                }
            }
        }
        while let Some(callee) = queue.pop_front() {
            for &caller in &callers[callee] {
                if !may_transfer[caller] {
                    may_transfer[caller] = true;
                    queue.push_back(caller);
                }
            }
        }
        Self {
            indices,
            may_transfer,
            expansions: 0,
            edges: 0,
            stopped: false,
        }
    }

    pub fn transferable(&self, name: &str) -> Option<usize> {
        self.indices
            .get(name)
            .copied()
            .filter(|&index| self.may_transfer[index])
    }
}

fn scan<'a>(body: &'a [Stmt], transfers: &mut bool, calls: &mut HashSet<&'a str>) {
    for statement in body {
        match statement {
            Stmt::Divert(divert) => *transfers |= matches!(divert.target, DivertTarget::Node(_)),
            Stmt::Call(call) => {
                calls.insert(&call.name);
            }
            Stmt::If(branches) => {
                for (_, body) in &branches.branches {
                    scan(body, transfers, calls);
                }
            }
            Stmt::Choice(choice) => scan(&choice.body, transfers, calls),
            Stmt::Scene(scene) => scan(&scene.body, transfers, calls),
            _ => {}
        }
    }
}

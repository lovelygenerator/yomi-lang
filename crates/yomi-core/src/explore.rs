//! Exploration of the branches of a script (`spec/host/08-test.md`).
//!
//! A breadth-first search over the states at stop points. The VM goes back to a state with a snapshot (and the persistent
//! data, which a snapshot does not hold). States are compared by [`Vm::state_key`], so a state reached again is not explored again.

use std::collections::{BTreeSet, HashMap, VecDeque};

use crate::program::{Program, StopInfo, StopKind};
use crate::registry::Registry;
use crate::value::{Ty, Value};
use crate::vm::{
    DEFAULT_SEED, Input, Snapshot, StateKey, Vm, VmConfig, VmError, VmErrorKind, Yield,
};

/// The default of [`ExploreConfig::max_states`].
pub const DEFAULT_MAX_STATES: usize = 100_000;

/// Settings of [`explore`].
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ExploreConfig {
    /// The settings of the VM (start label, limits, ...). `seed` is not used (see `seeds`); `persistent`, `seen_points` and
    /// the text table are not used either. The history is not kept (`history_limit` is 0).
    pub vm: VmConfig,
    /// The seeds of the random generator to explore with. Empty means the default seed only.
    pub seeds: Vec<u64>,
    /// The maximum number of states to examine (across all seeds).
    pub max_states: usize,
}

impl Default for ExploreConfig {
    fn default() -> Self {
        Self {
            vm: VmConfig::default(),
            seeds: vec![DEFAULT_SEED],
            max_states: DEFAULT_MAX_STATES,
        }
    }
}

/// One answer on a path.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ExploreStep {
    /// The name of the stop point (`module.label#id`, or `module.label:line` without an ID).
    pub stop: String,
    /// The name of the command it stopped at.
    pub command: String,
    /// The kind of the stop point.
    pub kind: StopKind,
    /// The answer passed (for a choice, the number in the list given to the host, from 0).
    pub answer: Value,
    /// For a choice, the stable ID of the option chosen (if it has one).
    pub option_id: Option<String>,
}

/// A path that reached the end.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct TestPath {
    /// The name (usable as a file name).
    pub name: String,
    /// The seed.
    pub seed: u64,
    /// The answers from the start.
    pub steps: Vec<ExploreStep>,
}

/// A path that ended with a runtime error.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorPath {
    /// The seed.
    pub seed: u64,
    /// The answers from the start (the last one is the answer after which the error happened).
    pub steps: Vec<ExploreStep>,
    /// The error.
    pub error: VmError,
}

/// A path that stopped at a stop point that could not be answered.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct StuckPath {
    /// The seed.
    pub seed: u64,
    /// The answers from the start, up to the stop point.
    pub steps: Vec<ExploreStep>,
    /// The name of the stop point.
    pub stop: String,
    /// The name of the command.
    pub command: String,
}

/// The result of [`explore`].
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct ExploreReport {
    /// Paths that reached the end.
    pub paths: Vec<TestPath>,
    /// Paths that ended with a runtime error.
    pub errors: Vec<ErrorPath>,
    /// Paths that stopped at a stop point that could not be answered.
    pub unanswered: Vec<StuckPath>,
    /// Stop points never reached.
    pub unreached_stops: Vec<String>,
    /// Options never chosen.
    pub unchosen_options: Vec<String>,
    /// Labels never entered (`module.label`).
    pub unreached_labels: Vec<String>,
    /// The number of states examined.
    pub states: usize,
    /// Whether the search stopped at `max_states`.
    pub truncated: bool,
}

/// Explores the branches of a script: tries the answers of every stop point and reports the paths to the end, the paths
/// that end with an error, and the stop points that could not be answered (`spec/host/08-test.md`).
///
/// `answers` gives the candidates for a stop point whose answer the core cannot decide (a choice and a command that returns
/// `()` are decided by the core). `None` or an empty list means it cannot be answered.
///
/// Fails if a VM cannot be made (no start label, ...), if a state cannot be turned into bytes, or if the VM was interrupted
/// ([`VmConfig::interrupt`]).
pub fn explore(
    program: &Program,
    registry: &Registry,
    config: &ExploreConfig,
    answers: &mut dyn FnMut(&Yield) -> Option<Vec<Value>>,
) -> Result<ExploreReport, VmError> {
    let mut ex = Explorer::new(program, registry, config, answers);
    let default_seeds = [DEFAULT_SEED];
    let seeds: &[u64] = if config.seeds.is_empty() {
        &default_seeds
    } else {
        &config.seeds
    };
    for &seed in seeds {
        if ex.truncated {
            break;
        }
        ex.explore_seed(seed, seeds.len() > 1)?;
    }
    Ok(ex.finish())
}

/// Index of the virtual node of the end.
const END: usize = 0;

struct Edge {
    from: usize,
    to: usize,
    step: ExploreStep,
    /// The part of the path name made by a choice.
    name_part: Option<String>,
}

#[derive(Default)]
struct Node {
    /// The edge first used to reach this node.
    parent: Option<usize>,
    /// The edge of the shortest way to the end (the first one in the order of the candidates among those as short).
    next: Option<usize>,
}

struct Frame {
    node: usize,
    snap: Snapshot,
    persistent: Vec<u8>,
    label: usize,
    ordinal: u32,
    name: String,
    command: String,
    kind: StopKind,
    candidates: Vec<Value>,
    /// For a choice, the numbers in the order written of the options in the list.
    origins: Vec<u32>,
}

struct Explorer<'p, 'f> {
    program: &'p Program,
    registry: &'p Registry,
    config: &'p ExploreConfig,
    answers: &'f mut dyn FnMut(&Yield) -> Option<Vec<Value>>,
    states: usize,
    truncated: bool,
    reached: BTreeSet<(usize, u32)>,
    chosen: BTreeSet<(usize, u32, usize)>,
    visited: BTreeSet<u32>,
    paths: Vec<TestPath>,
    errors: Vec<ErrorPath>,
    unanswered: Vec<StuckPath>,
    error_keys: BTreeSet<(String, &'static str)>,
    stuck_keys: BTreeSet<String>,
    path_names: HashMap<String, usize>,
}

/// What a choice adds to a step.
#[derive(Default)]
struct ChoicePart {
    /// The part of the path name.
    name: Option<String>,
    /// The number of the option in the order written.
    option: Option<usize>,
    /// The stable ID of the option.
    id: Option<String>,
}

/// What `run` after `resume` gave.
enum Arrival {
    End,
    Error(VmError),
    Stop(Box<StateKey>),
}

impl<'p, 'f> Explorer<'p, 'f> {
    fn new(
        program: &'p Program,
        registry: &'p Registry,
        config: &'p ExploreConfig,
        answers: &'f mut dyn FnMut(&Yield) -> Option<Vec<Value>>,
    ) -> Self {
        Self {
            program,
            registry,
            config,
            answers,
            states: 0,
            truncated: false,
            reached: BTreeSet::new(),
            chosen: BTreeSet::new(),
            visited: BTreeSet::new(),
            paths: Vec::new(),
            errors: Vec::new(),
            unanswered: Vec::new(),
            error_keys: BTreeSet::new(),
            stuck_keys: BTreeSet::new(),
            path_names: HashMap::new(),
        }
    }

    fn vm_config(&self, seed: u64) -> VmConfig {
        let mut cfg = self.config.vm.clone();
        cfg.seed = seed;
        cfg.persistent = None;
        cfg.seen_points = Vec::new();
        cfg.history_limit = 0;
        cfg
    }

    fn explore_seed(&mut self, seed: u64, multi: bool) -> Result<(), VmError> {
        let mut vm = Vm::new(self.program, self.registry, self.vm_config(seed))?;
        let first = vm.run();
        self.visited.extend(vm.visited_indices().iter().copied());
        let key = match first {
            Err(e) => {
                self.report_error(seed, Vec::new(), e)?;
                return Ok(());
            }
            Ok(Yield::End) => {
                let name = self.unique_name(path_name(&[], seed, multi));
                self.paths.push(TestPath {
                    name,
                    seed,
                    steps: Vec::new(),
                });
                return Ok(());
            }
            Ok(_) => vm.state_key()?,
        };

        let mut edges: Vec<Edge> = Vec::new();
        // Node 0 is the end
        let mut nodes: Vec<Node> = vec![Node::default()];
        let mut index: HashMap<u128, usize> = HashMap::new();
        let mut queue: VecDeque<Frame> = VecDeque::new();
        // The persistent data the VM holds now (`None` when unknown)
        let mut applied = Some(key.persistent.clone());

        let start = self.new_node(&mut nodes, &mut index, &key, None);
        if let Some(frame) = self.open_frame(&vm, start, key.persistent, &nodes, &edges, seed)? {
            queue.push_back(frame);
        }

        'search: while let Some(frame) = queue.pop_front() {
            for (k, candidate) in frame.candidates.iter().enumerate() {
                vm.restore(&frame.snap)?;
                if applied.as_deref() != Some(&frame.persistent[..])
                    && vm.load_persistent(&frame.persistent).is_err()
                {
                    return Err(vm_error(VmErrorKind::SnapshotMismatch));
                }
                let node = frame.node;
                let ChoicePart {
                    name: name_part,
                    option,
                    id: option_id,
                } = self.choice_part(&frame, k);
                let step = ExploreStep {
                    stop: frame.name.clone(),
                    command: frame.command.clone(),
                    kind: frame.kind,
                    answer: candidate.clone(),
                    option_id,
                };
                if let Some(opt) = option {
                    self.chosen.insert((frame.label, frame.ordinal, opt));
                }
                let arrival = match vm.resume(Input::Return(candidate.clone())) {
                    Err(e) => Arrival::Error(e),
                    Ok(()) => {
                        let r = vm.run();
                        self.visited.extend(vm.visited_indices().iter().copied());
                        match r {
                            Err(e) => Arrival::Error(e),
                            Ok(Yield::End) => Arrival::End,
                            Ok(_) => Arrival::Stop(Box::new(vm.state_key()?)),
                        }
                    }
                };
                match arrival {
                    Arrival::Error(e) => {
                        applied = None;
                        let mut steps = prefix_steps(&nodes, &edges, node);
                        steps.push(step);
                        self.report_error(seed, steps, e)?;
                    }
                    Arrival::End => {
                        applied = None;
                        edges.push(Edge {
                            from: node,
                            to: END,
                            step,
                            name_part,
                        });
                    }
                    Arrival::Stop(key) => {
                        applied = Some(key.persistent.clone());
                        if let Some(&t) = index.get(&hash_key(&key)) {
                            edges.push(Edge {
                                from: node,
                                to: t,
                                step,
                                name_part,
                            });
                            continue;
                        }
                        if self.states >= self.config.max_states {
                            self.truncated = true;
                            break 'search;
                        }
                        let e = edges.len();
                        edges.push(Edge {
                            from: node,
                            to: usize::MAX,
                            step,
                            name_part,
                        });
                        let t = self.new_node(&mut nodes, &mut index, &key, Some(e));
                        edges[e].to = t;
                        if let Some(f) =
                            self.open_frame(&vm, t, key.persistent, &nodes, &edges, seed)?
                        {
                            queue.push_back(f);
                        }
                    }
                }
            }
        }

        link_to_end(&mut nodes, &edges);
        self.collect_paths(&nodes, &edges, seed, multi);
        Ok(())
    }

    fn new_node(
        &mut self,
        nodes: &mut Vec<Node>,
        index: &mut HashMap<u128, usize>,
        key: &StateKey,
        parent: Option<usize>,
    ) -> usize {
        self.states += 1;
        let id = nodes.len();
        nodes.push(Node { parent, next: None });
        index.insert(hash_key(key), id);
        id
    }

    /// Makes the frame of a node the VM has just arrived at. `None` when the stop point cannot be answered (reported).
    fn open_frame(
        &mut self,
        vm: &Vm<'_>,
        node: usize,
        persistent: Vec<u8>,
        nodes: &[Node],
        edges: &[Edge],
        seed: u64,
    ) -> Result<Option<Frame>, VmError> {
        let (label, ordinal) = vm
            .stop_position()
            .expect("the VM is stopped at a stop point");
        let Some(Yield::Stop { command, .. }) = vm.current().cloned() else {
            return Ok(None);
        };
        let stop = &self.program.labels[label].stops[ordinal as usize];
        let name = stop_name(self.program, label, stop);
        let command_name = self.registry.command(command).name.clone();
        self.reached.insert((label, ordinal));
        let current = vm.current().cloned().expect("stopped");
        let candidates = if stop.choice.is_some() {
            (0..vm.choice_origins().len() as i64)
                .map(Value::Int)
                .collect()
        } else if self.registry.command(command).ret == Ty::Unit {
            vec![Value::Unit]
        } else {
            let mut list: Vec<Value> = Vec::new();
            for v in (self.answers)(&current).unwrap_or_default() {
                if !list.contains(&v) {
                    list.push(v);
                }
            }
            list
        };
        if candidates.is_empty() {
            if self.stuck_keys.insert(name.clone()) {
                self.unanswered.push(StuckPath {
                    seed,
                    steps: prefix_steps(nodes, edges, node),
                    stop: name,
                    command: command_name,
                });
            }
            return Ok(None);
        }
        Ok(Some(Frame {
            node,
            snap: vm.snapshot()?,
            persistent,
            label,
            ordinal,
            name,
            command: command_name,
            kind: stop.kind,
            candidates,
            origins: vm.choice_origins().to_vec(),
        }))
    }

    /// For a choice, the part of the path name for candidate `k` and the number of the option in the order written.
    fn choice_part(&self, frame: &Frame, k: usize) -> ChoicePart {
        let stop = &self.program.labels[frame.label].stops[frame.ordinal as usize];
        let Some(info) = &stop.choice else {
            return ChoicePart::default();
        };
        let orig = frame.origins.get(k).copied().unwrap_or(k as u32) as usize;
        let id = info.options.get(orig).and_then(|o| o.id.clone());
        let name = match &id {
            Some(id) => sanitize(id),
            None => format!("c{}", k + 1),
        };
        ChoicePart {
            name: Some(name),
            option: Some(orig),
            id,
        }
    }

    fn report_error(
        &mut self,
        seed: u64,
        steps: Vec<ExploreStep>,
        error: VmError,
    ) -> Result<(), VmError> {
        if matches!(error.kind, VmErrorKind::Interrupted) {
            return Err(error);
        }
        let from = steps
            .last()
            .map(|s| s.stop.clone())
            .unwrap_or_else(|| "(start)".to_string());
        if self.error_keys.insert((from, error.kind.code())) {
            self.errors.push(ErrorPath { seed, steps, error });
        }
        Ok(())
    }

    fn collect_paths(&mut self, nodes: &[Node], edges: &[Edge], seed: u64, multi: bool) {
        // The latest edges first: a path to a deep edge covers all the edges before it on the way
        let mut covered = vec![false; edges.len()];
        let mut chosen: Vec<Vec<usize>> = Vec::new();
        for e in (0..edges.len()).rev() {
            if covered[e] || !completes(nodes, edges[e].to) {
                continue;
            }
            let mut ids: Vec<usize> = prefix_edges(nodes, edges, edges[e].from);
            ids.push(e);
            let mut at = edges[e].to;
            while at != END {
                let n = nodes[at].next.expect("a node that completes has an edge");
                ids.push(n);
                at = edges[n].to;
            }
            for &i in &ids {
                covered[i] = true;
            }
            chosen.push(ids);
        }
        // Back to the order the edges were made in
        for ids in chosen.into_iter().rev() {
            let parts: Vec<&str> = ids
                .iter()
                .filter_map(|&i| edges[i].name_part.as_deref())
                .collect();
            let name = self.unique_name(path_name(&parts, seed, multi));
            self.paths.push(TestPath {
                name,
                seed,
                steps: ids.iter().map(|&i| edges[i].step.clone()).collect(),
            });
        }
    }

    fn unique_name(&mut self, name: String) -> String {
        let count = self.path_names.entry(name.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            name
        } else {
            format!("{name}~{count}")
        }
    }

    fn finish(self) -> ExploreReport {
        let program = self.program;
        let mut unreached_stops = Vec::new();
        let mut unchosen_options = Vec::new();
        let mut unreached_labels = Vec::new();
        for (l, label) in program.labels.iter().enumerate() {
            if !self.visited.contains(&(l as u32)) {
                unreached_labels.push(format!(
                    "{}.{}",
                    program.modules[label.module as usize], label.name
                ));
            }
            for (o, stop) in label.stops.iter().enumerate() {
                if !self.reached.contains(&(l, o as u32)) {
                    unreached_stops.push(stop_name(program, l, stop));
                }
                if let Some(info) = &stop.choice {
                    for (i, opt) in info.options.iter().enumerate() {
                        if !self.chosen.contains(&(l, o as u32, i)) {
                            unchosen_options.push(option_name(
                                program,
                                l,
                                opt.id.as_deref(),
                                opt.line,
                            ));
                        }
                    }
                }
            }
        }
        ExploreReport {
            paths: self.paths,
            errors: self.errors,
            unanswered: self.unanswered,
            unreached_stops,
            unchosen_options,
            unreached_labels,
            states: self.states,
            truncated: self.truncated,
        }
    }
}

fn vm_error(kind: VmErrorKind) -> VmError {
    VmError {
        kind,
        module: None,
        label: None,
        line: None,
    }
}

fn completes(nodes: &[Node], node: usize) -> bool {
    node == END || nodes[node].next.is_some()
}

/// Sets `Node::next` of every node that can reach the end: the edge of a shortest way, and among the shortest the one made
/// first (the first of the candidates).
fn link_to_end(nodes: &mut [Node], edges: &[Edge]) {
    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (e, edge) in edges.iter().enumerate() {
        incoming[edge.to].push(e);
    }
    let mut dist: Vec<Option<usize>> = vec![None; nodes.len()];
    dist[END] = Some(0);
    let mut queue = VecDeque::from([END]);
    while let Some(t) = queue.pop_front() {
        let d = dist[t].expect("queued nodes have a distance");
        for &e in &incoming[t] {
            let f = edges[e].from;
            if dist[f].is_none() {
                dist[f] = Some(d + 1);
                queue.push_back(f);
            }
        }
    }
    for (e, edge) in edges.iter().enumerate() {
        if nodes[edge.from].next.is_none()
            && let (Some(df), Some(dt)) = (dist[edge.from], dist[edge.to])
            && df == dt + 1
        {
            nodes[edge.from].next = Some(e);
        }
    }
}

/// The edges from the start to `node` along the first discovery.
fn prefix_edges(nodes: &[Node], edges: &[Edge], node: usize) -> Vec<usize> {
    let mut ids = Vec::new();
    let mut at = node;
    while let Some(e) = nodes[at].parent {
        ids.push(e);
        at = edges[e].from;
    }
    ids.reverse();
    ids
}

fn prefix_steps(nodes: &[Node], edges: &[Edge], node: usize) -> Vec<ExploreStep> {
    prefix_edges(nodes, edges, node)
        .into_iter()
        .map(|e| edges[e].step.clone())
        .collect()
}

fn hash_key(key: &StateKey) -> u128 {
    let mut a: u64 = 0xcbf2_9ce4_8422_2325;
    let mut b: u64 = 0x8422_2325_cbf2_9ce4 ^ 0x9e37_79b9_7f4a_7c15;
    for chunk in [&key.save, &key.persistent] {
        for &byte in chunk.iter() {
            a = (a ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
            b = (b.rotate_left(5) ^ u64::from(byte)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        }
        // Separate the two parts
        a = (a ^ 0xff).wrapping_mul(0x0000_0100_0000_01b3);
        b = (b.rotate_left(5) ^ 0xff).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        a ^= chunk.len() as u64;
        b ^= (chunk.len() as u64).rotate_left(32);
    }
    (u128::from(a) << 64) | u128::from(b)
}

/// The name of a stop point: `module.label#id`, or `module.label:line` without an ID.
fn stop_name(program: &Program, label: usize, stop: &StopInfo) -> String {
    option_name(program, label, stop.id.as_deref(), stop.line)
}

fn option_name(program: &Program, label: usize, id: Option<&str>, line: u32) -> String {
    let code = &program.labels[label];
    let base = format!("{}.{}", program.modules[code.module as usize], code.name);
    match id {
        Some(id) => format!("{base}#{id}"),
        None => format!("{base}:{line}"),
    }
}

/// Keeps only the characters a file name can have (`a-z`, `0-9`, `_`, `-`).
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn fnv1a64(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The name of a path (`spec/host/08-test.md`, 8.3), before dealing with names that overlap.
fn path_name(parts: &[&str], seed: u64, multi: bool) -> String {
    let mut name = if parts.is_empty() {
        "main".to_string()
    } else {
        parts.join("-")
    };
    if multi {
        name = format!("s{seed}-{name}");
    }
    if name.chars().count() > 200 {
        let head: String = name.chars().take(180).collect();
        name = format!("{head}~{:016x}", fnv1a64(&name));
    }
    name
}

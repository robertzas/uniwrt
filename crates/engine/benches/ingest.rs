//! The telemetry ingest (docs/dev/TESTING.md, Performance; D-149): what the engine does with its
//! nodes' collect answers, a step at a time and as one whole tick, on the simulated fleets of the
//! runtime budgets (10 access points with 200 clients, and 50 with 1,000).
//!
//! The sim runs on tokio's paused clock until its clients have settled, then each node answers
//! `collect` with every set twice, a base interval (15 s) apart. Each step starts from the state
//! the first round left and takes the second, as the node actors (`actor/telemetry.rs`), the
//! topology's keeper (`topology.rs`) and the metrics recorder (`metrics.rs`) do: `parse` the
//! sets into each node's facts; build the `topology` against the previous one and watch its
//! links; update the `clients`' book; take the `metrics` samples and drain them; `tick` is all
//! four in a row.
//!
//! `cargo bench -p uniwrt-engine` measures; `just profile bench ingest` writes flamegraphs.

// A bench may panic on what it sets up.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{hint::black_box, time::Duration};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use uniwrt_clients::{Book, BookConfig, Changes};
use uniwrt_core::{Clock as _, ManualClock, NodeId, Timestamp};
use uniwrt_metrics::{Sample, Sampler};
use uniwrt_model::{facts::NodeFacts, status::NodeState};
use uniwrt_node::{
    NodeLink as _, facts,
    proto::ops::{Collect, Collected},
};
use uniwrt_sim::{SimConfig, SimFleet, SimNode};
use uniwrt_topo::{LinkWatch, Member, TopoKind, Topology};

/// The fleets of the runtime budgets (D-149): access points and clients.
const FLEETS: [(usize, usize); 2] = [(10, 200), (50, 1_000)];
/// How long the world runs before the first round: the clients associated, their leases taken.
const SETTLE: Duration = Duration::from_secs(60);
/// The base interval between the two rounds (D-037).
const TICK: Duration = Duration::from_secs(15);
/// When the samples are drained: past the second round's window.
const DRAIN_AFTER: Duration = Duration::from_secs(600);

/// The two rounds of one fleet's collect answers, and the state after the first.
struct Fleet {
    nodes: Vec<SimNode>,
    second: Vec<Collected>,
    drain_at: Timestamp,
    /// Each node's facts after the first round.
    facts: Vec<NodeFacts>,
    /// The topology built from the first round.
    topology: Topology,
}

impl Fleet {
    fn capture(access_points: usize, clients: usize) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .unwrap();
        let clock = ManualClock::new(Timestamp::from_unix_secs(1_795_000_000));
        let (nodes, first, second) = runtime.block_on(async {
            let config = SimConfig {
                access_points,
                clients,
                reboot_every: None,
                ..SimConfig::default()
            };
            let sim = SimFleet::spawn(&config, &clock);
            run(&clock, SETTLE).await;
            let first = collect(&sim).await;
            run(&clock, TICK).await;
            let second = collect(&sim).await;
            (sim.nodes(), first, second)
        });
        let facts: Vec<NodeFacts> = first
            .iter()
            .map(|collected| {
                let mut facts = NodeFacts::default();
                assert_eq!(facts::apply(collected, &mut facts), []);
                facts
            })
            .collect();
        let topology = uniwrt_topo::build(&members(&nodes, &facts), None);
        let placed = topology
            .nodes
            .iter()
            .filter(|node| node.kind == TopoKind::Client)
            .count();
        assert!(
            placed * 2 >= clients,
            "{placed} of {clients} clients placed"
        );
        let drain_at = Timestamp::from_unix_ms(
            clock.now().as_unix_ms() + i64::try_from(DRAIN_AFTER.as_millis()).unwrap(),
        );
        Self {
            nodes,
            second,
            drain_at,
            facts,
            topology,
        }
    }

    /// The facts after the second round.
    fn parse(&self, mut facts: Vec<NodeFacts>) -> Vec<NodeFacts> {
        for (facts, collected) in facts.iter_mut().zip(&self.second) {
            facts::apply(collected, facts);
        }
        facts
    }

    /// The link watch as the first build left it.
    fn watch(&self) -> LinkWatch {
        let mut watch = LinkWatch::default();
        watch.observe(&mut self.topology.clone());
        watch
    }

    /// The book as the first build left it.
    fn book(&self) -> Book {
        let mut book = Book::new(BookConfig::default());
        book.update(&self.topology, &members(&self.nodes, &self.facts));
        book
    }

    fn build(&self, facts: &[NodeFacts], watch: &mut LinkWatch) -> Topology {
        let mut topology = uniwrt_topo::build(&members(&self.nodes, facts), Some(&self.topology));
        watch.observe(&mut topology);
        topology
    }

    fn sample(
        &self,
        mut sampler: Sampler,
        facts: &[NodeFacts],
        topology: &Topology,
        changes: &Changes,
    ) -> Vec<Sample> {
        for (index, facts) in facts.iter().enumerate() {
            sampler.facts(id(index), facts, true);
        }
        sampler.topology(topology);
        sampler.clients(&changes.samples);
        sampler.drain(self.drain_at)
    }
}

/// Runs the world for `time`, a second at a time, with the clock it stamps its facts by.
async fn run(clock: &ManualClock, time: Duration) {
    for _ in 0..time.as_secs() {
        clock.advance(Duration::from_secs(1));
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Every node's answer to `collect` with every set.
async fn collect(sim: &SimFleet) -> Vec<Collected> {
    let mut answers = Vec::new();
    for index in 0..sim.nodes().len() {
        let link = sim.link(index).expect("a link per node");
        let answer = link
            .request(&Collect::all(), Duration::from_secs(10))
            .await
            .expect("the sim answers collect");
        answers.push(answer);
    }
    answers
}

fn id(index: usize) -> NodeId {
    NodeId::new(i64::try_from(index).unwrap() + 1)
}

fn members<'a>(nodes: &[SimNode], facts: &'a [NodeFacts]) -> Vec<Member<'a>> {
    nodes
        .iter()
        .zip(facts)
        .enumerate()
        .map(|(index, (node, facts))| Member {
            id: id(index),
            role: node.role,
            mac: node.mac,
            state: NodeState::Online,
            facts: Some(facts),
        })
        .collect()
}

fn ingest(c: &mut Criterion) {
    for (access_points, clients) in FLEETS {
        let fleet = Fleet::capture(access_points, clients);
        let after = fleet.parse(fleet.facts.clone());
        let topology = fleet.build(&after, &mut fleet.watch());
        let changes = fleet
            .book()
            .update(&topology, &members(&fleet.nodes, &after));
        let mut group = c.benchmark_group(format!("ingest_{access_points}x{clients}"));
        group.throughput(Throughput::Elements(fleet.nodes.len() as u64));
        group.bench_function("parse", |b| {
            b.iter_batched(
                || fleet.facts.clone(),
                |facts| fleet.parse(facts),
                BatchSize::LargeInput,
            );
        });
        group.bench_function("topology", |b| {
            b.iter_batched(
                || fleet.watch(),
                |mut watch| fleet.build(black_box(&after), &mut watch),
                BatchSize::LargeInput,
            );
        });
        group.bench_function("clients", |b| {
            b.iter_batched(
                || fleet.book(),
                |mut book| book.update(black_box(&topology), &members(&fleet.nodes, &after)),
                BatchSize::LargeInput,
            );
        });
        group.bench_function("metrics", |b| {
            b.iter_batched(
                Sampler::new,
                |sampler| fleet.sample(sampler, black_box(&after), &topology, &changes),
                BatchSize::LargeInput,
            );
        });
        group.bench_function("tick", |b| {
            b.iter_batched(
                || {
                    (
                        fleet.facts.clone(),
                        fleet.watch(),
                        fleet.book(),
                        Sampler::new(),
                    )
                },
                |(facts, mut watch, mut book, sampler)| {
                    let facts = fleet.parse(facts);
                    let topology = fleet.build(&facts, &mut watch);
                    let changes = book.update(&topology, &members(&fleet.nodes, &facts));
                    fleet.sample(sampler, &facts, &topology, &changes)
                },
                BatchSize::LargeInput,
            );
        });
        group.finish();
    }
}

criterion_group! {
    name = benches;
    config = uniwrt_bench::criterion();
    targets = ingest
}
criterion_main!(benches);

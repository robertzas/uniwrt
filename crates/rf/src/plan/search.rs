//! The search over units' choices (features/wifi.md, RF planner: Search): a local descent from
//! the current channels, then branch and bound over every combination where the band is small
//! enough to finish, else simulated annealing with a fixed seed and a last descent. Ties keep
//! the earlier answer, which starts from what runs, so equal plans don't move radios.

use super::output::Search;
use crate::span::Span;

/// Branch and bound runs on bands of at most this many units, and gives up past this many
/// nodes (the band falls to annealing).
const EXHAUSTIVE_UNITS: usize = 10;
const NODE_BUDGET: u64 = 500_000;
/// Annealing's steps per unit, and its cap.
const STEPS_PER_UNIT: usize = 4_000;
const STEPS_CAP: usize = 200_000;
/// Annealing's temperatures, from a few loud co-channel networks to nearly nothing.
const T_START: f32 = 3.0;
const T_END: f32 = 0.001;
/// The fixed seed: the same inputs plan the same.
const SEED: u64 = 0x5EED_0FA1;
/// Annealing runs this many rounds, each from the best so far with a seed of its own.
const ROUNDS: u64 = 3;
/// Smaller differences are ties.
const EPSILON: f32 = 1e-6;

/// The space: each unit's choices' spans and unary costs, and `pair[u][v]`, how much unit `v`
/// disturbs unit `u` per share of overlap.
pub(crate) struct Space<'a> {
    pub spans: Vec<Vec<Span>>,
    pub unary: &'a [Vec<f32>],
    pub pair: &'a [Vec<f32>],
}

impl Space<'_> {
    fn between(&self, u: usize, cu: usize, v: usize, cv: usize) -> f32 {
        let (a, b) = (self.spans[u][cu], self.spans[v][cv]);
        self.pair[u][v] * a.overlap(b) + self.pair[v][u] * b.overlap(a)
    }

    /// Unit `u`'s share on choice `c` against the others' choices.
    fn local(&self, assign: &[usize], u: usize, c: usize) -> f32 {
        self.unary[u][c]
            + (0..assign.len())
                .filter(|v| *v != u)
                .map(|v| self.between(u, c, v, assign[v]))
                .sum::<f32>()
    }

    pub(crate) fn total(&self, assign: &[usize]) -> f32 {
        let mut sum = 0.0;
        for u in 0..assign.len() {
            sum += self.unary[u][assign[u]];
            for v in u + 1..assign.len() {
                sum += self.between(u, assign[u], v, assign[v]);
            }
        }
        sum
    }

    /// Moves one unit at a time to its best choice until none improves.
    fn descend(&self, assign: &mut [usize]) {
        loop {
            let mut moved = false;
            for u in 0..assign.len() {
                let mut best = (self.local(assign, u, assign[u]), assign[u]);
                for c in 0..self.unary[u].len() {
                    let cost = self.local(assign, u, c);
                    if cost < best.0 - EPSILON {
                        best = (cost, c);
                    }
                }
                if best.1 != assign[u] {
                    assign[u] = best.1;
                    moved = true;
                }
            }
            if !moved {
                return;
            }
        }
    }
}

/// The best assignment found from `start`, how it was found and whether it is proven best.
pub(crate) fn solve(space: &Space<'_>, start: Vec<usize>) -> (Vec<usize>, Search, bool) {
    if space.unary.iter().all(|u| u.len() <= 1) {
        return (start, Search::Fixed, true);
    }
    let mut best = start;
    space.descend(&mut best);
    let mut bound = Bound {
        space,
        best_cost: space.total(&best),
        best: best.clone(),
        nodes: 0,
        order: (0..space.unary.len()).collect(),
        floor: Vec::new(),
    };
    if space.unary.len() <= EXHAUSTIVE_UNITS && bound.run() {
        return (bound.best, Search::Exhaustive, true);
    }
    let best = bound.best;
    let annealed = rounds(space, best.clone());
    if space.total(&annealed) < space.total(&best) - EPSILON {
        return (annealed, Search::Annealing, false);
    }
    (best, Search::Annealing, false)
}

/// Branch and bound: every term is 0 or more, so a partial cost plus each remaining unit's
/// cheapest unary cost bounds every completion.
struct Bound<'s, 'a> {
    space: &'s Space<'a>,
    best: Vec<usize>,
    best_cost: f32,
    nodes: u64,
    order: Vec<usize>,
    /// The cheapest unary cost of the units from each depth on.
    floor: Vec<f32>,
}

impl Bound<'_, '_> {
    /// Whether it finished within the budget.
    fn run(&mut self) -> bool {
        let unary = self.space.unary;
        // The most constrained units first.
        self.order.sort_by_key(|u| (unary[*u].len(), *u));
        let min = |u: usize| unary[u].iter().copied().fold(f32::INFINITY, f32::min);
        self.floor = vec![0.0; self.order.len() + 1];
        for d in (0..self.order.len()).rev() {
            self.floor[d] = self.floor[d + 1] + min(self.order[d]);
        }
        let mut assign = vec![usize::MAX; self.order.len()];
        self.branch(&mut assign, 0, 0.0)
    }

    fn branch(&mut self, assign: &mut [usize], depth: usize, cost: f32) -> bool {
        self.nodes += 1;
        if self.nodes > NODE_BUDGET {
            return false;
        }
        if depth == self.order.len() {
            if cost < self.best_cost - EPSILON {
                self.best_cost = cost;
                self.best = assign.to_vec();
            }
            return true;
        }
        let u = self.order[depth];
        // Cheapest first, so good plans come early and prune more.
        let mut choices: Vec<(f32, usize)> = (0..self.space.unary[u].len())
            .map(|c| {
                let with = self.space.unary[u][c]
                    + self.order[..depth]
                        .iter()
                        .map(|v| self.space.between(u, c, *v, assign[*v]))
                        .sum::<f32>();
                (with, c)
            })
            .collect();
        choices.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (with, c) in choices {
            let next = cost + with;
            if next + self.floor[depth + 1] >= self.best_cost - EPSILON {
                break;
            }
            assign[u] = c;
            if !self.branch(assign, depth + 1, next) {
                return false;
            }
        }
        assign[u] = usize::MAX;
        true
    }
}

/// Annealing's rounds, each followed by a descent.
fn rounds(space: &Space<'_>, start: Vec<usize>) -> Vec<usize> {
    let mut best = start;
    for round in 0..ROUNDS {
        let mut next = anneal(space, best.clone(), SEED.wrapping_add(round));
        space.descend(&mut next);
        if space.total(&next) < space.total(&best) - EPSILON {
            best = next;
        }
    }
    best
}

/// Simulated annealing: random single-unit moves, taken when they help or, with a chance that
/// shrinks as it cools, when they don't.
fn anneal(space: &Space<'_>, start: Vec<usize>, seed: u64) -> Vec<usize> {
    let units = space.unary.len();
    let steps = (STEPS_PER_UNIT * units).min(STEPS_CAP);
    let mut rng = SplitMix(seed);
    let mut assign = start;
    let mut cost = space.total(&assign);
    let mut best = (cost, assign.clone());
    #[allow(clippy::cast_precision_loss)]
    let cooling = (T_END / T_START).powf(1.0 / steps as f32);
    let mut t = T_START;
    for _ in 0..steps {
        t *= cooling;
        let u = rng.below(units);
        let n = space.unary[u].len();
        if n < 2 {
            continue;
        }
        let c = rng.below(n);
        if c == assign[u] {
            continue;
        }
        let delta = space.local(&assign, u, c) - space.local(&assign, u, assign[u]);
        if delta < 0.0 || rng.unit() < (-delta / t).exp() {
            assign[u] = c;
            cost += delta;
            if cost < best.0 - EPSILON {
                best = (cost, assign.clone());
            }
        }
    }
    best.1
}

/// `SplitMix64`: small, fast and the same everywhere.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    #[allow(clippy::cast_possible_truncation)]
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    #[allow(clippy::cast_precision_loss)]
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[cfg(test)]
pub(crate) fn exhaustive_or_none(space: &Space<'_>, start: Vec<usize>) -> Option<Vec<usize>> {
    let mut bound = Bound {
        space,
        best_cost: space.total(&start),
        best: start,
        nodes: 0,
        order: (0..space.unary.len()).collect(),
        floor: Vec::new(),
    };
    bound.run().then_some(bound.best)
}

#[cfg(test)]
pub(crate) fn annealed(space: &Space<'_>, start: Vec<usize>) -> Vec<usize> {
    rounds(space, start)
}

//! Pairing the old package's sections with the new one's, and finding the ones that moved.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::{DefaultHasher, Hash, Hasher},
};

use super::same;
use crate::ast::Section;

/// The most cells the table of equal sections may have (4 MiB); larger runs pair greedily.
const MAX_CELLS: usize = 1 << 20;
/// The most cells the table of similar sections may have; larger gaps pair in order.
const MAX_SIMILAR: usize = 1 << 16;

/// An old section (`a`), a new one (`b`) or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Pair {
    pub(super) a: Option<usize>,
    pub(super) b: Option<usize>,
    pub(super) moved: bool,
}

/// Every section once, in the new order, each removed one where it was.
pub(super) fn pair(a: &[Section], b: &[Section]) -> Vec<Pair> {
    let mut of_b: Vec<Option<usize>> = vec![None; b.len()];
    let named: HashMap<&str, usize> = a
        .iter()
        .enumerate()
        .filter_map(|(i, s)| Some((s.name.as_deref()?, i)))
        .collect();
    for (j, s) in b.iter().enumerate() {
        of_b[j] = s.name.as_deref().and_then(|name| named.get(name).copied());
    }
    let mut kinds: Vec<&str> = Vec::new();
    for s in b.iter().filter(|s| s.name.is_none()) {
        if !kinds.contains(&s.kind.as_str()) {
            kinds.push(&s.kind);
        }
    }
    for kind in kinds {
        let anonymous = |sections: &[Section]| -> Vec<usize> {
            (0..sections.len())
                .filter(|&i| sections[i].name.is_none() && sections[i].kind == kind)
                .collect()
        };
        let (xs, ys) = (anonymous(a), anonymous(b));
        for (x, y) in sequence(a, b, &xs, &ys) {
            of_b[y] = Some(x);
        }
    }
    let moved = moves(a, b, &of_b);

    let mut of_a: Vec<Option<usize>> = vec![None; a.len()];
    for (j, i) in of_b.iter().enumerate() {
        if let Some(i) = *i {
            of_a[i] = Some(j);
        }
    }
    let mut keyed: Vec<((usize, u8, usize), Pair)> = (0..b.len())
        .map(|j| {
            (
                (j, 1, 0),
                Pair {
                    a: of_b[j],
                    b: Some(j),
                    moved: moved[j],
                },
            )
        })
        .collect();
    // A removed section comes right after the new place of the old one before it.
    let mut after = 0;
    for (i, j) in of_a.iter().enumerate() {
        match *j {
            Some(j) => after = j + 1,
            None => keyed.push((
                (after, 0, i),
                Pair {
                    a: Some(i),
                    b: None,
                    moved: false,
                },
            )),
        }
    }
    keyed.sort_unstable_by_key(|(key, _)| *key);
    keyed.into_iter().map(|(_, pair)| pair).collect()
}

/// Pairs one type's anonymous sections: `xs` in `a`, `ys` in `b`.
fn sequence(
    before: &[Section],
    after: &[Section],
    xs: &[usize],
    ys: &[usize],
) -> Vec<(usize, usize)> {
    let (fa, fb): (Vec<u64>, Vec<u64>) = (
        xs.iter().map(|&i| fingerprint(&before[i])).collect(),
        ys.iter().map(|&j| fingerprint(&after[j])).collect(),
    );
    let equal = |p: usize, q: usize| fa[p] == fb[q] && same(&before[xs[p]], &after[ys[q]]);
    let (len_a, len_b) = (xs.len(), ys.len());
    let mut lo = 0;
    while lo < len_a && lo < len_b && equal(lo, lo) {
        lo += 1;
    }
    let (mut hn, mut hm) = (len_a, len_b);
    while hn > lo && hm > lo && equal(hn - 1, hm - 1) {
        hn -= 1;
        hm -= 1;
    }
    // Equal sections: the start, the middle's common subsequence, the end.
    let mut anchors: Vec<(usize, usize)> = (0..lo).map(|k| (k, k)).collect();
    anchors.extend(common(lo..hn, lo..hm, &fa, &fb, &equal));
    anchors.extend((0..len_a - hn).map(|k| (hn + k, hm + k)));
    // Between them, similar ones.
    let mut out = Vec::new();
    let (mut p, mut q) = (0, 0);
    for &(ap, aq) in anchors.iter().chain(std::iter::once(&(len_a, len_b))) {
        let gap_a: Vec<usize> = xs[p..ap].to_vec();
        let gap_b: Vec<usize> = ys[q..aq].to_vec();
        out.extend(similar(before, after, &gap_a, &gap_b));
        if ap < len_a {
            out.push((xs[ap], ys[aq]));
        }
        (p, q) = (ap + 1, aq + 1);
    }
    // Equal sections left over moved: pair them in order.
    let (paired_a, paired_b): (HashSet<usize>, HashSet<usize>) = out.iter().copied().unzip();
    let mut left: HashMap<u64, VecDeque<usize>> = HashMap::new();
    for p in (0..len_a).filter(|p| !paired_a.contains(&xs[*p])) {
        left.entry(fa[p]).or_default().push_back(p);
    }
    for q in (0..len_b).filter(|q| !paired_b.contains(&ys[*q])) {
        let Some(ps) = left.get_mut(&fb[q]) else {
            continue;
        };
        if let Some(p) = ps
            .iter()
            .position(|&p| equal(p, q))
            .and_then(|k| ps.remove(k))
        {
            out.push((xs[p], ys[q]));
        }
    }
    out
}

/// A longest common subsequence of equal sections in `fa[pa]` and `fb[pb]`, or a greedy one
/// when the table would be too large.
fn common(
    pa: std::ops::Range<usize>,
    pb: std::ops::Range<usize>,
    fa: &[u64],
    fb: &[u64],
    equal: &dyn Fn(usize, usize) -> bool,
) -> Vec<(usize, usize)> {
    let (n, m) = (pa.len(), pb.len());
    if n == 0 || m == 0 {
        return Vec::new();
    }
    if (n + 1) * (m + 1) > MAX_CELLS {
        let mut at: HashMap<u64, Vec<usize>> = HashMap::new();
        for p in pa.clone() {
            at.entry(fa[p]).or_default().push(p);
        }
        let mut out = Vec::new();
        let mut from = pa.start;
        for q in pb {
            let Some(ps) = at.get(&fb[q]) else { continue };
            let k = ps.partition_point(|&p| p < from);
            if let Some(&p) = ps.get(k).filter(|&&p| equal(p, q)) {
                out.push((p, q));
                from = p + 1;
            }
        }
        return out;
    }
    let width = m + 1;
    let mut len = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            len[i * width + j] = if equal(pa.start + i, pb.start + j) {
                len[(i + 1) * width + j + 1] + 1
            } else {
                len[(i + 1) * width + j].max(len[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < n && j < m {
        if equal(pa.start + i, pb.start + j) {
            out.push((pa.start + i, pb.start + j));
            (i, j) = (i + 1, j + 1);
        } else if len[(i + 1) * width + j] >= len[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// Pairs sections in order to maximize how alike the pairs are, each at least half alike.
fn similar(
    before: &[Section],
    after: &[Section],
    xs: &[usize],
    ys: &[usize],
) -> Vec<(usize, usize)> {
    let (len_a, len_b) = (xs.len(), ys.len());
    if len_a == 0 || len_b == 0 {
        return Vec::new();
    }
    if (len_a + 1) * (len_b + 1) > MAX_SIMILAR {
        return xs
            .iter()
            .zip(ys)
            .filter(|&(&x, &y)| alike(&before[x], &after[y]).is_some())
            .map(|(&x, &y)| (x, y))
            .collect();
    }
    let width = len_b + 1;
    let score: Vec<Option<u64>> = (0..len_a * len_b)
        .map(|k| alike(&before[xs[k / len_b]], &after[ys[k % len_b]]))
        .collect();
    let mut best = vec![0u64; (len_a + 1) * width];
    for i in (0..len_a).rev() {
        for j in (0..len_b).rev() {
            let take = score[i * len_b + j].map_or(0, |s| s + best[(i + 1) * width + j + 1]);
            best[i * width + j] = take
                .max(best[(i + 1) * width + j])
                .max(best[i * width + j + 1]);
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < len_a && j < len_b {
        let here = best[i * width + j];
        if here == 0 {
            break;
        }
        if score[i * len_b + j].is_some_and(|s| s + best[(i + 1) * width + j + 1] == here) {
            out.push((xs[i], ys[j]));
            (i, j) = (i + 1, j + 1);
        } else if best[(i + 1) * width + j] == here {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// How alike two sections are, in half points (an equal option 2, an option only both have 1),
/// when that is at least half of their options; the score is at least 1.
fn alike(x: &Section, y: &Section) -> Option<u64> {
    let total = (x.options.len() + y.options.len()) as u64;
    let score: u64 = x
        .options
        .iter()
        .filter_map(|e| y.get(&e.name).map(|v| if *v == e.value { 2 } else { 1 }))
        .sum();
    (score > 0 && 2 * score >= total).then_some(score)
}

/// A hash of a section's type, name and options in name order.
fn fingerprint(section: &Section) -> u64 {
    let mut options: Vec<_> = section.options.iter().collect();
    options.sort_unstable_by(|x, y| x.name.cmp(&y.name));
    let mut hasher = DefaultHasher::new();
    (&section.kind, &section.name).hash(&mut hasher);
    for entry in options {
        (&entry.name, &entry.value).hash(&mut hasher);
    }
    hasher.finish()
}

/// Per new section, whether it left its place among its type's: pairs of one type, in the new
/// order, keep their place when their old positions are a longest increasing run.
fn moves(a: &[Section], b: &[Section], of_b: &[Option<usize>]) -> Vec<bool> {
    let mut moved = vec![false; b.len()];
    let mut by_kind: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();
    for (j, i) in of_b.iter().enumerate() {
        if let Some(i) = *i
            && a[i].kind == b[j].kind
        {
            by_kind.entry(&b[j].kind).or_default().push((j, i));
        }
    }
    for pairs in by_kind.values() {
        let kept = increasing(&pairs.iter().map(|&(_, i)| i).collect::<Vec<_>>());
        for (k, &(j, _)) in pairs.iter().enumerate() {
            moved[j] = !kept[k];
        }
    }
    moved
}

/// Marks a longest strictly increasing subsequence (patience sorting).
fn increasing(values: &[usize]) -> Vec<bool> {
    let mut tails: Vec<usize> = Vec::new(); // index into values of each run length's tail
    let mut prev = vec![usize::MAX; values.len()];
    for k in 0..values.len() {
        let len = tails.partition_point(|&t| values[t] < values[k]);
        if len > 0 {
            prev[k] = tails[len - 1];
        }
        if len == tails.len() {
            tails.push(k);
        } else {
            tails[len] = k;
        }
    }
    let mut kept = vec![false; values.len()];
    let mut k = tails.last().copied().unwrap_or(usize::MAX);
    while k != usize::MAX {
        kept[k] = true;
        k = prev[k];
    }
    kept
}

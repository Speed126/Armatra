//! Bounded heuristic refinement of the full unary + hierarchy objective.
//! Global optimality is claimed only for the unary LAP, never for refinement.
use crate::{
    assignment::{self, Assignment, AssignmentProblem, AssignmentSolver, ShortestPath},
    rig::{Anatomy, Rig},
};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
pub const MAX_REFINEMENT: usize = 12;
#[derive(Clone, Debug)]
pub struct Refined {
    pub assignment: Assignment,
    pub energies: Vec<f64>,
    pub iterations: usize,
    pub repeated_state: bool,
    pub chain_candidates: usize,
}
fn index(r: &Rig, node: usize) -> Option<usize> {
    r.joints.iter().position(|d| d.node == node)
}
pub fn ancestor(r: &Rig, a: usize, mut b: usize) -> bool {
    for _ in 0..r.joints.len() {
        let Some(p) = r.joints[b].parent.and_then(|p| index(r, p)) else {
            return false;
        };
        if p == a {
            return true;
        }
        b = p;
    }
    false
}
pub fn violations(s: &Rig, t: &Rig, m: &[Option<usize>]) -> usize {
    m.iter()
        .enumerate()
        .filter(|(i, j)| {
            j.is_some_and(|j| {
                let mut p = s.joints[*i].parent.and_then(|p| index(s, p));
                while let Some(a) = p {
                    if let Some(b) = m[a] {
                        return !ancestor(t, b, j);
                    }
                    p = s.joints[a].parent.and_then(|p| index(s, p));
                }
                false
            })
        })
        .count()
}
pub fn energy(s: &Rig, t: &Rig, p: &AssignmentProblem, m: &[Option<usize>]) -> f64 {
    m.iter()
        .enumerate()
        .map(|(i, j)| j.map_or(p.unmatched[i], |j| p.costs[i][j].unwrap_or(1e12)))
        .sum::<f64>()
        + 0.8 * violations(s, t, m) as f64
}
/// Edit-distance DP with explicit skips. Pairs always increase in both axes.
pub fn align_chains(
    costs: &[Vec<Option<f64>>],
    skip_source: &[f64],
    skip_target: &[f64],
) -> Vec<(usize, usize)> {
    let n = skip_source.len();
    let m = skip_target.len();
    if costs.len() != n || costs.iter().any(|r| r.len() != m) {
        return Vec::new();
    }
    let mut d = vec![vec![0.; m + 1]; n + 1];
    let mut op = vec![vec![0u8; m + 1]; n + 1];
    for i in 1..=n {
        d[i][0] = d[i - 1][0] + skip_source[i - 1];
        op[i][0] = 1;
    }
    for j in 1..=m {
        d[0][j] = d[0][j - 1] + skip_target[j - 1];
        op[0][j] = 2;
    }
    for i in 1..=n {
        for j in 1..=m {
            let candidates = [
                d[i - 1][j - 1] + costs[i - 1][j - 1].unwrap_or(1e12),
                d[i - 1][j] + skip_source[i - 1],
                d[i][j - 1] + skip_target[j - 1],
            ];
            let k = (0..3)
                .min_by(|&a, &b| candidates[a].total_cmp(&candidates[b]))
                .unwrap();
            d[i][j] = candidates[k];
            op[i][j] = k as u8;
        }
    }
    let (mut i, mut j) = (n, m);
    let mut pairs = Vec::new();
    while i > 0 || j > 0 {
        match op[i][j] {
            0 => {
                i -= 1;
                j -= 1;
                if costs[i][j].is_some() {
                    pairs.push((i, j));
                }
            }
            1 => i -= 1,
            _ => j -= 1,
        }
    }
    pairs.reverse();
    pairs
}
fn chain_candidate(
    s: &Rig,
    t: &Rig,
    p: &AssignmentProblem,
    m: &[Option<usize>],
) -> (Vec<Option<usize>>, usize) {
    let mut children = vec![Vec::new(); s.joints.len()];
    for (i, d) in s.joints.iter().enumerate() {
        if let Some(p) = d.parent.and_then(|p| index(s, p)) {
            children[p].push(i);
        }
    }
    let mut candidate = m.to_vec();
    let mut count = 0;
    for first in 0..s.joints.len() {
        if children[first].len() == 1 && s.joints[first].parent.is_some() {
            continue;
        }
        for &child in &children[first] {
            let mut chain = vec![first, child];
            let mut last = child;
            while children[last].len() == 1 {
                last = children[last][0];
                chain.push(last);
            }
            let (Some(a), Some(b)) = (m[first], m[last]) else {
                continue;
            };
            let mut target_chain = vec![b];
            let mut current = b;
            while current != a {
                let Some(parent) = t.joints[current].parent.and_then(|p| index(t, p)) else {
                    break;
                };
                target_chain.push(parent);
                current = parent;
            }
            if current != a || chain.len() < 3 || target_chain.len() < 3 {
                continue;
            }
            target_chain.reverse();
            let src = &chain[1..chain.len() - 1];
            let dst = &target_chain[1..target_chain.len() - 1];
            let outside: HashSet<_> = m
                .iter()
                .enumerate()
                .filter(|(i, _)| !src.contains(i))
                .filter_map(|(_, j)| *j)
                .collect();
            let costs: Vec<Vec<_>> = src
                .iter()
                .map(|&i| {
                    dst.iter()
                        .map(|&j| {
                            if outside.contains(&j) {
                                None
                            } else {
                                p.costs[i][j]
                            }
                        })
                        .collect()
                })
                .collect();
            let ss: Vec<_> = src.iter().map(|&i| p.unmatched[i]).collect();
            let st: Vec<_> = dst
                .iter()
                .map(|&j| {
                    if t.joints[j].anatomy == Anatomy::Helper {
                        0.02
                    } else {
                        0.25
                    }
                })
                .collect();
            for &i in src {
                candidate[i] = None;
            }
            for (i, j) in align_chains(&costs, &ss, &st) {
                candidate[src[i]] = Some(dst[j]);
            }
            count += 1;
        }
    }
    // Different chains must never consume a target twice.
    let mut unique = HashSet::new();
    if candidate.iter().flatten().any(|j| !unique.insert(*j)) {
        (m.to_vec(), count)
    } else {
        (candidate, count)
    }
}
pub fn refine(s: &Rig, t: &Rig) -> Result<Refined> {
    anyhow::ensure!(
        s.joints.len() <= 1024 && t.joints.len() <= 1024,
        "Automatic correspondence is limited to 1024 joints per rig"
    );
    let mut p = assignment::problem(s, t);
    if s.frame.is_none() || t.frame.is_none() {
        p.costs = s
            .joints
            .iter()
            .map(|s| {
                t.joints
                    .iter()
                    .map(|t| assignment::unary(s, t, false))
                    .collect()
            })
            .collect();
    }
    let initial = ShortestPath.solve(&p)?;
    let mut current = initial.targets;
    let mut best = current.clone();
    let mut best_energy = energy(s, t, &p, &best);
    let mut energies = vec![best_energy];
    let mut visited = HashSet::new();
    let mut iterations = 0;
    let mut repeated_state = false;
    let mut chain_candidates = 0;
    let by_node: HashMap<_, _> = s
        .joints
        .iter()
        .enumerate()
        .map(|(i, d)| (d.node, i))
        .collect();
    for _ in 0..MAX_REFINEMENT {
        if !visited.insert(current.clone()) {
            repeated_state = true;
            break;
        }
        iterations += 1;
        let mut adjusted = p.clone();
        for i in 0..s.joints.len() {
            for j in 0..t.joints.len() {
                if let Some(c) = &mut adjusted.costs[i][j] {
                    let mut parent = s.joints[i].parent.and_then(|p| by_node.get(&p).copied());
                    while let Some(a) = parent {
                        if let Some(b) = current[a] {
                            if !ancestor(t, b, j) {
                                *c += 0.8;
                            }
                            break;
                        }
                        parent = s.joints[a].parent.and_then(|p| by_node.get(&p).copied());
                    }
                    for (child, mapped) in current.iter().enumerate() {
                        if s.joints[child].parent == Some(s.joints[i].node)
                            && mapped.is_some_and(|b| !ancestor(t, j, b))
                        {
                            *c += 0.4;
                        }
                    }
                }
            }
        }
        let next = ShortestPath.solve(&adjusted)?.targets;
        let e = energy(s, t, &p, &next);
        energies.push(e);
        if e < best_energy {
            best = next.clone();
            best_energy = e;
        }
        let (chain, count) = chain_candidate(s, t, &p, &next);
        chain_candidates += count;
        let ce = energy(s, t, &p, &chain);
        energies.push(ce);
        if ce < best_energy {
            best = chain;
            best_energy = ce;
        }
        if next == current {
            break;
        }
        current = next;
    }
    Ok(Refined {
        assignment: Assignment {
            targets: best,
            cost: best_energy,
        },
        energies,
        iterations,
        repeated_state,
        chain_candidates,
    })
}

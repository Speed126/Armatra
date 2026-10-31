//! Rectangular shortest augmenting path assignment. Each source row receives a
//! real target or one of n dummy columns with that row's explicit unmatched cost.
use crate::rig::{Anatomy, Descriptor, Rig, Side};
use anyhow::{Result, bail};
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct AssignmentProblem {
    pub costs: Vec<Vec<Option<f64>>>,
    pub unmatched: Vec<f64>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Assignment {
    pub targets: Vec<Option<usize>>,
    pub cost: f64,
}
pub trait AssignmentSolver {
    fn solve(&self, problem: &AssignmentProblem) -> Result<Assignment>;
}
pub struct ShortestPath;
impl AssignmentSolver for ShortestPath {
    fn solve(&self, p: &AssignmentProblem) -> Result<Assignment> {
        let n = p.costs.len();
        let real = p.costs.first().map_or(0, Vec::len);
        let m = real + n;
        if n > 1024
            || real > 1024
            || p.unmatched.len() != n
            || p.costs.iter().any(|r| r.len() != real)
            || p.unmatched.iter().any(|v| !v.is_finite() || v.abs() > 1e8)
            || p.costs
                .iter()
                .flatten()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e8)
        {
            bail!("Invalid or oversized assignment problem")
        }
        let cost = |i: usize, j: usize| {
            if j < real {
                p.costs[i][j].unwrap_or(1e12)
            } else {
                p.unmatched[i]
            }
        };
        let mut u = vec![0.; n + 1];
        let mut v = vec![0.; m + 1];
        let mut row = vec![0usize; m + 1];
        let mut way = vec![0usize; m + 1];
        for i in 1..=n {
            row[0] = i;
            let mut j0 = 0;
            let mut min = vec![f64::INFINITY; m + 1];
            let mut used = vec![false; m + 1];
            loop {
                used[j0] = true;
                let i0 = row[j0];
                let mut delta = f64::INFINITY;
                let mut j1 = 0;
                for j in 1..=m {
                    if !used[j] {
                        let cur = cost(i0 - 1, j - 1) - u[i0] - v[j];
                        if cur < min[j] {
                            min[j] = cur;
                            way[j] = j0;
                        }
                        if min[j] < delta {
                            delta = min[j];
                            j1 = j;
                        }
                    }
                }
                for j in 0..=m {
                    if used[j] {
                        u[row[j]] += delta;
                        v[j] -= delta;
                    } else {
                        min[j] -= delta;
                    }
                }
                j0 = j1;
                if row[j0] == 0 {
                    break;
                }
            }
            loop {
                let j1 = way[j0];
                row[j0] = row[j1];
                j0 = j1;
                if j0 == 0 {
                    break;
                }
            }
        }
        let mut targets = vec![None; n];
        for j in 1..=real {
            if row[j] > 0 {
                targets[row[j] - 1] = Some(j - 1);
            }
        }
        let cost = targets
            .iter()
            .enumerate()
            .map(|(i, j)| j.map_or(p.unmatched[i], |j| p.costs[i][j].unwrap_or(1e12)))
            .sum();
        Ok(Assignment { targets, cost })
    }
}
pub fn unmatched(d: &Descriptor) -> f64 {
    match d.anatomy {
        Anatomy::Helper => 0.18,
        Anatomy::Finger | Anatomy::Toe => 0.9,
        Anatomy::Unknown => 1.1,
        _ => 2.4,
    }
}
pub fn unary(s: &Descriptor, t: &Descriptor, frame: bool) -> Option<f64> {
    use Anatomy::*;
    if matches!(
        (s.side, t.side),
        (Side::Left, Side::Right)
            | (Side::Right, Side::Left)
            | (Side::Center, Side::Left | Side::Right)
            | (Side::Left | Side::Right, Side::Center)
    ) {
        return None;
    }
    let known = s.anatomy != Unknown && t.anatomy != Unknown;
    let compatible =
        s.anatomy == t.anatomy || matches!((s.anatomy, t.anatomy), (Spine, Chest) | (Chest, Spine));
    if known && !compatible {
        return None;
    }
    let name = if s.tokens.is_empty() || t.tokens.is_empty() {
        1.
    } else {
        let a: HashSet<_> = s.tokens.iter().collect();
        let b: HashSet<_> = t.tokens.iter().collect();
        1. - a.intersection(&b).count() as f64 / a.union(&b).count().max(1) as f64
    };
    let geometry = if frame { 1. } else { 0.08 };
    Some(
        0.3 * name
            + if s.anatomy == t.anatomy {
                0.
            } else if known {
                0.25
            } else {
                0.45
            }
            + geometry
                * (2.0 * s.position.distance(t.position) as f64
                    + 0.12 * (1. - s.direction.dot(t.direction).clamp(-1., 1.)) as f64)
            + 0.15 * (s.length - t.length).abs() as f64
            + 0.15 * (s.depth - t.depth).abs() as f64
            + 0.04 * s.children.abs_diff(t.children).min(4) as f64,
    )
}
pub fn problem(s: &Rig, t: &Rig) -> AssignmentProblem {
    AssignmentProblem {
        costs: s
            .joints
            .iter()
            .map(|s| {
                t.joints
                    .iter()
                    .map(|t| unary(s, t, s.rigid && t.rigid))
                    .collect()
            })
            .collect(),
        unmatched: s.joints.iter().map(unmatched).collect(),
    }
}
pub fn weighted(s: &Rig, t: &Rig) -> Result<Assignment> {
    anyhow::ensure!(
        s.joints.len() <= 1024 && t.joints.len() <= 1024,
        "Automatic correspondence is limited to 1024 joints per rig"
    );
    let mut p = problem(s, t);
    if s.frame.is_none() || t.frame.is_none() {
        p.costs = s
            .joints
            .iter()
            .map(|s| t.joints.iter().map(|t| unary(s, t, false)).collect())
            .collect();
    }
    ShortestPath.solve(&p)
}
pub fn greedy_names(s: &Rig, t: &Rig) -> Vec<Option<usize>> {
    let mut used = HashSet::new();
    s.joints
        .iter()
        .map(|a| {
            t.joints
                .iter()
                .enumerate()
                .find(|(i, b)| !used.contains(i) && !a.tokens.is_empty() && a.tokens == b.tokens)
                .map(|(i, _)| {
                    used.insert(i);
                    i
                })
        })
        .collect()
}

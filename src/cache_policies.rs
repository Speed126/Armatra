//! Deterministic unified-residency trace model; production CPU/GPU LRUs stay independent.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetCost {
    pub key: String,
    pub name: String,
    pub cpu_bytes: usize,
    pub gpu_bytes: usize,
    pub decode_us: f64,
    pub upload_us: f64,
}
impl AssetCost {
    fn bytes(&self) -> usize {
        self.cpu_bytes + self.gpu_bytes
    }
    fn cost(&self) -> f64 {
        self.decode_us + self.upload_us
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Access {
    pub key: String,
    pub scenario: String,
    pub observed_cpu_hit: bool,
    pub observed_gpu_hit: bool,
    pub switch_us: f64,
    pub cpu_resident_bytes: usize,
    pub gpu_resident_bytes: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trace {
    pub environment: serde_json::Value,
    pub assets: Vec<AssetCost>,
    pub accesses: Vec<Access>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Policy {
    ByteLru,
    GreedyDualSize,
    LruPrefetch,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub policy: Policy,
    pub budget: usize,
    pub peak_bytes: usize,
    pub final_bytes: usize,
    pub hits: usize,
    pub misses: usize,
    pub bypasses: usize,
    pub evictions: usize,
    pub hit_rate: f64,
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub demand_upload_bytes: usize,
    pub prefetch_upload_bytes: usize,
    pub prefetch_issued: usize,
    pub prefetch_used: usize,
    pub prefetch_precision: f64,
    pub reload_work_us: f64,
}
#[derive(Clone)]
struct Resident {
    bytes: usize,
    used: usize,
    priority: f64,
    prefetched: bool,
}
struct State {
    entries: BTreeMap<String, Resident>,
    bytes: usize,
    peak: usize,
    aging: f64,
    clock: usize,
    evictions: usize,
}
impl State {
    fn insert(
        &mut self,
        asset: &AssetCost,
        budget: usize,
        policy: Policy,
        prefetch: bool,
        protected: &str,
    ) -> bool {
        if asset.bytes() > budget || self.entries.contains_key(&asset.key) {
            return false;
        }
        while self.bytes + asset.bytes() > budget {
            let key = self
                .entries
                .iter()
                .filter(|(k, _)| k.as_str() != protected)
                .min_by(|(ka, a), (kb, b)| {
                    if policy == Policy::GreedyDualSize {
                        a.priority.total_cmp(&b.priority).then_with(|| ka.cmp(kb))
                    } else {
                        a.used.cmp(&b.used).then_with(|| ka.cmp(kb))
                    }
                })
                .map(|(k, _)| k.clone());
            let Some(key) = key else { return false };
            let old = self.entries.remove(&key).unwrap();
            self.bytes -= old.bytes;
            self.aging = old.priority;
            self.evictions += 1;
        }
        self.clock += 1;
        self.entries.insert(
            asset.key.clone(),
            Resident {
                bytes: asset.bytes(),
                used: self.clock,
                priority: self.aging + asset.cost() / asset.bytes().max(1) as f64,
                prefetched: prefetch,
            },
        );
        self.bytes += asset.bytes();
        self.peak = self.peak.max(self.bytes);
        true
    }
}
pub fn replay(trace: &Trace, budget: usize, policy: Policy) -> Result<Report> {
    ensure!(
        budget > 0 && !trace.accesses.is_empty(),
        "Empty replay/budget"
    );
    let catalog: BTreeMap<_, _> = trace.assets.iter().map(|a| (a.key.clone(), a)).collect();
    ensure!(
        catalog.len() == trace.assets.len()
            && trace
                .assets
                .iter()
                .all(|a| a.cpu_bytes.checked_add(a.gpu_bytes).is_some()
                    && a.cost().is_finite()
                    && a.decode_us >= 0.
                    && a.upload_us >= 0.),
        "Invalid cost catalog"
    );
    ensure!(
        trace
            .accesses
            .iter()
            .all(|a| catalog.contains_key(&a.key) && a.switch_us.is_finite() && a.switch_us >= 0.),
        "Invalid trace access"
    );
    let mut warm: Vec<_> = trace
        .accesses
        .iter()
        .filter(|a| a.observed_cpu_hit && a.observed_gpu_hit)
        .map(|a| a.switch_us)
        .collect();
    warm.sort_by(f64::total_cmp);
    let base = warm.get(warm.len() / 2).copied().unwrap_or(0.);
    let mut state = State {
        entries: BTreeMap::new(),
        bytes: 0,
        peak: 0,
        aging: 0.,
        clock: 0,
        evictions: 0,
    };
    let mut hits = 0;
    let mut misses = 0;
    let mut bypasses = 0;
    let mut demand_bytes = 0;
    let mut prefetch_bytes = 0;
    let mut issued = 0;
    let mut used = 0;
    let mut work = 0.;
    let mut latencies = Vec::new();
    let mut transitions: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut previous: Option<String> = None;
    for access in &trace.accesses {
        let asset = catalog[&access.key];
        state.clock += 1;
        if let Some(entry) = state.entries.get_mut(&access.key) {
            hits += 1;
            entry.used = state.clock;
            entry.priority = state.aging + asset.cost() / asset.bytes().max(1) as f64;
            if entry.prefetched {
                used += 1;
                entry.prefetched = false;
            }
            latencies.push(base);
        } else {
            misses += 1;
            demand_bytes += asset.gpu_bytes;
            work += asset.cost();
            latencies.push(base + asset.cost());
            if !state.insert(asset, budget, policy, false, "") {
                bypasses += 1;
            }
        }
        if let Some(prev) = previous {
            *transitions
                .entry(prev)
                .or_default()
                .entry(access.key.clone())
                .or_default() += 1;
        }
        if policy == Policy::LruPrefetch
            && let Some(nexts) = transitions.get(&access.key)
        {
            let total: usize = nexts.values().sum();
            let prediction = nexts
                .iter()
                .max_by(|(ka, a), (kb, b)| a.cmp(b).then_with(|| kb.cmp(ka)));
            if let Some((next, &count)) = prediction
                && count >= 2
                && count as f64 / total as f64 >= 0.6
                && next != &access.key
                && !state.entries.contains_key(next)
            {
                let candidate = catalog[next];
                if state.insert(candidate, budget, Policy::ByteLru, true, &access.key) {
                    issued += 1;
                    prefetch_bytes += candidate.gpu_bytes;
                    work += candidate.cost();
                }
            }
        }
        previous = Some(access.key.clone());
    }
    latencies.sort_by(f64::total_cmp);
    let percentile = |p: f64| latencies[((latencies.len() - 1) as f64 * p).ceil() as usize];
    Ok(Report {
        policy,
        budget,
        peak_bytes: state.peak,
        final_bytes: state.bytes,
        hits,
        misses,
        bypasses,
        evictions: state.evictions,
        hit_rate: hits as f64 / trace.accesses.len() as f64,
        p50_us: percentile(0.5),
        p95_us: percentile(0.95),
        p99_us: percentile(0.99),
        demand_upload_bytes: demand_bytes,
        prefetch_upload_bytes: prefetch_bytes,
        prefetch_issued: issued,
        prefetch_used: used,
        prefetch_precision: if issued > 0 {
            used as f64 / issued as f64
        } else {
            0.
        },
        reload_work_us: work,
    })
}
pub fn benchmark(trace: &Trace) -> Result<()> {
    let sum: usize = trace.assets.iter().map(AssetCost::bytes).sum();
    let mut rows = Vec::new();
    let mut sizes: Vec<_> = trace.assets.iter().map(AssetCost::bytes).collect();
    sizes.sort_unstable();
    let mut budgets = vec![
        sizes[sizes.len() / 2] * 2,
        sizes[sizes.len() / 2] * 4,
        sum / 4,
        sum / 2,
        sum,
    ];
    budgets.sort_unstable();
    budgets.dedup();
    for budget in budgets {
        for policy in [Policy::ByteLru, Policy::GreedyDualSize, Policy::LruPrefetch] {
            rows.push(replay(trace, budget, policy)?);
        }
    }
    crate::animation_bench::save(
        "cache-replay",
        &serde_json::json!({"environment":trace.environment,"method":"Recorded synthetic egui browse/revisit/alternating workloads. Equal combined CPU+GPU byte budgets; one active model per request; prefetch protects current model. Replay pXX = observed warm UI median + measured mean import/upload costs on modeled misses. Prefetch uses only previously observed transitions, count>=2 and confidence>=.6; background contention is not modeled. Timings are predicted replay latency, not actual OS input-to-photon measurements.","decision":"Keep production's independent byte-LRUs. Small controlled traces and optimistic prefetch scheduling do not justify default policy complexity.","rows":rows}),
    )
}

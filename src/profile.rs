//! Checksum protected, immutable profile generations. Unknown algorithms and
//! stale source/target/settings signatures are rejected before use.
use crate::{
    library::atomic_publish,
    retarget::{self, Plan, PlanKey},
    scene::Asset,
};
use anyhow::{Context, Result, bail};
use glam::Quat;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
#[derive(Serialize, Deserialize)]
struct Envelope {
    schema: u32,
    checksum: String,
    payload: String,
}
pub fn directory(root: &Path, key: &PlanKey) -> PathBuf {
    let digest = blake3::hash(&serde_json::to_vec(key).expect("plan key"));
    root.join("profiles").join(digest.to_hex().as_str())
}
pub fn validate(plan: &Plan, source: &Asset, target: &Asset) -> Result<()> {
    if plan.key != retarget::key(source, target, &plan.settings) {
        bail!("Stale or incompatible retarget profile")
    }
    let valid_q = |q: [f32; 4]| {
        let q = Quat::from_array(q);
        q.is_finite() && (q.length_squared() - 1.).abs() < 1e-3
    };
    if !valid_q(plan.frame)
        || !plan.scale.is_finite()
        || plan.scale <= 0.
        || !plan.target_height.is_finite()
        || plan.target_height <= 0.
        || plan.motion_source >= source.nodes.len()
        || plan.motion_target >= target.nodes.len()
    {
        bail!("Invalid root/frame in profile")
    }
    let mut used = HashSet::new();
    let mut sources = HashSet::new();
    for t in &plan.transfers {
        if t.source >= source.nodes.len()
            || t.target >= target.nodes.len()
            || !sources.insert(t.source)
            || !used.insert(t.target)
            || t.source_ancestor.is_some_and(|a| a >= source.nodes.len())
            || t.target_ancestor.is_some_and(|a| a >= target.nodes.len())
            || !valid_q(t.alignment)
            || !valid_q(t.source_rest_relative)
            || !valid_q(t.target_rest_relative)
        {
            bail!("Invalid transfer in profile")
        }
    }
    for c in &plan.chains {
        if c.nodes.len() != c.weights.len()
            || c.nodes.len() != c.axes.len()
            || c.nodes.len() < 2
            || c.nodes.iter().any(|&i| i >= target.nodes.len())
            || c.weights.iter().any(|w| !w.is_finite() || *w < 0.)
            || (c.weights.iter().sum::<f32>() - 1.).abs() > 1e-4
            || c.axes.iter().any(|a| {
                !glam::Vec3::from_array(*a).is_finite()
                    || (glam::Vec3::from_array(*a).length_squared() - 1.).abs() > 1e-3
            })
        {
            bail!("Invalid twist chain in profile")
        }
    }
    if plan.rows.iter().any(|r| {
        r.source >= source.nodes.len() || r.target.is_some_and(|j| j >= target.nodes.len())
    }) {
        bail!("Invalid mapping rows")
    }
    Ok(())
}
pub fn save(root: &Path, plan: &Plan, source: &Asset, target: &Asset) -> Result<PathBuf> {
    validate(plan, source, target)?;
    let payload = serde_json::to_string(plan)?;
    let envelope = Envelope {
        schema: 1,
        checksum: blake3::hash(payload.as_bytes()).to_hex().to_string(),
        payload,
    };
    let dir = directory(root, &plan.key);
    std::fs::create_dir_all(&dir)?;
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let path = dir.join(format!("{time:032}-{}.json", std::process::id()));
    atomic_publish(&path, &serde_json::to_vec(&envelope)?)?;
    Ok(path)
}
/// Restore the most recent saved settings and edits for an asset pair.
pub fn load_pair(root: &Path, source: &Asset, target: &Asset) -> Result<Option<Plan>> {
    let directory = root.join("profiles");
    if !directory.exists() {
        return Ok(None);
    }
    let source_key = retarget::signature(source);
    let target_key = retarget::signature(target);
    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(directory)?
        .take(4096)
        .filter_map(|e| e.ok())
    {
        if !entry.file_type()?.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(entry.path())?
            .take(256)
            .filter_map(|e| e.ok())
        {
            let p = file.path();
            if p.extension().is_some_and(|s| s == "json") {
                candidates.push(p);
            }
        }
    }
    candidates.sort_by_key(|p| std::cmp::Reverse(p.file_name().map(|s| s.to_owned())));
    for path in candidates {
        if std::fs::metadata(&path)?.len() > 4 * 1024 * 1024 {
            continue;
        }
        let Ok(env) = serde_json::from_slice::<Envelope>(&std::fs::read(&path)?) else {
            continue;
        };
        if env.schema != 1 || blake3::hash(env.payload.as_bytes()).to_hex().as_str() != env.checksum
        {
            continue;
        }
        let Ok(plan) = serde_json::from_str::<Plan>(&env.payload) else {
            continue;
        };
        if plan.key.source == source_key && plan.key.target == target_key {
            if validate(&plan, source, target).is_ok() {
                return Ok(Some(plan));
            }
            // Earlier frame inference did not recognize head/head-tip chains.
            // Recompile saved edits; never reuse old compiled transfers or
            // rewrite the immutable saved generation during restoration.
            let current_key = retarget::key(source, target, &plan.settings);
            let mut previous_key = plan.key.clone();
            previous_key.algorithm = current_key.algorithm;
            if matches!(plan.key.algorithm, 5 | 6) && previous_key == current_key {
                let mut checked = plan.clone();
                checked.key = current_key;
                if validate(&checked, source, target).is_ok()
                    && let Ok(mut upgraded) =
                        retarget::compile(source, target, &plan.settings, &plan.overrides)
                {
                    upgraded.warnings.push("Recompiled saved mapping with current rig/frame inference; stored profile unchanged".into());
                    return Ok(Some(upgraded));
                }
            }
        }
    }
    Ok(None)
}
pub fn load(root: &Path, key: &PlanKey, source: &Asset, target: &Asset) -> Result<Option<Plan>> {
    let dir = directory(root, key);
    if !dir.exists() {
        return Ok(None);
    }
    let mut paths: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|s| s == "json"))
        .collect();
    paths.sort();
    paths.reverse();
    let mut last_error = None;
    for path in paths {
        let candidate = (|| -> Result<Plan> {
            if std::fs::metadata(&path)?.len() > 4 * 1024 * 1024 {
                bail!("Profile too large")
            }
            let env: Envelope = serde_json::from_slice(&std::fs::read(path)?)?;
            if env.schema != 1
                || blake3::hash(env.payload.as_bytes()).to_hex().as_str() != env.checksum
            {
                bail!("Profile checksum/schema")
            }
            let plan: Plan = serde_json::from_str(&env.payload)?;
            if plan.key != *key {
                bail!("Profile key mismatch")
            }
            validate(&plan, source, target)?;
            Ok(plan)
        })();
        match candidate {
            Ok(mut plan) => {
                if last_error.is_some() {
                    plan.warnings.push(
                        "Recovered an older valid retarget profile after an invalid generation"
                            .into(),
                    );
                }
                return Ok(Some(plan));
            }
            Err(e) => last_error = Some(e),
        }
    }
    Err(last_error.context("No valid profile generations")?)
}

use crate::{animation::Clip, scene::Asset};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub enum Compatibility {
    SameAsset,
    Exact,
    NeedsMapping(String),
    Unsupported(String),
}
#[derive(Clone, Debug)]
pub struct Mapping {
    pub status: Compatibility,
    pub nodes: Vec<Option<usize>>,
}
pub fn compile(source: &Asset, target: &Asset, clip: &Clip, same_asset: bool) -> Mapping {
    let fail = |status| Mapping {
        status,
        nodes: Vec::new(),
    };
    if !clip.supported {
        return fail(Compatibility::Unsupported(
            "This clip animates morph weights".into(),
        ));
    }
    if !target.exact_mesh {
        return fail(Compatibility::Unsupported(
            "Target has unsupported deformation or primitives; exact playback unavailable".into(),
        ));
    }
    if same_asset {
        return Mapping {
            status: Compatibility::SameAsset,
            nodes: (0..source.nodes.len()).map(Some).collect(),
        };
    }
    let mut required = HashSet::new();
    for track in &clip.tracks {
        let mut node = Some(track.node);
        while let Some(i) = node {
            required.insert(i);
            node = source.nodes[i].parent;
        }
    }
    let source_joints: HashSet<_> = source
        .skins
        .iter()
        .flat_map(|s| s.joints.iter().copied())
        .collect();
    let target_joints: HashSet<_> = target
        .skins
        .iter()
        .flat_map(|s| s.joints.iter().copied())
        .collect();
    let mut nodes = vec![None; source.nodes.len()];
    let mut used = HashSet::new();
    let mut by_parent_name: HashMap<(Option<usize>, &str), Vec<usize>> = HashMap::new();
    let mut by_parent: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
    for (id, node) in target.nodes.iter().enumerate() {
        by_parent_name
            .entry((node.parent, &node.name))
            .or_default()
            .push(id);
        by_parent.entry(node.parent).or_default().push(id);
    }
    for &i in &source.order {
        if !required.contains(&i) {
            continue;
        }
        let s = &source.nodes[i];
        let parent = s.parent.and_then(|p| nodes[p]);
        let possible = if s.name.is_empty() {
            by_parent.get(&parent)
        } else {
            by_parent_name.get(&(parent, s.name.as_str()))
        };
        if s.name.is_empty() && possible.is_some_and(|v| v.len() > 4096) {
            return fail(Compatibility::NeedsMapping("Too many unnamed siblings for safe automatic correspondence; source-rig association or explicit mapping required".into()));
        }
        let candidates: Vec<_> = possible
            .into_iter()
            .flatten()
            .copied()
            .filter(|&j| {
                let t = &target.nodes[j];
                !used.contains(&j)
                    && (!source_joints.contains(&i) || target_joints.contains(&j))
                    && s.rest.matrix().abs_diff_eq(t.rest.matrix(), 1e-4)
            })
            .collect();
        if candidates.len() != 1 {
            return fail(Compatibility::NeedsMapping(format!(
                "Node '{}' ({i}): {} exact candidates. Source-rig association or manual mapping required; different rest frames need retargeting.",
                s.name,
                candidates.len()
            )));
        }
        nodes[i] = Some(candidates[0]);
        used.insert(candidates[0]);
    }
    Mapping {
        status: Compatibility::Exact,
        nodes,
    }
}

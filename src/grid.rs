//! Visual grid units derived once from visible rest geometry at GPU upload.
//! No animated bounds, world-unit conversion, root or contact changes.
use crate::scene::Asset;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Grid {
    pub interval: f32,
    pub half_extent: f32,
    pub character_size: f32,
}
impl Grid {
    pub fn from_size(size: f32) -> Self {
        let size = if size.is_finite() && size > 0. {
            size.clamp(1e-12, 1e12)
        } else {
            2.
        };
        let target = size / 4.;
        let decade = 10f32.powf(target.log10().floor());
        let interval = [1., 2., 5., 10.]
            .into_iter()
            .map(|m| m * decade)
            .min_by(|a, b| (a / target).ln().abs().total_cmp(&(b / target).ln().abs()))
            .unwrap();
        Self {
            interval,
            half_extent: interval * 8.,
            character_size: size,
        }
    }
    pub fn for_asset(asset: &Asset, scene: usize) -> Self {
        let geometry = asset.visible_rest_bounds(scene).and_then(|(min, max)| {
            let extent = max - min;
            let size = extent.max_element();
            (min.is_finite() && max.is_finite() && size.is_finite() && size > 0.).then_some(size)
        });
        let size = geometry.unwrap_or_else(|| {
            let rig = crate::rig::Rig::analyze(asset);
            let visible: std::collections::HashSet<_> =
                asset.visible_nodes(scene).into_iter().collect();
            let globals = asset.globals(&asset.rest_pose());
            let mut min = glam::Vec3::splat(f32::INFINITY);
            let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
            for joint in &rig.joints {
                let position = globals[joint.node].w_axis.truncate();
                if visible.contains(&joint.node)
                    && position.is_finite()
                    && !matches!(
                        joint.anatomy,
                        crate::rig::Anatomy::Helper
                            | crate::rig::Anatomy::Root
                            | crate::rig::Anatomy::Unknown
                    )
                {
                    min = min.min(position);
                    max = max.max(position);
                }
            }
            let semantic_size = (max - min).max_element();
            if semantic_size.is_finite() && semantic_size > 0. {
                semantic_size
            } else {
                rig.height
            }
        });
        Self::from_size(size)
    }
}

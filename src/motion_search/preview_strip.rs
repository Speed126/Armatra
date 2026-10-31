//! UI projection over existing medoid times. No retrieval descriptor, clustering
//! or score changes. One frame across the clip's real medoids and rest pose.
use super::*;
use glam::Quat;

pub const PREVIEW_FRAME_VERSION: u32 = 2;
pub struct PoseImage {
    pub time: f32,
    pub image: image::RgbaImage,
}
fn pose_points(asset: &Asset, rig: &Rig, time: Option<(&Clip, f32)>) -> Vec<Vec3> {
    let rest = asset.globals(&asset.rest_pose());
    let globals = if let Some((clip, time)) = time {
        asset.globals(&animation::evaluate(
            asset,
            clip,
            &(0..asset.nodes.len()).map(Some).collect::<Vec<_>>(),
            time,
            &mut vec![],
        ))
    } else {
        rest.clone()
    };
    let inverse = rig.frame.unwrap_or_default().inverse();
    let root = rig
        .joints
        .iter()
        .find(|j| j.anatomy == Anatomy::Root)
        .or_else(|| rig.joints.iter().find(|j| j.anatomy == Anatomy::Pelvis));
    let travel = root.map_or(Vec3::ZERO, |j| {
        inverse * (globals[j.node].w_axis.truncate() - rest[j.node].w_axis.truncate()) / rig.height
    });
    let camera =
        Quat::from_rotation_x(5f32.to_radians()) * Quat::from_rotation_y(58f32.to_radians());
    rig.joints
        .iter()
        .map(|j| {
            let p = inverse * (globals[j.node].w_axis.truncate() - rig.origin) / rig.height;
            camera * (p - Vec3::new(travel.x, 0., travel.z))
        })
        .collect()
}
pub fn preview_strip(asset: &Asset, clip: &Clip, times: &[f32]) -> Vec<PoseImage> {
    let rig = Rig::analyze(asset);
    let mut times = times
        .iter()
        .copied()
        .filter(|t| t.is_finite())
        .take(4)
        .collect::<Vec<_>>();
    times.sort_by(f32::total_cmp);
    let rest = pose_points(asset, &rig, None);
    let poses: Vec<_> = times
        .iter()
        .map(|t| pose_points(asset, &rig, Some((clip, *t))))
        .collect();
    let points = poses.iter().chain(std::iter::once(&rest)).flatten();
    let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for p in points {
        min = min.min(*p);
        max = max.max(*p);
    }
    if !min.is_finite() || !max.is_finite() {
        min = Vec3::ZERO;
        max = Vec3::ONE;
    }
    let scale = (220. / (max.x - min.x).max(0.01)).min(226. / (max.y - min.y).max(0.01));
    let point = |v: Vec3| {
        [
            130. + (v.x - (min.x + max.x) * 0.5) * scale,
            135. - (v.y - (min.y + max.y) * 0.5) * scale,
        ]
    };
    let background = image::Rgba(crate::ui::theme::BACKGROUND.to_array());
    let bone = image::Rgba(crate::ui::theme::TEXT.to_array());
    let faint = image::Rgba(crate::ui::theme::BORDER.to_array());
    times
        .into_iter()
        .zip(poses)
        .map(|(time, pose)| {
            let mut image = image::RgbaImage::from_pixel(260, 270, background);
            let reference = rig
                .joints
                .iter()
                .enumerate()
                .filter(|(_, j)| j.anatomy == Anatomy::Foot)
                .map(|(i, _)| point(rest[i])[1])
                .fold(f32::NEG_INFINITY, f32::max);
            if reference.is_finite() {
                line(
                    &mut image,
                    [20., reference],
                    [240., reference],
                    faint,
                    false,
                    0,
                );
            }
            for (i, joint) in rig.joints.iter().enumerate() {
                if let Some(parent) = rig.joints.iter().position(|p| Some(p.node) == joint.parent) {
                    line(
                        &mut image,
                        point(pose[parent]),
                        point(pose[i]),
                        bone,
                        joint.side == Side::Right,
                        1,
                    );
                }
                let [x, y] = point(pose[i]);
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        if dx * dx + dy * dy <= 4 {
                            pixel(
                                &mut image,
                                x.round() as i32 + dx,
                                y.round() as i32 + dy,
                                bone,
                            );
                        }
                    }
                }
            }
            PoseImage { time, image }
        })
        .collect()
}
fn pixel(image: &mut image::RgbaImage, x: i32, y: i32, color: image::Rgba<u8>) {
    if (0..image.width() as i32).contains(&x) && (0..image.height() as i32).contains(&y) {
        image.put_pixel(x as u32, y as u32, color);
    }
}
fn line(
    image: &mut image::RgbaImage,
    a: [f32; 2],
    b: [f32; 2],
    color: image::Rgba<u8>,
    dashed: bool,
    radius: i32,
) {
    let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2))
        .sqrt()
        .max(1.);
    for n in 0..=length.ceil() as usize {
        if dashed && (n / 6) % 2 == 1 {
            continue;
        }
        let t = n as f32 / length;
        let x = (a[0] + (b[0] - a[0]) * t).round() as i32;
        let y = (a[1] + (b[1] - a[1]) * t).round() as i32;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                pixel(image, x + dx, y + dy, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_uses_real_times_shared_bounds_and_preserves_vertical_travel() {
        let (name, builder) = crate::motion_fixtures::motions()
            .into_iter()
            .find(|(name, _)| name == "Jump")
            .unwrap();
        assert_eq!(name, "Jump");
        let asset = crate::import::from_bytes(&builder.finish(), &crate::import::Limits::default())
            .unwrap();
        let times = [0., 0.5, 1., 1.5];
        let images = preview_strip(&asset, &asset.clips[0], &times);
        assert_eq!(images.iter().map(|p| p.time).collect::<Vec<_>>(), times);
        assert!(images.iter().all(|p| p.image.dimensions() == (260, 270)));
        assert_ne!(
            images[0].image.as_raw(),
            images[1].image.as_raw(),
            "Root lift must not be recentered away"
        );
    }
}

//! Ordinary rigid-only dual quaternions, xyzw convention. LBS is the safe default.
use crate::scene::{Asset, Vertex, skin_position};
use glam::{Mat3, Mat4, Quat, Vec3, Vec4};

pub fn rigid(m: Mat4) -> bool {
    if !m.is_finite()
        || (m.w_axis.w - 1.).abs() > 1e-4
        || m.x_axis.w.abs() + m.y_axis.w.abs() + m.z_axis.w.abs() > 1e-4
    {
        return false;
    }
    let axes = [
        m.x_axis.truncate(),
        m.y_axis.truncate(),
        m.z_axis.truncate(),
    ];
    axes.iter().all(|v| (v.length_squared() - 1.).abs() < 1e-4)
        && axes[0].dot(axes[1]).abs() < 1e-4
        && axes[0].dot(axes[2]).abs() < 1e-4
        && axes[1].dot(axes[2]).abs() < 1e-4
        && (m.determinant() - 1.).abs() < 1e-4
}
pub fn dual_quaternion(m: Mat4) -> Option<(Quat, Quat)> {
    if !rigid(m) {
        return None;
    }
    let real = Quat::from_mat3(&Mat3::from_mat4(m)).normalize();
    let t = m.w_axis.truncate();
    let dual = (Quat::from_xyzw(t.x, t.y, t.z, 0.) * real) * 0.5;
    Some((real, dual))
}
pub fn packed_palette(
    asset: &Asset,
    node: usize,
    globals: &[Mat4],
    palette: &[Mat4],
) -> Option<Vec<Mat4>> {
    if !rigid(globals[node]) {
        return None;
    }
    if let Some(skin) = asset.nodes[node].skin {
        let skin = &asset.skins[skin];
        if skin.joints.iter().any(|&j| !rigid(globals[j]))
            || skin.inverse_bind.iter().any(|&m| !rigid(m))
        {
            return None;
        }
    }
    palette
        .iter()
        .map(|&m| {
            dual_quaternion(m).map(|(r, d)| {
                Mat4::from_cols(
                    Vec4::from_array(r.to_array()),
                    Vec4::from_array(d.to_array()),
                    Vec4::ZERO,
                    Vec4::ZERO,
                )
            })
        })
        .collect()
}
pub fn blend(vertex: &Vertex, palette: &[(Quat, Quat)]) -> Option<(Quat, Vec3)> {
    let mut real = Vec4::ZERO;
    let mut dual = Vec4::ZERO;
    let mut reference = None;
    for (&j, &w) in vertex.joints.iter().zip(&vertex.weights) {
        if w > 0.
            && let Some(&(r, d)) = palette.get(j as usize)
        {
            let r = Vec4::from_array(r.to_array());
            let d = Vec4::from_array(d.to_array());
            let q = *reference.get_or_insert(r);
            let sign = if q.dot(r) < 0. { -1. } else { 1. };
            real += r * w * sign;
            dual += d * w * sign;
        }
    }
    let norm = real.length();
    if norm < 1e-7 {
        return None;
    }
    real /= norm;
    dual /= norm;
    dual -= real * real.dot(dual);
    let rotation = Quat::from_array(real.to_array());
    let translation = (Quat::from_array(dual.to_array()) * rotation.conjugate()) * 2.;
    Some((
        rotation,
        Vec3::new(translation.x, translation.y, translation.z),
    ))
}
pub fn position(vertex: &Vertex, palette: &[Mat4]) -> Vec3 {
    let Some(duals) = palette
        .iter()
        .copied()
        .map(dual_quaternion)
        .collect::<Option<Vec<_>>>()
    else {
        return skin_position(vertex, palette);
    };
    blend(vertex, &duals).map_or(Vec3::from(vertex.position), |(r, t)| {
        r * Vec3::from(vertex.position) + t
    })
}

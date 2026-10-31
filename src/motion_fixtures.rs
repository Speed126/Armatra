use crate::{fixtures::Builder, rig_corpus};
use glam::Quat;
use serde_json::json;

/// Original layered-material regression for the Library's transition frame.
pub fn layered_preview() -> Builder {
    let mut b = twist(false);
    for i in [1, 2] {
        b.json["materials"].as_array_mut().unwrap().push(json!({"name":format!("Original test material {i}"),"pbrMetallicRoughness":{"baseColorFactor":[0.9,0.4,0.2,1]}}));
    }
    b.json["meshes"][0]["primitives"][0]["material"] = json!(2);
    b.json["animations"][0]["name"] = json!("Layered twist");
    b
}

pub fn motions() -> Vec<(String, Builder)> {
    let mut result = Vec::new();
    for (name, family, duration) in [
        ("Walk", "walk", 2.),
        ("Fast-walk", "walk", 1.3),
        ("Slow-walk", "walk", 2.8),
        ("Wave", "wave", 2.),
        ("Fast-wave", "wave", 1.3),
        ("Idle", "idle", 2.),
        ("Sway", "sway", 2.),
        ("Jump", "jump", 2.),
        ("Fast-jump", "jump", 1.3),
    ] {
        let (mut b, _) = rig_corpus::humanoid(Default::default(), family == "walk");
        let times: Vec<_> = (0..=120).map(|i| i as f32 / 120. * duration).collect();
        let input = b.accessor(&times, "SCALAR", 1);
        b.json["accessors"][input]["min"] = json!([0.]);
        b.json["accessors"][input]["max"] = json!([duration]);
        if family == "walk" {
            for sampler in b.json["animations"][0]["samplers"].as_array_mut().unwrap() {
                sampler["input"] = json!(input);
            }
            b.json["animations"][0]["name"] = json!(name);
        } else {
            let rotation = family == "wave";
            let values: Vec<f32> = times
                .iter()
                .flat_map(|&t| {
                    let phase = t / duration * std::f32::consts::TAU;
                    if rotation {
                        Quat::from_rotation_z(0.7 + phase.sin() * 0.65)
                            .to_array()
                            .to_vec()
                    } else {
                        let y = match family {
                            "jump" => (phase * 0.5).sin() * 0.55,
                            "sway" => phase.sin() * 0.025,
                            _ => 0.,
                        };
                        vec![0., y, 0.]
                    }
                })
                .collect();
            let out = b.accessor(
                &values,
                if rotation { "VEC4" } else { "VEC3" },
                if rotation { 4 } else { 3 },
            );
            b.json["animations"] = json!([{"name":name,"samplers":[{"input":input,"output":out,"interpolation":"LINEAR"}],"channels":[{"sampler":0,"target":{"node":if rotation{7}else{0},"path":if rotation{"rotation"}else{"translation"}}}]}]);
        }
        result.push((name.into(), b));
    }
    result
}

pub fn twist(affine: bool) -> Builder {
    use glam::{Mat4, Vec3};
    let mut b = Builder::default();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut weights = Vec::new();
    let mut joints = Vec::new();
    for y in 0..=32 {
        for radial in 0..32 {
            let angle = radial as f32 / 32. * std::f32::consts::TAU;
            let t = y as f32 / 32.;
            positions.extend([0.4 * angle.cos(), t * 2., 0.4 * angle.sin()]);
            normals.extend([angle.cos(), 0., angle.sin()]);
            weights.extend([1. - t, t, 0., 0.]);
            joints.extend([0u8, 1, 0, 0]);
        }
    }
    let position = b.positions(&positions);
    let normal = b.accessor(&normals, "VEC3", 3);
    let weight = b.accessor(&weights, "VEC4", 4);
    let view = b.view(&joints);
    let joint = b.json["accessors"].as_array().unwrap().len();
    b.json["accessors"]
        .as_array_mut()
        .unwrap()
        .push(json!({"bufferView":view,"componentType":5121,"type":"VEC4","count":33*32}));
    let mut indices = Vec::new();
    for y in 0..32u16 {
        for r in 0..32u16 {
            let a = y * 32 + r;
            let c = y * 32 + (r + 1) % 32;
            indices.extend([a, a + 32, c, c, a + 32, c + 32]);
        }
    }
    let view = b.view(
        &indices
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    );
    let index = b.json["accessors"].as_array().unwrap().len();
    b.json["accessors"].as_array_mut().unwrap().push(
        json!({"bufferView":view,"componentType":5123,"type":"SCALAR","count":indices.len()}),
    );
    let ibm = b.accessor(
        &[
            Mat4::IDENTITY.to_cols_array(),
            Mat4::from_translation(-Vec3::Y * 2.).to_cols_array(),
        ]
        .concat(),
        "MAT4",
        16,
    );
    b.json["nodes"] = json!([{"name":"Base","children":[1]},{"name":"Tip","translation":[0,2,0]},{"name":"Twist cylinder","mesh":0,"skin":0,"scale":if affine{[1.,1.3,1.]}else{[1.;3]}}]);
    b.json["scenes"][0]["nodes"] = json!([0, 2]);
    b.json["skins"] = json!([{"joints":[0,1],"inverseBindMatrices":ibm}]);
    b.json["materials"] =
        json!([{"doubleSided":true,"pbrMetallicRoughness":{"baseColorFactor":[0.3,0.8,0.65,1.]}}]);
    b.json["meshes"] = json!([{"primitives":[{"attributes":{"POSITION":position,"NORMAL":normal,"JOINTS_0":joint,"WEIGHTS_0":weight},"indices":index,"material":0}]}]);
    let input = b.accessor(&[0., 1.], "SCALAR", 1);
    b.json["accessors"][input]["min"] = json!([0.]);
    b.json["accessors"][input]["max"] = json!([1.]);
    let output = b.accessor(
        &[
            Quat::IDENTITY.to_array(),
            Quat::from_rotation_y(3.).to_array(),
        ]
        .concat(),
        "VEC4",
        4,
    );
    b.json["animations"] = json!([{"name":"170 degree twist","samplers":[{"input":input,"output":output,"interpolation":"LINEAR"}],"channels":[{"sampler":0,"target":{"node":1,"path":"rotation"}}]}]);
    b
}

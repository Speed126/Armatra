//! Reproducible, original humanoid ground truth. Node identities are recorded
//! before names, rest frames, proportions, and node indices are changed.
use crate::{
    fixtures::Builder,
    import::{self, Limits},
    scene::Asset,
};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Case {
    pub name: String,
    pub expected: Vec<Option<usize>>,
    pub review: Vec<usize>,
}
#[derive(Clone, Copy, Default)]
pub struct Variant {
    pub renamed: bool,
    pub permuted: bool,
    pub proportions: bool,
    pub frames: bool,
    pub helpers: bool,
    pub missing: bool,
    pub unnamed: bool,
    pub ambiguous: bool,
    pub nonuniform: bool,
}
struct Bone {
    name: String,
    parent: Option<usize>,
    position: Vec3,
    rotation: Quat,
    identity: Option<usize>,
}
pub fn humanoid(v: Variant, animated: bool) -> (Builder, Vec<Option<usize>>) {
    let defs = [
        ("Root", None, [0., 0., 0.]),
        ("Pelvis", Some(0), [0., 1., 0.]),
        ("Spine", Some(1), [0., 0.25, 0.]),
        ("Chest", Some(2), [0., 0.28, 0.]),
        ("Neck", Some(3), [0., 0.2, 0.]),
        ("Head", Some(4), [0., 0.15, 0.]),
        ("Shoulder.L", Some(3), [0.17, 0.12, 0.]),
        ("UpperArm.L", Some(6), [0.14, 0., 0.]),
        ("Forearm.L", Some(7), [0.3, 0., 0.]),
        ("Hand.L", Some(8), [0.27, 0., 0.]),
        ("Shoulder.R", Some(3), [-0.17, 0.12, 0.]),
        ("UpperArm.R", Some(10), [-0.14, 0., 0.]),
        ("Forearm.R", Some(11), [-0.3, 0., 0.]),
        ("Hand.R", Some(12), [-0.27, 0., 0.]),
        ("Thigh.L", Some(1), [0.13, -0.08, 0.]),
        ("Shin.L", Some(14), [0., -0.42, 0.025]),
        ("Foot.L", Some(15), [0., -0.42, -0.025]),
        ("Toe.L", Some(16), [0., -0.02, 0.15]),
        ("Thigh.R", Some(1), [-0.13, -0.08, 0.]),
        ("Shin.R", Some(18), [0., -0.42, 0.025]),
        ("Foot.R", Some(19), [0., -0.42, -0.025]),
        ("Toe.R", Some(20), [0., -0.02, 0.15]),
        ("Index.L", Some(9), [0.12, 0., 0.]),
        ("Index.R", Some(13), [-0.12, 0., 0.]),
    ];
    let aliases = [
        "rig",
        "hips",
        "spinal",
        "thorax",
        "neck_base",
        "skull",
        "clavicle_l",
        "humerus_l",
        "elbow_l",
        "wrist_l",
        "clavicle_r",
        "humerus_r",
        "elbow_r",
        "wrist_r",
        "femur_l",
        "calf_l",
        "ankle_l",
        "ball_l",
        "femur_r",
        "calf_r",
        "ankle_r",
        "ball_r",
        "pointer_l",
        "pointer_r",
    ];
    let mut bones: Vec<Bone> = Vec::new();
    let mut identity: Vec<Option<usize>> = vec![None; defs.len()];
    for (i, (name, parent, t)) in defs.iter().enumerate() {
        if v.missing && [17, 21, 22, 23].contains(&i) {
            continue;
        }
        let mut delta = Vec3::from(*t);
        if v.proportions {
            delta *= if [15, 16, 19, 20].contains(&i) {
                Vec3::new(1., 1.35, 1.)
            } else {
                Vec3::new(0.8, 1.15, 1.)
            };
        }
        let p = parent.and_then(|p| identity[p]);
        let position = p.map_or(delta, |p| bones[p].position + delta);
        let mut name = if v.renamed {
            aliases[i].to_string()
        } else {
            name.to_string()
        };
        if v.unnamed && ![0, 1, 5, 14, 18].contains(&i) {
            name.clear()
        }
        if v.ambiguous && [7, 11].contains(&i) {
            name = "arm".into()
        }
        let rotation = if v.frames {
            Quat::from_euler(
                glam::EulerRot::XYZ,
                (i as f32 * 0.43).sin() * 0.5,
                0.35,
                (i as f32 * 0.2).cos() * 0.4,
            )
        } else {
            Quat::IDENTITY
        };
        identity[i] = Some(bones.len());
        bones.push(Bone {
            name,
            parent: p,
            position,
            rotation,
            identity: Some(i),
        });
    }
    if v.helpers {
        for (a, b) in [(8, 9), (12, 13), (2, 3)] {
            let a = identity[a].unwrap();
            let b = identity[b].unwrap();
            let index = bones.len();
            bones.push(Bone {
                name: if a == 2 {
                    "SpineHelper".into()
                } else {
                    format!("ForearmTwist.{}", if a == 8 { "L" } else { "R" })
                },
                parent: Some(a),
                position: bones[a].position.lerp(bones[b].position, 0.5),
                rotation: bones[a].rotation,
                identity: None,
            });
            bones[b].parent = Some(index);
        }
    }
    let mut ids: Vec<_> = (0..bones.len()).collect();
    if v.permuted {
        ids.reverse()
    }
    let remap = |i: usize| ids.iter().position(|&x| x == i).unwrap();
    let mut b = Builder::default();
    b.json["nodes"]=json!(ids.iter().map(|&i| {
        let n=&bones[i]; let pr=n.parent.map_or(Quat::IDENTITY,|p|bones[p].rotation);
        let pt=n.parent.map_or(Vec3::ZERO,|p|bones[p].position);
        json!({"name":n.name,"translation":(pr.inverse()*(n.position-pt)).to_array(),"rotation":(pr.inverse()*n.rotation).to_array(),"children":bones.iter().enumerate().filter(|(_,x)|x.parent==Some(i)).map(|(j,_)|remap(j)).collect::<Vec<_>>()})
    }).collect::<Vec<_>>());
    if v.nonuniform {
        b.json["nodes"][remap(0)]["scale"] = json!([1., 1.2, 0.8]);
    }
    b.json["scenes"][0]["nodes"] = json!([remap(0), bones.len()]);
    // Rigid bone boxes skinned with one influence; keeps geometry/rest truth clear.
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut joints = Vec::<u16>::new();
    let mut weights = Vec::new();
    for (i, n) in bones
        .iter()
        .enumerate()
        .filter(|(_, n)| n.identity != Some(0))
    {
        let size = if n.identity == Some(5) {
            Vec3::splat(0.18)
        } else if matches!(n.identity, Some(1..=3)) {
            Vec3::new(0.28, 0.22, 0.18)
        } else {
            Vec3::splat(0.08)
        };
        let end = bones
            .iter()
            .find(|x| x.parent == Some(i))
            .map_or(n.position + Vec3::Y * 0.08, |x| x.position);
        let center = n.position.lerp(end, 0.45);
        let extent = size.max((end - n.position).abs() * 0.45);
        for (normal, u, w) in [
            (Vec3::X, Vec3::Y, Vec3::Z),
            (-Vec3::X, -Vec3::Y, Vec3::Z),
            (Vec3::Y, Vec3::Z, Vec3::X),
            (-Vec3::Y, -Vec3::Z, Vec3::X),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (-Vec3::Z, -Vec3::X, Vec3::Y),
        ] {
            for (x, y) in [
                (-1., -1.),
                (1., -1.),
                (1., 1.),
                (-1., -1.),
                (1., 1.),
                (-1., 1.),
            ] {
                positions.extend_from_slice((center + (normal + u * x + w * y) * extent).as_ref());
                normals.extend_from_slice(normal.as_ref());
                joints.extend_from_slice(&[remap(i) as u16, 0, 0, 0]);
                weights.extend_from_slice(&[1., 0., 0., 0.]);
            }
        }
    }
    let p = b.positions(&positions);
    let n = b.accessor(&normals, "VEC3", 3);
    let w = b.accessor(&weights, "VEC4", 4);
    let view = b.view(
        &joints
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect::<Vec<_>>(),
    );
    let j = b.json["accessors"].as_array().unwrap().len();
    b.json["accessors"]
        .as_array_mut()
        .unwrap()
        .push(json!({"bufferView":view,"componentType":5123,"type":"VEC4","count":joints.len()/4}));
    b.json["materials"] = json!([{"pbrMetallicRoughness":{"baseColorFactor":if v.renamed {[0.18,0.65,0.85,1.]} else {[0.9,0.5,0.18,1.]},"metallicFactor":0.}}]);
    b.json["meshes"] = json!([{"primitives":[{"attributes":{"POSITION":p,"NORMAL":n,"JOINTS_0":j,"WEIGHTS_0":w},"material":0}]}]);
    let ibms: Vec<_> = ids
        .iter()
        .flat_map(|&i| {
            Mat4::from_rotation_translation(bones[i].rotation, bones[i].position)
                .inverse()
                .to_cols_array()
        })
        .collect();
    let ibm = b.accessor(&ibms, "MAT4", 16);
    b.json["skins"] =
        json!([{"joints":(0..bones.len()).collect::<Vec<_>>(),"inverseBindMatrices":ibm}]);
    b.json["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"Body","mesh":0,"skin":0}));
    if animated {
        let times: Vec<f32> = (0..=120).map(|i| i as f32 / 60.).collect();
        let input = b.accessor(&times, "SCALAR", 1);
        b.json["accessors"][input]["min"] = json!([0.]);
        b.json["accessors"][input]["max"] = json!([2.]);
        let mut samplers = Vec::new();
        let mut channels = Vec::new();
        let root_values: Vec<_> = times.iter().flat_map(|&t| [0., 0., t * 0.2]).collect();
        let out = b.accessor(&root_values, "VEC3", 3);
        samplers.push(json!({"input":input,"output":out,"interpolation":"LINEAR"}));
        channels.push(
            json!({"sampler":0,"target":{"node":remap(identity[0].unwrap()),"path":"translation"}}),
        );
        for id in [14, 15, 16, 18, 19, 20, 7, 11] {
            let values: Vec<_> = times
                .iter()
                .flat_map(|&t| {
                    let left = id < 18 && id != 11;
                    let phase = (t + if left { 0. } else { 1. }).rem_euclid(2.);
                    let q = if [7, 11].contains(&id) {
                        Quat::from_rotation_x(
                            (t * std::f32::consts::PI
                                + if left { 0. } else { std::f32::consts::PI })
                            .sin()
                                * 0.25,
                        )
                    } else {
                        let hip = Vec3::new(if left { 0.13 } else { -0.13 }, 0.92, t * 0.2);
                        let z = if phase < 1. {
                            (if left { 0.22 } else { 0.02 })
                                + ((t + if left { 0. } else { 1. }) / 2.).floor() * 0.4
                        } else {
                            let u = phase - 1.;
                            (if left { 0.22 } else { 0.02 })
                                + ((t + if left { 0. } else { 1. }) / 2.).floor() * 0.4
                                + 0.4 * (u * u * (3. - 2. * u))
                        };
                        let lift = if phase < 1. {
                            0.
                        } else {
                            0.12 * ((phase - 1.) * std::f32::consts::PI).sin()
                        };
                        let foot = Vec3::new(hip.x, 0.08 + lift, z);
                        let d = foot - hip;
                        let c = d.length().min(0.838);
                        let dir = d.normalize();
                        let knee = hip
                            + dir * (c * 0.5)
                            + (Vec3::Z - dir * dir.dot(Vec3::Z)).normalize_or_zero()
                                * (0.42f32.powi(2) - (c * 0.5).powi(2)).max(0.).sqrt();
                        let thigh = Quat::from_rotation_arc(
                            Vec3::new(0., -0.42, 0.025).normalize(),
                            (knee - hip).normalize(),
                        );
                        let shin_global = Quat::from_rotation_arc(
                            Vec3::new(0., -0.42, -0.025).normalize(),
                            (foot - knee).normalize(),
                        );
                        match id {
                            14 | 18 => thigh,
                            15 | 19 => thigh.inverse() * shin_global,
                            _ => shin_global.inverse(),
                        }
                    };
                    q.to_array()
                })
                .collect();
            let out = b.accessor(&values, "VEC4", 4);
            let sampler = samplers.len();
            samplers.push(json!({"input":input,"output":out,"interpolation":"LINEAR"}));
            channels.push(json!({"sampler":sampler,"target":{"node":remap(identity[id].unwrap()),"path":"rotation"}}));
        }
        b.json["animations"] =
            json!([{"name":"Measured walk","samplers":samplers,"channels":channels}]);
    }
    (b, identity.into_iter().map(|i| i.map(remap)).collect())
}
pub fn cases() -> Vec<(Case, Asset, Asset)> {
    let variants = [
        (
            "renamed",
            Variant {
                renamed: true,
                ..Default::default()
            },
        ),
        (
            "permuted",
            Variant {
                permuted: true,
                ..Default::default()
            },
        ),
        (
            "proportions",
            Variant {
                renamed: true,
                proportions: true,
                ..Default::default()
            },
        ),
        (
            "rest-frames",
            Variant {
                renamed: true,
                frames: true,
                ..Default::default()
            },
        ),
        (
            "twist-spine-helpers",
            Variant {
                renamed: true,
                helpers: true,
                ..Default::default()
            },
        ),
        (
            "missing-toes-fingers",
            Variant {
                renamed: true,
                missing: true,
                ..Default::default()
            },
        ),
        (
            "unnamed",
            Variant {
                unnamed: true,
                ..Default::default()
            },
        ),
        (
            "ambiguous",
            Variant {
                ambiguous: true,
                ..Default::default()
            },
        ),
        (
            "nonuniform-scale",
            Variant {
                nonuniform: true,
                ..Default::default()
            },
        ),
        (
            "combined",
            Variant {
                renamed: true,
                permuted: true,
                proportions: true,
                frames: true,
                helpers: true,
                ..Default::default()
            },
        ),
    ];
    variants
        .into_iter()
        .map(|(name, v)| {
            let (s, _) = humanoid(Variant::default(), true);
            let (t, expected) = humanoid(v, false);
            (
                Case {
                    name: name.into(),
                    expected,
                    review: if v.ambiguous { vec![7, 11] } else { vec![] },
                },
                import::from_bytes(&s.finish(), &Limits::default()).expect("source corpus"),
                import::from_bytes(&t.finish(), &Limits::default()).expect("target corpus"),
            )
        })
        .collect()
}
pub fn write_samples() -> anyhow::Result<()> {
    std::fs::create_dir_all("tests/fixtures/retarget")?;
    std::fs::write(
        "tests/fixtures/retarget/Multi-skin-wave-target.glb",
        crate::fixtures::multi_skin_wave_target().finish(),
    )?;
    for (name, v, motion) in [
        ("Walk-source", Variant::default(), true),
        (
            "Tall-target",
            Variant {
                renamed: true,
                proportions: true,
                helpers: true,
                ..Default::default()
            },
            false,
        ),
        (
            "Rest-frame-target",
            Variant {
                renamed: true,
                frames: true,
                ..Default::default()
            },
            false,
        ),
    ] {
        std::fs::write(
            format!("tests/fixtures/retarget/{name}.glb"),
            humanoid(v, motion).0.finish(),
        )?;
    }
    std::fs::create_dir_all("tests/corpus")?;
    std::fs::write(
        "tests/corpus/rig-manifest.json",
        serde_json::to_vec_pretty(&cases().into_iter().map(|x| x.0).collect::<Vec<_>>())?,
    )?;
    Ok(())
}

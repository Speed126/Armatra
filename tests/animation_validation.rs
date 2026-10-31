use armatra::{
    animation::{Clip, Interpolation, Property, Track},
    compression::{self, Codec, DerivedClip, TrackData},
    scene::{Asset, Local, Node, Trs},
};
use glam::{Quat, Vec3, Vec4};

fn fixture() -> Asset {
    let mut asset = Asset::default();
    asset.nodes.push(Node {
        name: "root".into(),
        parent: None,
        children: vec![],
        rest: Local::Trs(Trs::default()),
        mesh: None,
        skin: None,
    });
    asset.order.push(0);
    asset.clips.push(Clip {
        name: "mixed".into(),
        duration: 2.,
        supported: true,
        tracks: vec![
            Track {
                node: 0,
                property: Property::Translation,
                interpolation: Interpolation::Cubic,
                times: vec![0., 1., 2.],
                values: vec![
                    Vec4::ZERO,
                    Vec4::ZERO,
                    Vec4::X * 4.,
                    -Vec4::X,
                    Vec4::Y,
                    Vec4::X,
                    Vec4::Y,
                    Vec4::X * 2.,
                    Vec4::ZERO,
                ],
            },
            Track {
                node: 0,
                property: Property::Rotation,
                interpolation: Interpolation::Linear,
                times: vec![0., 1., 2.],
                values: vec![
                    Vec4::W,
                    Vec4::from_array(Quat::from_rotation_y(1.).to_array()),
                    -Vec4::W,
                ],
            },
            Track {
                node: 0,
                property: Property::Scale,
                interpolation: Interpolation::Step,
                times: vec![0., 0.413, 2.],
                values: vec![
                    Vec3::ONE.extend(0.),
                    Vec3::splat(2.).extend(0.),
                    Vec3::ONE.extend(0.),
                ],
            },
        ],
    });
    asset
}
#[test]
fn derived_roundtrip_step_cubic_and_random_seek() {
    let asset = fixture();
    let hash = [7; 32];
    for codec in [Codec::SampledF32, Codec::ReducedF32, Codec::Quantized(16)] {
        let encoded = compression::encode(&asset, &asset.clips[0], hash, 60, codec).unwrap();
        let bytes = encoded.to_bytes();
        let decoded = DerivedClip::from_bytes(&bytes, &asset, hash).unwrap();
        assert_eq!(decoded.to_bytes(), bytes);
        for i in 0..501 {
            let time = ((i * 97) % 501) as f32 / 250.;
            let raw = armatra::animation::evaluate(
                &asset,
                &asset.clips[0],
                &[Some(0)],
                time,
                &mut vec![],
            );
            let pose = decoded.evaluate(&asset, time);
            assert!((pose[0].matrix().w_axis - raw[0].matrix().w_axis).length() < 0.002);
            assert!((pose[0].matrix().x_axis - raw[0].matrix().x_axis).length() < 0.002);
        }
        for time in [0.413 - 1e-6, 0.413, 0.413 + 1e-6] {
            let Local::Trs(t) = decoded.evaluate(&asset, time)[0] else {
                panic!()
            };
            assert_eq!(
                t.scale,
                if time < 0.413 {
                    Vec3::ONE
                } else {
                    Vec3::splat(2.)
                }
            );
        }
    }
}
#[test]
fn derived_default_constant_and_stale_corrupt_data() {
    let mut asset = fixture();
    asset.clips[0].tracks[0].interpolation = Interpolation::Linear;
    asset.clips[0].tracks[0].values = vec![Vec4::ZERO; 3];
    asset.clips[0].tracks[1].values =
        vec![Vec4::from_array(Quat::from_rotation_x(0.4).to_array()); 3];
    let derived =
        compression::encode(&asset, &asset.clips[0], [0; 32], 30, Codec::Quantized(12)).unwrap();
    assert!(matches!(derived.tracks[0].data, TrackData::Default));
    assert!(matches!(derived.tracks[1].data, TrackData::Constant(_)));
    let bytes = derived.to_bytes();
    for i in 0..bytes.len() {
        assert!(DerivedClip::from_bytes(&bytes[..i], &asset, [0; 32]).is_err());
    }
    assert!(DerivedClip::from_bytes(&bytes, &asset, [1; 32]).is_err());
    let mut bad = bytes.clone();
    bad[4] = 99;
    assert!(DerivedClip::from_bytes(&bad, &asset, [0; 32]).is_err());
    for i in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[i] ^= 0xff;
        if let Ok(clip) = DerivedClip::from_bytes(&bad, &asset, [0; 32]) {
            assert!(
                clip.evaluate(&asset, 0.5)
                    .iter()
                    .all(|t| t.matrix().is_finite())
            );
        }
    }
}

#[test]
fn sampled_budgets_validate_final_data_and_reject_bad_deformation() {
    use armatra::compression_validation as cv;
    let asset = fixture();
    let clip = &asset.clips[0];
    let times = cv::sample_times(clip);
    assert!(times.contains(&0.413));
    assert!(times.contains(&0.413f32.next_down()));
    assert!(times.contains(&0.413f32.next_up()));
    let selected = cv::select(&asset, clip, [0; 32], 0.002).unwrap().unwrap();
    assert!(selected.1.accepted);
    let valid = cv::validate(&asset, clip, &selected.0, 0.002).unwrap();
    assert!(valid.accepted);
    let bad = cv::validate_with(&asset, clip, 0.002, |_, _| asset.rest_pose()).unwrap();
    assert!(!bad.accepted);
    assert!(bad.max_probe_error > 0.1);
    assert!(cv::validate(&asset, clip, &selected.0, f32::NAN).is_err());
    assert!(cv::select(&asset, clip, [0; 32], 0.).unwrap().is_none());
    let model = armatra::import::from_bytes(
        &armatra::fixtures::robot(false, false).finish(),
        &Default::default(),
    )
    .unwrap();
    assert!(cv::radii(&model).iter().all(|r| r.is_finite() && *r > 0.));
    let derived =
        compression::encode(&model, &model.clips[0], [0; 32], 60, Codec::Quantized(16)).unwrap();
    assert!(
        cv::validate(&model, &model.clips[0], &derived, 0.01)
            .unwrap()
            .vertex_probes_per_sample
            > 0
    );
}

#[test]
fn medoids_are_actual_frames_and_transport_preserves_mass() {
    use armatra::motion_search::{self as search, Medoid};
    let samples: Vec<_> = [0., 0.1, 0.2, 5., 5.1, 5.2].map(|v| vec![v]).into();
    let (indices, counts, cost) = search::medoids(&samples, 2).unwrap();
    assert_eq!(counts.iter().sum::<usize>(), 6);
    assert!(cost < 0.5);
    assert!(indices.iter().all(|i| *i < 6));
    let medoid = |x, weight| Medoid {
        time: 0.,
        weight,
        descriptor: vec![x],
    };
    let a = vec![medoid(0., 0.5), medoid(10., 0.5)];
    let b = vec![medoid(4., 0.75), medoid(12., 0.25)];
    let d = search::transport(&a, &b).unwrap();
    assert!((d - 4.).abs() < 1e-8);
    assert!((d - search::transport(&b, &a).unwrap()).abs() < 1e-8);
    assert!(search::transport(&a, &a).unwrap() < 1e-9);
    let mut bad = a.clone();
    bad[0].weight = 0.1;
    assert!(search::transport(&bad, &b).is_err());
}
#[test]
fn search_is_root_translation_invariant_and_rejects_unknown_rigs() {
    use armatra::motion_search as search;
    let (_, builder) = armatra::motion_fixtures::motions().remove(0);
    let asset = armatra::import::from_bytes(&builder.finish(), &Default::default()).unwrap();
    let a = search::summarize(&asset, &asset.clips[0]).unwrap();
    let mut moved = asset.clone();
    for track in &mut moved.clips[0].tracks {
        if track.node == 0 && track.property == Property::Translation {
            for v in &mut track.values {
                *v += Vec3::new(5., 0., -4.).extend(0.);
            }
        }
    }
    let b = search::summarize(&moved, &moved.clips[0]).unwrap();
    assert!(search::similarity(&a, &b).unwrap() < 1e-4);
    assert!((a.medoids.iter().map(|m| m.weight).sum::<f64>() - 1.).abs() < 1e-9);
    for node in &mut moved.nodes {
        node.name.clear();
    }
    assert!(search::summarize(&moved, &moved.clips[0]).is_err());
}

#[test]
fn cache_replay_respects_bytes_uses_costs_and_prefetch_has_no_future_oracle() {
    use armatra::cache_policies::*;
    let assets = [("A", 100.), ("B", 1.), ("C", 1.)]
        .map(|(name, cost)| AssetCost {
            key: name.into(),
            name: name.into(),
            cpu_bytes: 1,
            gpu_bytes: 0,
            decode_us: cost,
            upload_us: 0.,
        })
        .to_vec();
    let accesses = ["A", "B", "C", "A"]
        .map(|key| Access {
            key: key.into(),
            scenario: "test".into(),
            observed_cpu_hit: true,
            observed_gpu_hit: true,
            switch_us: 1.,
            cpu_resident_bytes: 0,
            gpu_resident_bytes: 0,
        })
        .to_vec();
    let trace = Trace {
        environment: serde_json::json!({}),
        assets,
        accesses,
    };
    let lru = replay(&trace, 2, Policy::ByteLru).unwrap();
    let gds = replay(&trace, 2, Policy::GreedyDualSize).unwrap();
    assert_eq!(lru.misses, 4);
    assert_eq!(gds.misses, 3);
    assert!(gds.peak_bytes <= 2 && lru.peak_bytes <= 2);
    assert!(gds.reload_work_us < lru.reload_work_us);
    let prefetch = replay(&trace, 2, Policy::LruPrefetch).unwrap();
    assert_eq!(prefetch.prefetch_issued, 0);
    let oversized = replay(&trace, 1, Policy::GreedyDualSize).unwrap();
    assert!(oversized.peak_bytes <= 1);
    assert!(replay(&trace, 0, Policy::ByteLru).is_err());
    let mut invalid = trace.clone();
    invalid.assets[0].decode_us = f64::NAN;
    assert!(replay(&invalid, 2, Policy::ByteLru).is_err());
}

#[test]
fn dqs_preserves_rigid_motion_signs_and_rejects_affine_frames() {
    use armatra::{scene::Vertex, skinning};
    use glam::Mat4;
    let mut v = Vertex {
        position: [0.4, 1., 0.],
        normal: [1., 0., 0.],
        uv: [0.; 2],
        joints: [0, 1, 0, 0, 0, 0, 0, 0],
        weights: [0.5, 0.5, 0., 0., 0., 0., 0., 0.],
    };
    let palette = [Mat4::IDENTITY, Mat4::from_rotation_y(3.)];
    let p = skinning::position(&v, &palette);
    assert!((Vec3::new(p.x, 0., p.z).length() - 0.4).abs() < 1e-6);
    let (r, d) = skinning::dual_quaternion(Mat4::from_rotation_translation(
        Quat::from_rotation_x(0.3),
        Vec3::new(1., 2., 3.),
    ))
    .unwrap();
    let a = skinning::blend(&v, &[(r, d), (-r, -d)]).unwrap();
    assert!((a.1 - Vec3::new(1., 2., 3.)).length() < 1e-5);
    assert!(a.0.dot(r).abs() > 0.99999);
    for scale in [
        Vec3::new(1., 2., 1.),
        Vec3::splat(2.),
        Vec3::new(-1., 1., 1.),
        Vec3::ZERO,
    ] {
        assert!(skinning::dual_quaternion(Mat4::from_scale(scale)).is_none());
    }
    v.weights.fill(0.);
    assert_eq!(skinning::position(&v, &palette), Vec3::from(v.position));
    let b = armatra::motion_fixtures::twist(true).finish();
    let asset = armatra::import::from_bytes(&b, &Default::default()).unwrap();
    let g = asset.globals(&asset.rest_pose());
    let p = asset.palette(2, &g).unwrap();
    assert!(skinning::packed_palette(&asset, 2, &g, &p).is_none());
}

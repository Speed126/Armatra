use armatra::{
    animation::{Interpolation, Property, Track},
    contacts, import, retarget, rig_corpus,
    scene::Local,
};
use glam::{Mat4, Vec3, Vec4};

#[test]
fn camera_frames_tiny_large_and_switched_models_at_portrait_and_wide_aspects() {
    let original = import::from_bytes(
        &armatra::fixtures::robot(false, false).finish(),
        &Default::default(),
    )
    .unwrap();
    let mut camera = armatra::render::Camera::default();
    for size in [1e-4, 1., 10000., 1e-4] {
        let mut asset = original.clone();
        for node in &mut asset.nodes {
            if node.parent.is_none() {
                node.rest = Local::Matrix(Mat4::from_scale(Vec3::splat(size)) * node.rest.matrix());
            }
        }
        for aspect in [0.4, 1., 2.5] {
            camera.fit_aspect(&asset, asset.default_scene, aspect);
            let (min, max) = asset.character_bounds(asset.default_scene).unwrap();
            for x in [min.x, max.x] {
                for y in [min.y, max.y] {
                    for z in [min.z, max.z] {
                        let p = camera.matrix(aspect) * Vec3::new(x, y, z).extend(1.);
                        let ndc = p.truncate() / p.w;
                        assert!(
                            ndc.is_finite()
                                && ndc.x.abs() < 0.96
                                && ndc.y.abs() < 0.96
                                && ndc.z > 0.
                                && ndc.z < 1.
                        );
                    }
                }
            }
            let before = (camera.target, camera.distance);
            camera.fit(&asset, asset.default_scene);
            assert_eq!(before, (camera.target, camera.distance));
            assert!(camera.distance / size < 20.);
        }
    }
}

#[test]
fn repeat_and_continuous_root_contacts_and_ik_are_stateless_over_ten_loops() {
    let (builder, _) = rig_corpus::humanoid(Default::default(), true);
    let mut source = import::from_bytes(&builder.finish(), &Default::default()).unwrap();
    let target = source.clone();
    source.clips[0]
        .tracks
        .retain(|t| !(t.node == 0 && t.property == Property::Translation));
    source.clips[0].tracks.push(Track {
        node: 0,
        property: Property::Translation,
        interpolation: Interpolation::Linear,
        times: vec![0., 2.],
        values: vec![Vec4::ZERO, Vec4::new(1., -0.8, 0.3, 0.)],
    });
    let clip = &source.clips[0];
    for policy in [
        contacts::LoopPolicy::Repeat,
        contacts::LoopPolicy::Accumulate,
    ] {
        let mut settings = retarget::Settings::default();
        settings.contacts.looping = policy.clone();
        let plan = retarget::compile(&source, &target, &settings, &[]).unwrap();
        let c = contacts::compile(&source, &target, clip, &plan, Mat4::IDENTITY).unwrap();
        let displacement = Vec3::from_array(c.cycle_displacement);
        if policy == contacts::LoopPolicy::Repeat {
            assert_eq!(displacement, Vec3::ZERO);
        } else {
            assert!(displacement.y < -0.1);
        }
        for local in [0., 0.125, 0.5, 1.75, 1.999] {
            let baseline = c
                .raw_pose_checked(&source, &target, clip, &plan, local, true)
                .unwrap();
            let origin = target.globals(&baseline)[plan.motion_target]
                .w_axis
                .truncate();
            let corrected = armatra::ik::sample(&source, &target, clip, &plan, &c, local, true);
            let anchors = c.active(local, true);
            for cycle in [0, 1, 2, 5, 10, -2, 7, 0] {
                let time = local + cycle as f64 * clip.duration as f64;
                let pose = c
                    .raw_pose_checked(&source, &target, clip, &plan, time, true)
                    .unwrap();
                let root = target.globals(&pose)[plan.motion_target].w_axis.truncate();
                assert!((root - origin).abs_diff_eq(displacement * cycle as f32, 2e-5));
                let active = c.active(time, true);
                for (a, b) in anchors.iter().zip(&active) {
                    assert!((b.anchor - a.anchor).abs_diff_eq(displacement * cycle as f32, 2e-5));
                }
                let ik = armatra::ik::sample(&source, &target, clip, &plan, &c, time, true);
                for (node, (a, b)) in target
                    .globals(&corrected.pose)
                    .iter()
                    .zip(target.globals(&ik.pose))
                    .enumerate()
                {
                    let mut ancestor = Some(node);
                    let mut moved = false;
                    while let Some(n) = ancestor {
                        if n == plan.motion_target {
                            moved = true;
                            break;
                        }
                        ancestor = target.nodes[n].parent;
                    }
                    let expected = if moved {
                        displacement * cycle as f32
                    } else {
                        Vec3::ZERO
                    };
                    assert!(
                        (b.w_axis.truncate() - a.w_axis.truncate()).abs_diff_eq(expected, 3e-4),
                        "node {node}, cycle {cycle}, policy {policy:?}"
                    );
                }
            }
        }
        for cycle in -2..=10 {
            let t = contacts::time(cycle as f64 * 2., 2., true);
            assert_eq!(t.local, 0.);
            assert_eq!(t.cycle, cycle);
        }
        // A different clip is evaluated solely from its data, never the last cycle.
        let mut next = clip.clone();
        for t in &mut next.tracks {
            if t.node == 0 && t.property == Property::Translation {
                t.values.fill(Vec4::ZERO);
            }
        }
        let next_contacts =
            contacts::compile(&source, &target, &next, &plan, Mat4::IDENTITY).unwrap();
        assert_eq!(next_contacts.cycle_displacement, [0.; 3]);
    }
}

#[test]
fn native_root_travel_and_contact_anchors_share_explicit_loop_policy() {
    let (builder, _) = rig_corpus::humanoid(Default::default(), true);
    let source = import::from_bytes(&builder.finish(), &Default::default()).unwrap();
    let identity: Vec<_> = (0..source.nodes.len()).map(Some).collect();
    let travel = armatra::animation::root_travel(&source, &source.clips[0], &identity).unwrap();
    assert!(travel.displacement.length() > 0.1);
    for cycle in [-2, 0, 1, 10, 0] {
        let mut pose =
            armatra::animation::evaluate(&source, &source.clips[0], &identity, 0.5, &mut vec![]);
        let before = source.globals(&pose)[travel.node].w_axis.truncate();
        armatra::animation::place_cycle(&source, &mut pose, Some(travel), cycle);
        assert!(
            (source.globals(&pose)[travel.node].w_axis.truncate() - before)
                .abs_diff_eq(travel.displacement * cycle as f32, 1e-5)
        );
    }
    for policy in [
        contacts::LoopPolicy::Repeat,
        contacts::LoopPolicy::Accumulate,
    ] {
        let mut settings = retarget::Settings::default();
        settings.contacts.looping = policy;
        let plan = retarget::compile(&source, &source, &settings, &[]).unwrap();
        let c =
            contacts::compile(&source, &source, &source.clips[0], &plan, Mat4::IDENTITY).unwrap();
        assert!(!c.intervals.is_empty());
        for interval in &c.intervals {
            let phase = (interval.start + interval.end) as f64 * 0.5;
            let first = c.active(phase, true);
            let first_ik =
                armatra::ik::sample(&source, &source, &source.clips[0], &plan, &c, phase, true);
            for cycle in [-1, 0, 1, 10, 0] {
                let time = phase + cycle as f64 * source.clips[0].duration as f64;
                let next = c.active(time, true);
                for (a, b) in first.iter().zip(&next) {
                    assert!(
                        (b.anchor - a.anchor).abs_diff_eq(
                            Vec3::from_array(c.cycle_displacement) * cycle as f32,
                            2e-5
                        )
                    );
                }
                let next_ik =
                    armatra::ik::sample(&source, &source, &source.clips[0], &plan, &c, time, true);
                let a = source.globals(&first_ik.pose)[plan.motion_target]
                    .w_axis
                    .truncate();
                let b = source.globals(&next_ik.pose)[plan.motion_target]
                    .w_axis
                    .truncate();
                assert!(
                    (b - a)
                        .abs_diff_eq(Vec3::from_array(c.cycle_displacement) * cycle as f32, 3e-4)
                );
            }
        }
    }
    let mut playback = armatra::animation::Playback {
        time: 0.5,
        cycle: 10,
        playing: false,
        ..Default::default()
    };
    playback.tick(100., 2.);
    assert_eq!((playback.time, playback.cycle), (0.5, 10));
    playback.playing = true;
    playback.speed = -1.;
    playback.tick(20.25, 2.);
    assert_eq!((playback.time, playback.cycle), (0.25, 0));
}

#[test]
fn continuous_vertical_travel_moves_ik_ground_reference_with_contact_anchors() {
    let (builder, _) = rig_corpus::humanoid(Default::default(), true);
    let source = import::from_bytes(&builder.finish(), &Default::default()).unwrap();
    let mut settings = retarget::Settings::default();
    settings.contacts.looping = contacts::LoopPolicy::Accumulate;
    let plan = retarget::compile(&source, &source, &settings, &[]).unwrap();
    let mut c =
        contacts::compile(&source, &source, &source.clips[0], &plan, Mat4::IDENTITY).unwrap();
    assert!(!c.intervals.is_empty());
    // Isolate cycle placement from contact detection: these already validated
    // contacts travel down as well as forward in explicit continuous mode.
    c.cycle_displacement = [0.2, -0.8, 0.3];
    for interval in &c.intervals {
        let phase = (interval.start + interval.end) as f64 * 0.5;
        let first = armatra::ik::sample(&source, &source, &source.clips[0], &plan, &c, phase, true);
        assert!(!first.feet.is_empty());
        let original = source.globals(&first.pose);
        for cycle in [-2, 0, 1, 5, 10, 0] {
            let time = phase + cycle as f64 * source.clips[0].duration as f64;
            let next =
                armatra::ik::sample(&source, &source, &source.clips[0], &plan, &c, time, true);
            let actual = source.globals(&next.pose);
            let expected = Vec3::from_array(c.cycle_displacement) * cycle as f32;
            for joint in [plan.motion_target, interval.target] {
                assert!(
                    (actual[joint].w_axis.truncate() - original[joint].w_axis.truncate())
                        .abs_diff_eq(expected, 3e-4),
                    "joint {joint}, cycle {cycle}"
                );
            }
            assert!(next.pelvis_delta.abs_diff_eq(first.pelvis_delta, 3e-4));
        }
    }
}

#[test]
fn frame_character_bounds_follow_the_evaluated_skinned_pose() {
    let asset = import::from_bytes(
        &armatra::fixtures::robot(false, false).finish(),
        &Default::default(),
    )
    .unwrap();
    let mut pose = asset.rest_pose();
    let offset = Vec3::new(50., -100., 12.);
    if let Local::Trs(root) = &mut pose[0] {
        root.translation += offset;
    } else {
        panic!("TRS root")
    }
    let (rest_min, rest_max) = asset.character_bounds(asset.default_scene).unwrap();
    let (min, max) = asset
        .character_pose_bounds(asset.default_scene, &pose)
        .unwrap();
    assert!(min.abs_diff_eq(rest_min + offset, 1e-4) && max.abs_diff_eq(rest_max + offset, 1e-4));
    let mut camera = armatra::render::Camera::default();
    camera.fit_bounds(min, max, 0.6);
    assert!(
        camera
            .target
            .abs_diff_eq((rest_min + rest_max) * 0.5 + offset, 1e-4)
    );
    for p in [min, max] {
        let p = camera.matrix(0.6) * p.extend(1.);
        let ndc = p.truncate() / p.w;
        assert!(ndc.x.abs() < 0.96 && ndc.y.abs() < 0.96);
    }
}

use armatra::{
    rig::{self, Anatomy, Rig, Side},
    rig_corpus,
};

#[test]
fn partial_assignment_matches_exhaustive_reference_and_pays_unmatched() {
    use armatra::assignment::{AssignmentProblem, AssignmentSolver, ShortestPath};
    fn exhaustive(p: &AssignmentProblem, i: usize, used: &mut Vec<usize>) -> f64 {
        if i == p.costs.len() {
            return 0.;
        }
        let mut best = p.unmatched[i] + exhaustive(p, i + 1, used);
        for (j, c) in p.costs[i].iter().enumerate() {
            if let Some(c) = c
                && !used.contains(&j)
            {
                used.push(j);
                best = best.min(c + exhaustive(p, i + 1, used));
                used.pop();
            }
        }
        best
    }
    for seed in 0..80usize {
        let n = seed % 4 + 1;
        let m = (seed / 4) % 4;
        let p = AssignmentProblem {
            costs: (0..n)
                .map(|i| {
                    (0..m)
                        .map(|j| {
                            if (seed + i + j).is_multiple_of(5) {
                                None
                            } else {
                                Some(((seed * 3 + i * 7 + j * 11) % 31) as f64 / 8. - 0.5)
                            }
                        })
                        .collect()
                })
                .collect(),
            unmatched: (0..n).map(|i| 0.5 + (i % 3) as f64).collect(),
        };
        let a = ShortestPath.solve(&p).unwrap();
        assert!((a.cost - exhaustive(&p, 0, &mut vec![])).abs() < 1e-9);
        let unique: std::collections::HashSet<_> = a.targets.iter().flatten().collect();
        assert_eq!(unique.len(), a.targets.iter().flatten().count());
        assert_eq!(a.targets.len(), n);
    }
    let p = AssignmentProblem {
        costs: vec![vec![Some(1.), None], vec![None, Some(1.)]],
        unmatched: vec![5., 5.],
    };
    assert_eq!(
        ShortestPath.solve(&p).unwrap().targets,
        vec![Some(0), Some(1)]
    );
    let p = AssignmentProblem {
        costs: vec![vec![None]],
        unmatched: vec![2.],
    };
    assert_eq!(ShortestPath.solve(&p).unwrap().cost, 2.);
    let p = AssignmentProblem {
        costs: vec![vec![Some(f64::NAN)]],
        unmatched: vec![2.],
    };
    assert!(ShortestPath.solve(&p).is_err());
}
#[test]
fn weighted_mapping_improves_names_and_never_crosses_known_sides() {
    let mut greedy = 0;
    let mut weighted = 0;
    let mut total = 0;
    for (case, s, t) in rig_corpus::cases() {
        let s = Rig::analyze(&s);
        let t = Rig::analyze(&t);
        let g = armatra::assignment::greedy_names(&s, &t);
        let a = armatra::assignment::weighted(&s, &t).unwrap();
        for (i, j) in a.targets.iter().enumerate() {
            let truth = case.expected[s.joints[i].node];
            total += 1;
            greedy += usize::from(g[i].map(|j| t.joints[j].node) == truth);
            weighted += usize::from(j.map(|j| t.joints[j].node) == truth);
            if let Some(j) = j {
                assert!(!matches!(
                    (s.joints[i].side, t.joints[*j].side),
                    (Side::Left, Side::Right) | (Side::Right, Side::Left)
                ));
            }
        }
    }
    eprintln!("correct rows: greedy={greedy}, weighted={weighted}, total={total}");
    assert!(weighted > greedy);
}
#[test]
fn corpus_descriptors_are_finite_and_identity_independent() {
    for (case, source, target) in rig_corpus::cases() {
        let s = Rig::analyze(&source);
        let t = Rig::analyze(&target);
        assert!(s.frame.is_some(), "{}", case.name);
        for d in s.joints.iter().chain(&t.joints) {
            assert!(d.position.is_finite() && d.direction.is_finite());
            assert!(d.rest_rotation.is_finite() && d.depth <= 1.);
        }
        if case.name == "permuted" || case.name == "renamed" || case.name == "rest-frames" {
            for (i, j) in case.expected.iter().enumerate() {
                let a = s.joints.iter().find(|x| x.node == i).unwrap();
                let b = t.joints.iter().find(|x| Some(x.node) == *j).unwrap();
                assert!(
                    a.position.abs_diff_eq(b.position, 1e-5),
                    "{} {i}",
                    case.name
                );
            }
        }
        if case.name == "nonuniform-scale" {
            assert!(t.joints.iter().any(|d| !d.rigid));
        }
    }
}

#[test]
fn chain_alignment_is_monotone_and_skips_twists() {
    let costs = vec![vec![Some(0.), None, None], vec![None, None, Some(0.)]];
    assert_eq!(
        armatra::correspondence::align_chains(&costs, &[2., 2.], &[1., 0.01, 1.]),
        vec![(0, 0), (1, 2)]
    );
    assert_eq!(
        armatra::correspondence::align_chains(&[], &[], &[1.]),
        vec![]
    );
    // Crossing cheap edges cannot produce a crossing chain.
    let costs = vec![vec![Some(3.), Some(0.)], vec![Some(0.), Some(3.)]];
    let pairs = armatra::correspondence::align_chains(&costs, &[1., 1.], &[1., 1.]);
    assert!(pairs.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1));
}
#[test]
fn refinement_terminates_and_retains_lowest_full_energy() {
    for (case, s, t) in rig_corpus::cases() {
        let sr = Rig::analyze(&s);
        let tr = Rig::analyze(&t);
        let r = armatra::correspondence::refine(&sr, &tr).unwrap();
        assert!(r.iterations <= armatra::correspondence::MAX_REFINEMENT);
        assert!(
            r.assignment.cost <= r.energies.iter().copied().fold(f64::INFINITY, f64::min) + 1e-9
        );
        assert_eq!(
            armatra::correspondence::violations(&sr, &tr, &r.assignment.targets),
            0,
            "{}",
            case.name
        );
        for (i, j) in r.assignment.targets.iter().enumerate() {
            assert_eq!(
                j.map(|j| tr.joints[j].node),
                case.expected[sr.joints[i].node],
                "{} {i}",
                case.name
            );
        }
        let again = armatra::correspondence::refine(&sr, &tr).unwrap();
        assert_eq!(r.assignment, again.assignment);
    }
}

#[test]
fn rest_transfer_and_walk_are_correct_under_different_joint_frames() {
    use armatra::{
        retarget::{self, Settings},
        scene::Local,
    };
    for (case, s, t) in rig_corpus::cases() {
        if case.name == "nonuniform-scale" {
            assert!(retarget::compile(&s, &t, &Settings::default(), &[]).is_err());
            continue;
        }
        let plan = retarget::compile(&s, &t, &Settings::default(), &[]).unwrap();
        let rest = retarget::transfer(&s, &t, &s.rest_pose(), &plan);
        for (actual, expected) in rest.iter().zip(t.rest_pose()) {
            assert!(
                actual.matrix().abs_diff_eq(expected.matrix(), 1e-4),
                "{} rest",
                case.name
            );
        }
        for time in [0.1, 0.51, 1.4, 1.99] {
            let pose = retarget::sample(&s, &t, &s.clips[0], &plan, time);
            for (i, p) in pose.iter().enumerate() {
                if let Local::Trs(p) = p {
                    assert!(
                        p.rotation.is_finite() && (p.rotation.length_squared() - 1.).abs() < 1e-5
                    );
                    if i != plan.motion_target
                        && let Local::Trs(rest) = t.nodes[i].rest
                    {
                        assert!(p.translation.abs_diff_eq(rest.translation, 1e-6));
                        assert_eq!(p.scale, rest.scale);
                    }
                }
            }
            if ["renamed", "permuted", "rest-frames", "twist-spine-helpers"]
                .contains(&case.name.as_str())
            {
                let identity: Vec<_> = (0..s.nodes.len()).map(Some).collect();
                let source =
                    armatra::animation::evaluate(&s, &s.clips[0], &identity, time, &mut Vec::new());
                let sg = s.globals(&source);
                let tg = t.globals(&pose);
                for (i, j) in case.expected.iter().enumerate().skip(1) {
                    if let Some(j) = j {
                        assert!(
                            sg[i]
                                .w_axis
                                .truncate()
                                .abs_diff_eq(tg[*j].w_axis.truncate(), 2e-4),
                            "{} {i} at {time}: {:?} {:?}",
                            case.name,
                            sg[i].w_axis,
                            tg[*j].w_axis
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn twist_subdivision_preserves_endpoint_and_collapses_source_helpers() {
    use armatra::{
        retarget::{self, Settings},
        scene::Local,
    };
    use glam::{Quat, Vec3};
    let (_, s, t) = rig_corpus::cases()
        .into_iter()
        .find(|x| x.0.name == "twist-spine-helpers")
        .unwrap();
    let plan = retarget::compile(&s, &t, &Settings::default(), &[]).unwrap();
    assert!(!plan.chains.is_empty());
    let mut sp = s.rest_pose();
    if let Local::Trs(r) = &mut sp[8] {
        r.rotation = Quat::from_rotation_x(1.4);
    }
    let tp = retarget::transfer(&s, &t, &sp, &plan);
    let sg = s.globals(&sp);
    let tg = t.globals(&tp);
    assert!(armatra::rig::rotation(sg[9]).abs_diff_eq(armatra::rig::rotation(tg[9]), 1e-4));
    for chain in &plan.chains {
        assert!((chain.weights.iter().sum::<f32>() - 1.).abs() < 1e-5);
    }
    let back = retarget::compile(&t, &s, &Settings::default(), &[]).unwrap();
    let mut pose = t.rest_pose();
    let helper = t
        .nodes
        .iter()
        .position(|n| n.name == "SpineHelper")
        .unwrap();
    if let Local::Trs(r) = &mut pose[helper] {
        r.rotation = Quat::from_rotation_z(0.3);
    }
    let out = retarget::transfer(&t, &s, &pose, &back);
    assert!(
        armatra::rig::rotation(s.globals(&out)[3]).abs_diff_eq(Quat::from_rotation_z(0.3), 1e-4)
    );
    let (swing, twist) = retarget::swing_twist(
        Quat::from_rotation_y(0.4) * Quat::from_rotation_x(1.2),
        Vec3::X,
    );
    assert!((swing * twist).abs_diff_eq(
        Quat::from_rotation_y(0.4) * Quat::from_rotation_x(1.2),
        1e-5
    ));
}
#[test]
fn retarget_plan_keys_and_root_policies_are_explicit() {
    use armatra::retarget::{self, RootPolicy, Settings};
    let (_, s, t) = rig_corpus::cases()
        .into_iter()
        .find(|x| x.0.name == "proportions")
        .unwrap();
    let p = retarget::compile(&s, &t, &Settings::default(), &[]).unwrap();
    let pose = retarget::sample(&s, &t, &s.clips[0], &p, 0.5);
    let g = t.globals(&pose);
    let r = t.globals(&t.rest_pose());
    assert!(
        ((g[p.motion_target].w_axis.z - r[p.motion_target].w_axis.z) - 0.1 * p.scale).abs() < 1e-5
    );
    let settings = Settings {
        root: RootPolicy::InPlace,
        ..Default::default()
    };
    let q = retarget::compile(&s, &t, &settings, &[]).unwrap();
    assert_ne!(p.key, q.key);
    let pose = retarget::sample(&s, &t, &s.clips[0], &q, 0.5);
    assert!(
        t.globals(&pose)[q.motion_target]
            .w_axis
            .abs_diff_eq(r[q.motion_target].w_axis, 1e-5)
    );
    let mut unknown = s.clone();
    for n in &mut unknown.nodes {
        n.name.clear()
    }
    assert!(retarget::compile(&unknown, &t, &Settings::default(), &[]).is_err());
}

#[test]
fn manual_profiles_survive_restart_recover_corruption_and_invalidate_stale_keys() {
    use armatra::{
        library::Library,
        profile,
        retarget::{self, Confidence, Override, Settings},
    };
    let root = std::path::PathBuf::from(format!(
        "target/test-profiles/{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let (_, s, t) = rig_corpus::cases()
        .into_iter()
        .find(|x| x.0.name == "ambiguous")
        .unwrap();
    let library = Library::open(root.clone()).unwrap();
    let p = retarget::compile(
        &s,
        &t,
        &Settings::default(),
        &[Override {
            source: 7,
            target: Some(7),
        }],
    )
    .unwrap();
    assert_eq!(
        p.rows.iter().find(|r| r.source == 7).unwrap().confidence,
        Confidence::Manual
    );
    profile::save(&library.root, &p, &s, &t).unwrap();
    drop(library);
    let library = Library::open(root).unwrap();
    let q = profile::load(&library.root, &p.key, &s, &t)
        .unwrap()
        .unwrap();
    assert_eq!(q.overrides, p.overrides);
    let path = profile::save(&library.root, &p, &s, &t).unwrap();
    std::fs::write(path, b"corrupt").unwrap();
    let q = profile::load(&library.root, &p.key, &s, &t)
        .unwrap()
        .unwrap();
    assert!(q.warnings.iter().any(|w| w.contains("Recovered")));
    let mut stale = q.clone();
    stale.key.algorithm += 1;
    assert!(profile::validate(&stale, &s, &t).is_err());
    let mut changed = t.clone();
    changed.nodes[7].name.push_str("changed");
    assert!(profile::validate(&q, &s, &changed).is_err());
    assert!(
        retarget::compile(
            &s,
            &t,
            &Settings::default(),
            &[
                Override {
                    source: 7,
                    target: Some(7)
                },
                Override {
                    source: 11,
                    target: Some(7)
                }
            ]
        )
        .is_err()
    );
    let unmapped = retarget::compile(
        &s,
        &t,
        &Settings::default(),
        &[Override {
            source: 7,
            target: None,
        }],
    )
    .unwrap();
    assert!(
        unmapped
            .rows
            .iter()
            .any(|r| r.source == 7 && r.target.is_none())
    );
}

#[test]
fn contacts_have_target_world_anchors_and_deterministic_cycles() {
    use armatra::{
        contacts,
        retarget::{self, Settings},
    };
    use glam::{Mat4, Quat, Vec3};
    let (_, s, t) = rig_corpus::cases()
        .into_iter()
        .find(|x| x.0.name == "proportions")
        .unwrap();
    let plan = retarget::compile(&s, &t, &Settings::default(), &[]).unwrap();
    let placement =
        Mat4::from_rotation_translation(Quat::from_rotation_y(0.7), Vec3::new(7., 1., -3.));
    let a = contacts::compile(&s, &t, &s.clips[0], &plan, placement).unwrap();
    let b = contacts::compile(&s, &t, &s.clips[0], &plan, placement).unwrap();
    assert!(!a.intervals.is_empty());
    assert_eq!(a.intervals, b.intervals);
    for i in &a.intervals {
        assert!(i.end > i.start);
        let onset = retarget::sample(&s, &t, &s.clips[0], &plan, i.start);
        let mut expected =
            placement.transform_point3(t.globals(&onset)[i.target].w_axis.truncate());
        let n = Vec3::from_array(a.target_normal);
        expected -= n * (n.dot(expected) - a.target_height);
        assert!(expected.abs_diff_eq(Vec3::from_array(i.anchor), 1e-5));
        assert!(Vec3::from_array(i.anchor).distance(Vec3::from_array(i.source_point)) > 4.);
        let mid = (i.start + i.end) * 0.5;
        let direct = a.active(mid as f64, true);
        for k in 0..100 {
            let _ = a.active(k as f64 * mid as f64 / 100., true);
        }
        assert_eq!(direct, a.active(mid as f64, true));
        let next = a.active(mid as f64 + 2., true);
        let first = direct.iter().find(|c| c.target == i.target).unwrap();
        let second = next.iter().find(|c| c.target == i.target).unwrap();
        assert!(
            (second.anchor - first.anchor)
                .abs_diff_eq(Vec3::from_array(a.cycle_displacement), 1e-5)
        );
        assert_eq!(second.cycle, 1);
        assert!(
            a.active(i.start as f64, true)
                .iter()
                .filter(|c| c.target == i.target)
                .all(|c| c.weight < 1e-5)
        );
    }
    for time in [-4.3, -0.3, 0.3, 2.3, 6.3, 1.1, 0.3] {
        let time = contacts::time(time, 2., true);
        assert!((0.0..2.).contains(&time.local));
    }
    let first = a.raw_pose(&s, &t, &s.clips[0], &plan, 0.5, true);
    let next = a.raw_pose(&s, &t, &s.clips[0], &plan, 2.5, true);
    let scene_delta = placement
        .inverse()
        .transform_vector3(Vec3::from_array(a.cycle_displacement));
    assert!(
        (t.globals(&next)[plan.motion_target].w_axis.truncate()
            - t.globals(&first)[plan.motion_target].w_axis.truncate())
        .abs_diff_eq(scene_delta, 1e-5)
    );
}
#[test]
fn contact_cache_keys_cover_clip_plan_settings_placement_and_versions() {
    use armatra::{
        contacts,
        retarget::{self, RootPolicy, Settings},
    };
    use glam::{Mat4, Vec3};
    let (_, s, t) = rig_corpus::cases().remove(0);
    let p = retarget::compile(&s, &t, &Settings::default(), &[]).unwrap();
    let key = contacts::identity(&s.clips[0], &p, Mat4::IDENTITY);
    let mut clip = s.clips[0].clone();
    clip.tracks[0].values[0].z += 1.;
    assert_ne!(key, contacts::identity(&clip, &p, Mat4::IDENTITY));
    assert_ne!(
        key,
        contacts::identity(&s.clips[0], &p, Mat4::from_translation(Vec3::X))
    );
    let q = retarget::compile(
        &s,
        &t,
        &Settings {
            root: RootPolicy::InPlace,
            ..Default::default()
        },
        &[],
    )
    .unwrap();
    assert_ne!(key, contacts::identity(&s.clips[0], &q, Mat4::IDENTITY));
    let root = std::path::PathBuf::from(format!("target/test-contacts/{}", std::process::id()));
    let a = contacts::cached(&root, &s, &t, &s.clips[0], &p, Mat4::IDENTITY).unwrap();
    let b = contacts::cached(&root, &s, &t, &s.clips[0], &p, Mat4::IDENTITY).unwrap();
    assert_eq!(a.intervals, b.intervals);
    let mut disabled = p.clone();
    disabled.settings.contacts.ground = contacts::Ground::Disabled;
    disabled.settings.contacts.looping = contacts::LoopPolicy::Accumulate;
    let unlocked = contacts::compile(&s, &t, &s.clips[0], &disabled, Mat4::IDENTITY).unwrap();
    assert!(
        glam::Vec3::from_array(unlocked.cycle_displacement).length() > 0.1,
        "Disabling contacts must retain locomotion loop displacement"
    );
    assert!(
        contacts::compile(&s, &t, &s.clips[0], &disabled, Mat4::IDENTITY)
            .unwrap()
            .intervals
            .is_empty()
    );
    let mut bad = p;
    bad.settings.contacts.sample_rate = 0.;
    assert!(contacts::compile(&s, &t, &s.clips[0], &bad, Mat4::IDENTITY).is_err());
}

#[test]
fn analytical_ik_reaches_goals_preserves_lengths_and_clamps_unreachable() {
    use armatra::ik;
    use glam::{Quat, Vec3};
    for i in 0..150 {
        let a = Vec3::new(i as f32 * 0.02, -0.4, 0.7);
        let q = Quat::from_rotation_y(i as f32 * 0.13) * Quat::from_rotation_z(0.7);
        let b = a + q * Vec3::new(0.1, -0.5, 0.1);
        let c = b + q * Vec3::new(-0.1, -0.45, 0.1);
        let goal = a + q * Vec3::new(0.2, -0.6, 0.15);
        let solved = ik::solve(a, b, c, goal, b).unwrap();
        assert!(solved.end.distance(goal) < 1e-5);
        assert!(!solved.unreachable);
        assert!((a.distance(solved.middle) - a.distance(b)).abs() < 1e-5);
        assert!((solved.middle.distance(solved.end) - b.distance(c)).abs() < 1e-5);
        let far = ik::solve(a, b, c, a + q * Vec3::Y * 10., b).unwrap();
        assert!(far.unreachable && far.residual > 8.);
        assert!(far.middle.is_finite() && far.end.is_finite());
        assert!((a.distance(far.middle) - a.distance(b)).abs() < 1e-5);
        assert!((far.middle.distance(far.end) - b.distance(c)).abs() < 1e-5);
        let folded = ik::solve(a, b, c, a, b).unwrap();
        assert!(folded.middle.is_finite() && folded.end.is_finite());
    }
    assert!(ik::solve(Vec3::ZERO, Vec3::ZERO, Vec3::Y, Vec3::X, Vec3::Z).is_err());
    assert!(ik::solve(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::NAN, Vec3::Z).is_err());
}
#[test]
fn contact_ik_improves_constraints_is_bounded_and_seek_independent() {
    use armatra::{
        contacts, ik,
        retarget::{self, Settings},
        scene::Local,
    };
    use glam::{Mat4, Vec3};
    let (_, s, t) = rig_corpus::cases()
        .into_iter()
        .find(|x| x.0.name == "proportions")
        .unwrap();
    let plan = retarget::compile(&s, &t, &Settings::default(), &[]).unwrap();
    let c = contacts::compile(&s, &t, &s.clips[0], &plan, Mat4::IDENTITY).unwrap();
    let (mut raw_error, mut fixed_error, mut samples) = (0., 0., 0);
    let mut previous = std::collections::HashMap::<usize, Vec3>::new();
    let mut jump = 0f32;
    for k in 0..=480 {
        let time = k as f64 / 240.;
        let raw = c.raw_pose(&s, &t, &s.clips[0], &plan, time, true);
        let corrected = ik::sample(&s, &t, &s.clips[0], &plan, &c, time, true);
        let rg = t.globals(&raw);
        let cg = t.globals(&corrected.pose);
        assert!(
            corrected.pelvis_delta.length()
                <= plan.target_height * plan.settings.ik.max_pelvis_height + 1e-5
        );
        for (i, p) in corrected.pose.iter().enumerate() {
            if let Local::Trs(p) = p {
                assert!(p.rotation.is_finite() && (p.rotation.length_squared() - 1.).abs() < 1e-5);
                if i != plan.motion_target
                    && let Local::Trs(rest) = t.nodes[i].rest
                {
                    assert_eq!(p.translation, rest.translation);
                    assert_eq!(p.scale, rest.scale);
                }
            }
        }
        for foot in [16, 20] {
            let offset = cg[foot].w_axis.truncate() - rg[foot].w_axis.truncate();
            if let Some(before) = previous.insert(foot, offset) {
                jump = jump.max(before.distance(offset));
            }
        }
        for active in c.active(time, true) {
            if active.weight > 0.999 {
                raw_error += rg[active.target]
                    .w_axis
                    .truncate()
                    .distance_squared(active.anchor);
                fixed_error += cg[active.target]
                    .w_axis
                    .truncate()
                    .distance_squared(active.anchor);
                samples += 1;
            }
        }
        if c.active(time, true).is_empty() {
            for (a, b) in raw.iter().zip(&corrected.pose) {
                assert!(a.matrix().abs_diff_eq(b.matrix(), 1e-6));
            }
        }
    }
    eprintln!(
        "contact constraints raw={raw_error} fixed={fixed_error} count={samples} transition_jump={jump}"
    );
    assert!(samples > 20 && fixed_error < raw_error * 0.15);
    assert!(jump < plan.target_height * 0.04);
    for time in [0.6, 3.4, -0.4, 1.8, 0.1, 0.6] {
        let first = ik::sample(&s, &t, &s.clips[0], &plan, &c, time, true);
        let second = ik::sample(&s, &t, &s.clips[0], &plan, &c, time, true);
        for (a, b) in first.pose.iter().zip(second.pose) {
            assert!(a.matrix().abs_diff_eq(b.matrix(), 1e-6));
        }
    }
}

#[test]
fn ik_correction_is_covariant_under_world_placement() {
    use armatra::{
        contacts, ik,
        retarget::{self, Settings},
    };
    use glam::{Mat4, Quat, Vec3};
    let (_, s, t) = rig_corpus::cases()
        .into_iter()
        .find(|x| x.0.name == "proportions")
        .unwrap();
    let p = retarget::compile(&s, &t, &Settings::default(), &[]).unwrap();
    let a = contacts::compile(&s, &t, &s.clips[0], &p, Mat4::IDENTITY).unwrap();
    let placement = Mat4::from_scale_rotation_translation(
        Vec3::splat(1.7),
        Quat::from_rotation_y(0.8),
        Vec3::new(7., 2., -4.),
    );
    let b = contacts::compile(&s, &t, &s.clips[0], &p, placement).unwrap();
    let pa = ik::sample(&s, &t, &s.clips[0], &p, &a, 0.5, true);
    let pb = ik::sample(&s, &t, &s.clips[0], &p, &b, 0.5, true);
    for (a, b) in pa.pose.iter().zip(pb.pose) {
        assert!(a.matrix().abs_diff_eq(b.matrix(), 2e-4));
    }
}

#[test]
fn animated_singular_or_nonuniform_source_frames_are_diagnosed_with_finite_rest_fallback() {
    use armatra::{
        animation::{Interpolation, Property, Track},
        contacts, ik,
        retarget::{self, Settings},
    };
    use glam::{Mat4, Vec4};
    let (_, mut s, t) = rig_corpus::cases().remove(0);
    s.clips[0].tracks.push(Track {
        node: 0,
        property: Property::Scale,
        interpolation: Interpolation::Linear,
        times: vec![0., 2.],
        values: vec![Vec4::ONE, Vec4::new(0., 2., 1., 0.)],
    });
    let p = retarget::compile(&s, &t, &Settings::default(), &[]).unwrap();
    assert!(retarget::sample_checked(&s, &t, &s.clips[0], &p, 1.).is_err());
    assert!(contacts::compile(&s, &t, &s.clips[0], &p, Mat4::IDENTITY).is_err());
    for time in [1., 2.] {
        let pose = retarget::sample(&s, &t, &s.clips[0], &p, time);
        for (a, b) in pose.iter().zip(t.rest_pose()) {
            assert!(a.matrix().is_finite() && a.matrix().abs_diff_eq(b.matrix(), 1e-6));
        }
    }
    let mut disabled = p;
    disabled.settings.contacts.ground = contacts::Ground::Disabled;
    let c = contacts::compile(&s, &t, &s.clips[0], &disabled, Mat4::IDENTITY).unwrap();
    let output = ik::sample(&s, &t, &s.clips[0], &disabled, &c, 2., false);
    assert!(!output.warnings.is_empty());
    assert!(output.pose.iter().all(|p| p.matrix().is_finite()));
}
#[test]
fn names_have_explicit_unknown_and_side_evidence() {
    assert_eq!(rig::semantics("mixamorig:LeftForeArm").0, Anatomy::Forearm);
    assert_eq!(rig::semantics("skeleton:foot_r_019").1, Side::Right);
    assert_eq!(rig::semantics("l_humerus.33_33").0, Anatomy::UpperArm);
    assert_eq!(rig::semantics("mystery").2, 0.);
    assert_eq!(rig::semantics("L5.6_6").1, Side::Center);
    assert_eq!(rig::semantics("c7.23_23").0, Anatomy::Neck);
    let (_, mut s, _) = rig_corpus::cases().remove(0);
    for n in &mut s.nodes {
        n.name.clear()
    }
    assert!(Rig::analyze(&s).frame.is_none());
}

#[test]
fn head_tip_chain_has_a_frame_but_separate_heads_remain_ambiguous() {
    use armatra::scene::{Local, Node};
    use glam::{Mat4, Vec3};
    let (_, mut asset, _) = rig_corpus::cases().remove(0);
    let original = Rig::analyze(&asset);
    let head = original
        .joints
        .iter()
        .find(|j| j.anatomy == Anatomy::Head)
        .unwrap()
        .node;
    let tip = asset.nodes.len();
    asset.nodes.push(Node {
        name: "skeleton:head_top_010".into(),
        parent: Some(head),
        children: vec![],
        rest: Local::Matrix(Mat4::from_translation(Vec3::Y * 0.1)),
        mesh: None,
        skin: None,
    });
    asset.nodes[head].children.push(tip);
    asset.order.push(tip);
    asset.skins[0].joints.push(tip);
    asset.skins[0].inverse_bind.push(Mat4::IDENTITY);
    let chained = Rig::analyze(&asset);
    assert!(
        chained
            .frame
            .unwrap()
            .abs_diff_eq(original.frame.unwrap(), 1e-5)
    );
    assert_eq!(
        chained
            .joints
            .iter()
            .filter(|j| j.anatomy == Anatomy::Head)
            .count(),
        2,
        "Frame inference must not hide descriptor ambiguity"
    );
    let pelvis = original
        .joints
        .iter()
        .find(|j| j.anatomy == Anatomy::Pelvis)
        .unwrap()
        .node;
    asset.nodes[head].children.retain(|&n| n != tip);
    asset.nodes[pelvis].children.push(tip);
    asset.nodes[tip].parent = Some(pelvis);
    assert!(Rig::analyze(&asset).frame.is_none());
}

#[test]
fn multiple_independent_skins_require_explicit_selection_and_preserve_original_ids() {
    use armatra::retarget::{self, Settings};
    let (_, source, _) = rig_corpus::cases().remove(0);
    let mut target = source.clone();
    target.skins.push(armatra::scene::Skin {
        joints: vec![0],
        inverse_bind: vec![glam::Mat4::IDENTITY],
    });
    assert!(
        retarget::compile(&source, &target, &Settings::default(), &[])
            .unwrap_err()
            .to_string()
            .contains("Multiple skins")
    );
    let settings = Settings {
        target_skin: Some(0),
        ..Default::default()
    };
    let plan = retarget::compile(&source, &target, &settings, &[]).unwrap();
    assert_eq!(plan.settings.target_skin, Some(0));
    assert!(
        plan.rows
            .iter()
            .filter_map(|r| r.target)
            .all(|n| target.skins[0].joints.contains(&n))
    );
    assert!(Rig::analyze_skin(&target, Some(1)).joints.len() == 1);
    assert!(
        retarget::compile(
            &source,
            &target,
            &Settings {
                target_skin: Some(1),
                ..Default::default()
            },
            &[]
        )
        .is_err()
    );
    assert!(
        retarget::compile(
            &source,
            &target,
            &Settings {
                target_skin: Some(99),
                ..Default::default()
            },
            &[]
        )
        .unwrap_err()
        .to_string()
        .contains("does not exist")
    );
}

#[test]
fn older_profile_edits_recompile_without_rewriting_saved_generation() {
    use armatra::{
        profile,
        retarget::{self, Override, Settings},
    };
    let (_, source, target) = rig_corpus::cases().remove(0);
    let settings = Settings {
        contacts: armatra::contacts::Settings {
            looping: armatra::contacts::LoopPolicy::Accumulate,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut old = retarget::compile(
        &source,
        &target,
        &settings,
        &[Override {
            source: 1,
            target: Some(1),
        }],
    )
    .unwrap();
    old.key.algorithm = 5;
    let root = std::path::PathBuf::from(format!(
        "target/test-profile-upgrade/{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let directory = profile::directory(&root, &old.key);
    std::fs::create_dir_all(&directory).unwrap();
    let payload = serde_json::to_string(&old).unwrap();
    let bytes=serde_json::to_vec(&serde_json::json!({"schema":1,"checksum":blake3::hash(payload.as_bytes()).to_hex().as_str(),"payload":payload})).unwrap();
    let path = directory.join("0001.json");
    std::fs::write(&path, &bytes).unwrap();
    let upgraded = profile::load_pair(&root, &source, &target)
        .unwrap()
        .unwrap();
    assert_eq!(upgraded.key.algorithm, retarget::ALGORITHM_VERSION);
    assert_eq!(upgraded.settings, old.settings);
    assert_eq!(upgraded.overrides, old.overrides);
    assert!(
        upgraded
            .warnings
            .iter()
            .any(|w| w.contains("Recompiled saved"))
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert!(!profile::directory(&root, &upgraded.key).exists());
}

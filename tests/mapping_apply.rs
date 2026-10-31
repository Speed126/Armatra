use armatra::{
    fixtures,
    import::{self, Limits},
    retarget::{self, Override, Settings},
    rig::Rig,
};
fn assets() -> (armatra::scene::Asset, armatra::scene::Asset) {
    (
        import::from_bytes(&fixtures::robot(true, true).finish(), &Limits::default()).unwrap(),
        import::from_bytes(
            &fixtures::multi_skin_wave_target().finish(),
            &Limits::default(),
        )
        .unwrap(),
    )
}
#[test]
fn sparse_skinless_wave_recovers_after_missing_skin_with_same_original_id_edits() {
    let (s, t) = assets();
    assert!(s.skins.is_empty());
    assert_eq!(t.skins.len(), 2);
    let rig = Rig::analyze_skin(&s, None);
    assert_eq!(rig.joints.len(), 12);
    assert!(rig.frame.is_some());
    let overrides = vec![
        Override {
            source: 6,
            target: Some(5),
        },
        Override {
            source: 11,
            target: Some(0),
        },
    ];
    let mut settings = Settings::default();
    let error = retarget::compile(&s, &t, &settings, &overrides)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Multiple skins") && error.contains("Target has 2 skins"),
        "{error}"
    );
    settings.target_skin = Some(0);
    let plan = retarget::compile(&s, &t, &settings, &overrides).unwrap();
    assert_eq!(plan.overrides, overrides);
    assert_eq!(plan.settings.source_skin, None);
    assert_eq!(plan.settings.target_skin, Some(0));
    let a = retarget::sample_checked(&s, &t, &s.clips[0], &plan, 0.).unwrap();
    let b = retarget::sample_checked(&s, &t, &s.clips[0], &plan, 0.75).unwrap();
    assert!(!t.globals(&a)[5].abs_diff_eq(t.globals(&b)[5], 1e-5));
    let globals_a = t.globals(&a);
    let globals_b = t.globals(&b);
    let pa = t.palette(12, &globals_a).unwrap();
    let pb = t.palette(12, &globals_b).unwrap();
    let delta = t.meshes[0].primitives[0]
        .vertices
        .iter()
        .map(|v| {
            armatra::scene::skin_position(v, &pa).distance(armatra::scene::skin_position(v, &pb))
        })
        .fold(0f32, f32::max);
    assert!(
        delta > 0.01,
        "Meaningful rendered vertex displacement: {delta}"
    );
}
#[test]
fn independent_target_skins_have_distinct_identity_and_original_node_domains() {
    let (s, t) = assets();
    let a = Settings {
        target_skin: Some(0),
        ..Default::default()
    };
    let b = Settings {
        target_skin: Some(1),
        ..Default::default()
    };
    let p = retarget::compile(
        &s,
        &t,
        &a,
        &[Override {
            source: 6,
            target: Some(5),
        }],
    )
    .unwrap();
    let q = retarget::compile(
        &s,
        &t,
        &b,
        &[Override {
            source: 6,
            target: Some(18),
        }],
    )
    .unwrap();
    assert_ne!(p.key, q.key);
    assert!(
        q.transfers
            .iter()
            .all(|r| t.skins[1].joints.contains(&r.target))
    );
    assert!(retarget::compile(&s, &t, &b, &p.overrides).is_err());
    let invalid = Settings {
        target_skin: Some(9),
        ..Default::default()
    };
    assert!(
        retarget::compile(&s, &t, &invalid, &[])
            .unwrap_err()
            .to_string()
            .contains("Target skin 9")
    );
}
#[test]
fn algorithm_six_profiles_recompile_without_rewriting_saved_generations() {
    let (s, t) = assets();
    let settings = Settings {
        target_skin: Some(0),
        ..Default::default()
    };
    let mut p = retarget::compile(
        &s,
        &t,
        &settings,
        &[Override {
            source: 6,
            target: Some(5),
        }],
    )
    .unwrap();
    p.key.algorithm = 6;
    let payload = serde_json::to_string(&p).unwrap();
    let envelope = serde_json::json!({"schema":1,"checksum":blake3::hash(payload.as_bytes()).to_hex().as_str(),"payload":payload});
    let root = std::env::temp_dir().join(format!("armatra-apply-profile-{}", std::process::id()));
    let dir = armatra::profile::directory(&root, &p.key);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("00000000000000000000000000000001.json");
    let bytes = serde_json::to_vec(&envelope).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let restored = armatra::profile::load_pair(&root, &s, &t).unwrap().unwrap();
    assert_eq!(restored.key.algorithm, retarget::ALGORITHM_VERSION);
    assert_eq!(restored.settings, settings);
    assert_eq!(restored.overrides, p.overrides);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

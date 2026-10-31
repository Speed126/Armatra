use armatra::{
    animation::{self, Cursor, Interpolation, Property, Track},
    cache::{ByteLru, Generation},
    compatibility::{self, Compatibility},
    fixtures::{self, Builder},
    import::{self, Limits},
    library::Library,
    scene::{Local, Trs},
};
use glam::{Mat4, Quat, Vec3, Vec4};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
fn parse(b: Builder) -> armatra::scene::Asset {
    import::from_bytes(&b.finish(), &Limits::default()).expect("valid fixture")
}
fn robot() -> armatra::scene::Asset {
    parse(fixtures::robot(false, false))
}
fn scratch() -> PathBuf {
    static ID: AtomicU64 = AtomicU64::new(0);
    let p = PathBuf::from("target/test-libraries").join(format!(
        "{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn static_cube_and_single_joint_skin() {
    let cube = parse(fixtures::cube());
    assert_eq!(cube.meshes[0].primitives[0].vertices.len(), 36);
    assert!(cube.skins.is_empty());
    let mut b = fixtures::robot(false, false);
    b.json["skins"][0]["joints"] = json!([0]);
    let j = b.json["meshes"][0]["primitives"][0]["attributes"]["JOINTS_0"]
        .as_u64()
        .unwrap() as usize;
    let view = b.json["accessors"][j]["bufferView"].as_u64().unwrap() as usize;
    let start = b.json["bufferViews"][view]["byteOffset"].as_u64().unwrap() as usize;
    let len = b.json["bufferViews"][view]["byteLength"].as_u64().unwrap() as usize;
    b.bin[start..start + len].fill(0);
    let a = parse(b);
    assert_eq!(
        a.palette(12, &a.globals(&a.rest_pose())).unwrap(),
        vec![Mat4::IDENTITY]
    );
}

#[test]
fn legacy_specular_glossiness_uses_diffuse_texture_with_explicit_approximation() {
    let mut b = fixtures::robot(false, false);
    b.json["extensionsUsed"] = json!(["KHR_materials_pbrSpecularGlossiness"]);
    b.json["extensionsRequired"] = json!(["KHR_materials_pbrSpecularGlossiness"]);
    b.json["materials"][0]["extensions"] = json!({"KHR_materials_pbrSpecularGlossiness":{"diffuseFactor":[0.2,0.3,0.4,0.7],"diffuseTexture":{"index":0},"specularFactor":[0.5,0.5,0.5],"glossinessFactor":0.8}});
    let a = parse(b);
    assert_eq!(a.materials[0].color, [0.2, 0.3, 0.4, 0.7]);
    assert_eq!(a.materials[0].image, Some(0));
    assert!(
        a.warnings
            .iter()
            .any(|w| w.contains("specular and glossiness"))
    );
}
#[test]
fn khronos_fox_all_clips_arbitrary_seek_and_nonjoint_channels() {
    let fixture = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "target".into())
        .join("external-fixtures/Fox.glb");
    let path = fixture.as_path();
    if !path.exists() {
        eprintln!(
            "Fox interoperability fixture absent; fetch via python scripts/fetch-test-fixture.py (CI fetches it)"
        );
        return;
    }
    let a = import::from_bytes(
        &import::read_file(path, &Limits::default()).unwrap(),
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(a.clips.len(), 3);
    let names: Vec<_> = a.clips.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Survey") && names.contains(&"Walk") && names.contains(&"Run"));
    let map: Vec<_> = (0..a.nodes.len()).map(Some).collect();
    for clip in &a.clips {
        let mut cursor = Vec::new();
        for i in 0..=120 {
            let t = i as f32 / 120. * clip.duration;
            let p = animation::evaluate(&a, clip, &map, t, &mut cursor);
            let seek = animation::evaluate(&a, clip, &map, t, &mut Vec::new());
            for (x, y) in p.iter().zip(seek) {
                assert!(x.matrix().abs_diff_eq(y.matrix(), 1e-5));
            }
            assert!(a.globals(&p).iter().all(|g| g.is_finite()));
        }
    }
}
#[test]
fn immutable_library_survives_source_deletion_and_rejects_second_writer() {
    let root = scratch();
    let source = root.join("original.glb");
    std::fs::write(&source, fixtures::robot(false, false).finish()).unwrap();
    let bytes = import::read_file(&source, &Limits::default()).unwrap();
    let asset = import::from_bytes(&bytes, &Limits::default()).unwrap();
    let mut l = Library::open(root.join("managed")).unwrap();
    let hash = l.import(&bytes, "Source", &asset).unwrap();
    assert!(Library::open(root.join("managed")).is_err());
    drop(l);
    std::fs::remove_file(&source).unwrap();
    let l = Library::open(root.join("managed")).unwrap();
    let restored = l.load(&hash).unwrap();
    assert_eq!(restored.nodes.len(), asset.nodes.len());
    assert_eq!(restored.clips[0].name, "Wave");
}
#[test]
fn textured_rig_and_multiple_roots_import() {
    let a = robot();
    assert_eq!(a.skins[0].joints.len(), 12);
    assert_eq!(a.images.len(), 1);
    assert_eq!(a.scenes[0].roots.len(), 2);
    assert!(a.warnings.is_empty(), "{:?}", a.warnings);
    assert_eq!(a.clips[0].duration, 3.);
}
#[test]
fn all_truncated_containers_are_errors() {
    let bytes = fixtures::robot(false, false).finish();
    for len in 0..bytes.len() {
        assert!(import::from_bytes(&bytes[..len], &Limits::default()).is_err());
    }
}
#[test]
fn deterministic_mutation_corpus_never_panics() {
    let bytes = fixtures::robot(false, false).finish();
    let mut state = 17u64;
    for _ in 0..512 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut damaged = bytes.clone();
        let i = (state as usize) % damaged.len();
        damaged[i] ^= (state >> 32) as u8;
        assert!(
            std::panic::catch_unwind(|| import::from_bytes(&damaged, &Limits::default())).is_ok(),
            "mutation at {i}"
        );
    }
}
#[test]
fn external_resources_and_required_extensions_rejected() {
    let mut b = fixtures::robot(false, false);
    b.json["buffers"][0]["uri"] = json!("../secret.bin");
    assert!(
        import::from_bytes(&b.finish(), &Limits::default())
            .unwrap_err()
            .to_string()
            .contains("external")
    );
    let mut b = fixtures::robot(false, false);
    b.json["extensionsRequired"] = json!(["KHR_draco_mesh_compression"]);
    assert!(import::from_bytes(&b.finish(), &Limits::default()).is_err());
}
#[test]
fn invalid_accessors_and_limits_rejected() {
    let mut b = fixtures::robot(false, false);
    b.json["accessors"][0]["byteOffset"] = json!(4294967292u64);
    assert!(import::from_bytes(&b.finish(), &Limits::default()).is_err());
    let bytes = fixtures::robot(false, false).finish();
    let limits = Limits {
        elements: 10,
        ..Limits::default()
    };
    assert!(import::from_bytes(&bytes, &limits).is_err());
    let limits = Limits {
        image_side: 2,
        ..Limits::default()
    };
    assert!(import::from_bytes(&bytes, &limits).is_err());
}
#[test]
fn repeated_animation_accessors_cannot_bypass_expansion_budget() {
    let mut b = fixtures::robot(false, false);
    let clip = b.json["animations"][0].clone();
    b.json["animations"] = serde_json::Value::Array(vec![clip; 300]);
    let limits = Limits {
        decoded_bytes: 64 * 1024,
        ..Limits::default()
    };
    let error = import::from_bytes(&b.finish(), &limits).unwrap_err();
    assert!(matches!(error, import::ImportError::Limit(_)), "{error}");
}
#[test]
fn sparse_normalized_strided_accessors() {
    let mut b = Builder::default();
    let dense = b.view(&[0, 255, 0, 0, 255, 0, 0, 0]);
    let ids = b.view(&[1]);
    let values = b.view(&[128, 64]);
    b.json["bufferViews"][dense]["byteStride"] = json!(4);
    b.json["accessors"] = json!([{"bufferView":dense,"componentType":5121,"count":2,"type":"VEC2","normalized":true,"sparse":{"count":1,"indices":{"bufferView":ids,"componentType":5121},"values":{"bufferView":values}}}]);
    let bytes = b.finish();
    let (_, bin) = import::container(&bytes, &Limits::default()).unwrap();
    let doc = gltf::Gltf::from_slice(&bytes).unwrap();
    let a = import::accessor(doc.accessors().next().unwrap(), bin, &Limits::default()).unwrap();
    assert_eq!(a[0], [0., 1.]);
    assert!((a[1][0] - 128. / 255.).abs() < 1e-6);
    assert!((a[1][1] - 64. / 255.).abs() < 1e-6);
}
#[test]
fn padded_matrix_accessor_and_sparse_zero_base() {
    let mut b = Builder::default();
    let view = b.view(&[1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0]);
    b.json["accessors"] = json!([{"bufferView":view,"componentType":5121,"count":1,"type":"MAT3"}]);
    let bytes = b.finish();
    let (_, bin) = import::container(&bytes, &Limits::default()).unwrap();
    let g = gltf::Gltf::from_slice(&bytes).unwrap();
    assert_eq!(
        import::accessor(g.accessors().next().unwrap(), bin, &Limits::default()).unwrap()[0],
        vec![1., 2., 3., 4., 5., 6., 7., 8., 9.]
    );
}
#[test]
fn invalid_hierarchy_cycles_multiple_parents_and_scene_roots() {
    for variant in 0..3 {
        let mut b = fixtures::robot(false, false);
        match variant {
            0 => b.json["nodes"][3]["children"] = json!([0]),
            1 => b.json["nodes"][0]["children"]
                .as_array_mut()
                .unwrap()
                .push(json!(3)),
            _ => b.json["scenes"][0]["nodes"] = json!([3]),
        }
        assert!(import::from_bytes(&b.finish(), &Limits::default()).is_err());
    }
}
#[test]
fn matrix_nodes_mirrored_instances_and_fk() {
    let mut b = fixtures::robot(false, false);
    b.json["nodes"][12] = json!({"mesh":0,"matrix":Mat4::from_scale_rotation_translation(Vec3::new(-1.,2.,1.),Quat::from_rotation_y(0.7),Vec3::new(3.,0.,0.)).to_cols_array()});
    b.json["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"mesh":0,"translation":[-3,0,0]}));
    b.json["scenes"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!(13));
    let a = parse(b);
    assert!(matches!(a.nodes[12].rest, Local::Matrix(_)));
    let g = a.globals(&a.rest_pose());
    assert!(g[12].determinant() < 0.);
    assert_eq!(g[13].w_axis.x, -3.);
    assert!((g[3].w_axis.y - 2.2).abs() < 1e-5);
}
#[test]
fn bind_pose_and_moved_mesh_instance_palettes() {
    let mut a = robot();
    let globals = a.globals(&a.rest_pose());
    let palette = a.palette(12, &globals).unwrap();
    for p in &palette {
        assert!(p.abs_diff_eq(Mat4::IDENTITY, 1e-5));
    }
    a.nodes[12].rest = Local::Trs(Trs {
        translation: Vec3::new(3., 2., 1.),
        ..Trs::default()
    });
    let g = a.globals(&a.rest_pose());
    let p = a.palette(12, &g).unwrap();
    for matrix in p {
        assert!((g[12] * matrix).abs_diff_eq(Mat4::IDENTITY, 1e-5));
    }
    a.nodes[12].rest = Local::Matrix(Mat4::from_scale(Vec3::ZERO));
    assert!(a.palette(12, &a.globals(&a.rest_pose())).is_err());
}
#[test]
fn absent_and_extra_inverse_bind_matrices_multiple_skins() {
    let mut b = fixtures::robot(false, false);
    b.json["skins"][0]
        .as_object_mut()
        .unwrap()
        .remove("inverseBindMatrices");
    let skin = b.json["skins"][0].clone();
    b.json["skins"].as_array_mut().unwrap().push(skin);
    b.json["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"mesh":0,"skin":1,"translation":[3,0,0]}));
    b.json["scenes"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!(13));
    let a = parse(b);
    assert_eq!(a.skins.len(), 2);
    assert_eq!(a.skins[0].inverse_bind[0], Mat4::IDENTITY);
    let g = a.globals(&a.rest_pose());
    assert_ne!(a.palette(12, &g).unwrap()[0], a.palette(13, &g).unwrap()[0]);
    let mut b = fixtures::robot(false, false);
    let matrices: Vec<_> = (0..14)
        .flat_map(|_| Mat4::IDENTITY.to_cols_array())
        .collect();
    let extra = b.accessor(&matrices, "MAT4", 16);
    b.json["skins"][0]["inverseBindMatrices"] = json!(extra);
    assert_eq!(parse(b).skins[0].inverse_bind.len(), 12);
}
#[test]
fn invalid_joint_and_zero_weights_are_errors() {
    let mut b = fixtures::robot(false, false);
    b.json["skins"][0]["joints"] = json!([99999]);
    assert!(import::from_bytes(&b.finish(), &Limits::default()).is_err());
    let mut b = fixtures::robot(false, false);
    let w = b.json["meshes"][0]["primitives"][0]["attributes"]["WEIGHTS_0"]
        .as_u64()
        .unwrap() as usize;
    let view = b.json["accessors"][w]["bufferView"].as_u64().unwrap() as usize;
    let offset = b.json["bufferViews"][view]["byteOffset"].as_u64().unwrap() as usize;
    b.bin[offset..offset + 16].fill(0);
    assert!(import::from_bytes(&b.finish(), &Limits::default()).is_err());
}
fn track(property: Property, interpolation: Interpolation) -> Track {
    Track {
        node: 0,
        property,
        interpolation,
        times: vec![0., 2.],
        values: vec![Vec4::ZERO, Vec4::new(2., 0., 0., 0.)],
    }
}
#[test]
fn interpolation_endpoints_step_linear_and_cubic_tangent_scaling() {
    let mut t = track(Property::Translation, Interpolation::Step);
    let mut cursor = Cursor::default();
    assert_eq!(t.sample(1., &mut cursor), Vec4::ZERO);
    assert_eq!(t.sample(2., &mut cursor).x, 2.);
    t.interpolation = Interpolation::Linear;
    assert_eq!(t.sample(1., &mut cursor).x, 1.);
    assert_eq!(t.sample(-1., &mut cursor).x, 0.);
    assert_eq!(t.sample(5., &mut cursor).x, 2.);
    t.interpolation = Interpolation::Cubic;
    t.values = vec![
        Vec4::ZERO,
        Vec4::ZERO,
        Vec4::X * 2.,
        Vec4::ZERO,
        Vec4::X * 2.,
        Vec4::ZERO,
    ];
    assert!((t.sample(1., &mut cursor).x - 1.5).abs() < 1e-6);
}
#[test]
fn quaternion_slerp_and_cubic_normalize() {
    let mut t = track(Property::Rotation, Interpolation::Linear);
    t.values = vec![
        Vec4::W,
        Vec4::from_array(Quat::from_rotation_y(1.).to_array()),
    ];
    let result = t.sample(1., &mut Cursor::default());
    assert!(Quat::from_array(result.to_array()).abs_diff_eq(Quat::from_rotation_y(0.5), 1e-5));
    t.interpolation = Interpolation::Cubic;
    t.values = vec![
        Vec4::ZERO,
        Vec4::W,
        Vec4::ZERO,
        Vec4::ZERO,
        -Vec4::W,
        Vec4::ZERO,
    ];
    assert!((t.sample(1., &mut Cursor::default()).length() - 1.).abs() < 1e-5);
}
#[test]
fn seeking_matches_progression_and_rest_properties_do_not_leak() {
    let a = robot();
    let mapping: Vec<_> = (0..a.nodes.len()).map(Some).collect();
    let mut cursors = Vec::new();
    for frame in 0..=180 {
        let time = frame as f32 / 60.;
        let sequential = animation::evaluate(&a, &a.clips[0], &mapping, time, &mut cursors);
        let seek = animation::evaluate(&a, &a.clips[0], &mapping, time, &mut Vec::new());
        for (x, y) in sequential.iter().zip(seek) {
            assert!(x.matrix().abs_diff_eq(y.matrix(), 1e-6));
        }
    }
    for time in [2.7, 0.2, 1.4, 0., 3.] {
        let p = animation::evaluate(&a, &a.clips[0], &mapping, time, &mut cursors);
        assert!(p[4].matrix().abs_diff_eq(a.nodes[4].rest.matrix(), 1e-6));
    }
}
#[test]
fn playback_loop_policy_and_nonlooping_end() {
    let mut p = animation::Playback {
        playing: true,
        ..Default::default()
    };
    p.tick(3.5, 3.);
    assert_eq!(p.time, 0.5);
    p.looping = false;
    p.tick(5., 3.);
    assert_eq!(p.time, 3.);
    assert!(!p.playing);
    p.playing = true;
    p.speed = -1.;
    p.tick(1., 3.);
    assert_eq!(p.time, 2.);
}
#[test]
fn independent_permuted_source_maps_joint_and_nonjoint_channels() {
    let target = robot();
    let source = parse(fixtures::robot(true, true));
    let clip = &source.clips[0];
    let m = compatibility::compile(&source, &target, clip, false);
    assert_eq!(m.status, Compatibility::Exact);
    assert_ne!(m.nodes[clip.tracks[0].node], Some(clip.tracks[0].node));
    for time in [0., 0.2, 0.75, 1.1, 3.] {
        let a = animation::evaluate(
            &target,
            &target.clips[0],
            &(0..target.nodes.len()).map(Some).collect::<Vec<_>>(),
            time,
            &mut Vec::new(),
        );
        let b = animation::evaluate(&target, clip, &m.nodes, time, &mut Vec::new());
        for (a, b) in a.iter().zip(b) {
            assert!(a.matrix().abs_diff_eq(b.matrix(), 1e-5));
        }
    }
}
#[test]
fn permuted_nonjoint_mesh_channel_maps_to_the_attachment_not_the_rig() {
    let mut target_builder = fixtures::robot(false, false);
    target_builder.json["animations"][0]["channels"][1]["target"]["node"] = json!(12);
    let target = parse(target_builder);
    assert!(!target.skins.iter().any(|s| s.joints.contains(&12)));
    let mut source_builder = fixtures::robot(true, true);
    source_builder.json["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"Robot mesh"}));
    source_builder.json["scenes"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!(12));
    source_builder.json["animations"][0]["channels"][1]["target"]["node"] = json!(12);
    let count = source_builder.json["nodes"].as_array().unwrap().len();
    source_builder.json["nodes"]
        .as_array_mut()
        .unwrap()
        .reverse();
    for node in source_builder.json["nodes"].as_array_mut().unwrap() {
        if let Some(children) = node
            .get_mut("children")
            .and_then(serde_json::Value::as_array_mut)
        {
            for child in children {
                *child = json!(count - 1 - child.as_u64().unwrap() as usize);
            }
        }
    }
    for root in source_builder.json["scenes"][0]["nodes"]
        .as_array_mut()
        .unwrap()
    {
        *root = json!(count - 1 - root.as_u64().unwrap() as usize);
    }
    for channel in source_builder.json["animations"][0]["channels"]
        .as_array_mut()
        .unwrap()
    {
        let id = channel["target"]["node"].as_u64().unwrap() as usize;
        channel["target"]["node"] = json!(count - 1 - id);
    }
    let source = parse(source_builder);
    let mapping = compatibility::compile(&source, &target, &source.clips[0], false);
    assert_eq!(mapping.status, Compatibility::Exact);
    assert_eq!(mapping.nodes[0], Some(12));
    let pose = animation::evaluate(
        &target,
        &source.clips[0],
        &mapping.nodes,
        0.75,
        &mut Vec::new(),
    );
    assert!((pose[12].matrix().w_axis.y - 0.08).abs() < 1e-5);
    assert!(
        pose[0]
            .matrix()
            .abs_diff_eq(target.nodes[0].rest.matrix(), 1e-6)
    );
}
#[test]
fn ambiguous_and_different_rest_mapping_refused() {
    let target = robot();
    let mut source = parse(fixtures::robot(true, true));
    source.nodes[11].rest = Local::Trs(Trs {
        translation: Vec3::X,
        ..Trs::default()
    });
    assert!(matches!(
        compatibility::compile(&source, &target, &source.clips[0], false).status,
        Compatibility::NeedsMapping(_)
    ));
    let source = parse(fixtures::robot(true, true));
    let mut target = target;
    target.nodes.push(target.nodes[0].clone());
    assert!(matches!(
        compatibility::compile(&source, &target, &source.clips[0], false).status,
        Compatibility::NeedsMapping(_)
    ));
}
#[test]
fn morph_animation_is_inspectable_but_never_exact() {
    let mut b = fixtures::robot(false, false);
    let input = b.accessor(&[0., 1.], "SCALAR", 1);
    let out = b.accessor(&[0., 1.], "SCALAR", 1);
    b.json["animations"] = json!([{"samplers":[{"input":input,"output":out}],"channels":[{"sampler":0,"target":{"node":12,"path":"weights"}}]}]);
    let a = parse(b);
    assert!(!a.clips[0].supported);
    assert!(matches!(
        compatibility::compile(&a, &a, &a.clips[0], true).status,
        Compatibility::Unsupported(_)
    ));
}
#[test]
fn persistent_dedup_reopen_and_corrupt_generation_recovery() {
    let root = scratch();
    let bytes = fixtures::robot(false, false).finish();
    let a = robot();
    let mut library = Library::open(root.clone()).unwrap();
    let hash = library.import(&bytes, "Robot", &a).unwrap();
    library.import(&bytes, "Duplicate", &a).unwrap();
    assert_eq!(library.manifest.records.len(), 2);
    assert_eq!(std::fs::read_dir(root.join("blobs")).unwrap().count(), 1);
    drop(library);
    std::fs::write(
        root.join("manifests/00000000000000000003.json"),
        b"torn publication",
    )
    .unwrap();
    let mut library = Library::open(root).unwrap();
    assert!(library.recovery.is_some());
    assert_eq!(library.load(&hash).unwrap().clips.len(), 1);
    library.import(&bytes, "Again", &a).unwrap();
    assert_eq!(library.manifest.generation, 4);
}
#[test]
fn cache_pinning_eviction_stale_generations_and_thousand_switches() {
    let mut cache = ByteLru::new(20);
    cache.pin([0]);
    cache.insert(0, Arc::new(0), 10);
    cache.insert(1, Arc::new(1), 10);
    for i in 0..1000 {
        assert_eq!(*cache.get(&0).unwrap(), 0);
        cache.insert(1 + i % 2, Arc::new(i), 10);
        assert!(cache.len() <= 2);
        assert_eq!(cache.bytes(), 20);
    }
    let mut generation = Generation::default();
    let stale = generation.advance();
    let fresh = generation.advance();
    assert!(!generation.accepts(stale));
    assert!(generation.accepts(fresh));
}

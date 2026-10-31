use armatra::{
    animation, import,
    library::{Kind, Library},
};
use glam::{Quat, Vec3};
use serde_json::json;
use std::path::PathBuf;

fn directory(name: &str) -> PathBuf {
    let path = PathBuf::from("target/format-tests").join(format!(
        "{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}
#[test]
fn gltf_multiple_buffers_external_texture_curves_and_persistent_reload() {
    let root = directory("gltf");
    std::fs::create_dir(root.join("data")).unwrap();
    std::fs::create_dir(root.join("textures")).unwrap();
    let builder = armatra::fixtures::robot(false, false);
    let original = import::from_bytes(&builder.finish(), &Default::default()).unwrap();
    let mut builder = armatra::fixtures::robot(false, false);
    let mut buffers = vec![];
    let mut files = vec![];
    for (i, view) in builder.json["bufferViews"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = view["byteLength"].as_u64().unwrap() as usize;
        let uri = format!("data/part-{i}.bin");
        let path = root.join(&uri);
        std::fs::write(&path, &builder.bin[offset..offset + len]).unwrap();
        files.push(path);
        buffers.push(json!({"uri":uri,"byteLength":len}));
        view["buffer"] = json!(i);
        view["byteOffset"] = json!(0);
    }
    let image = &builder.json["images"][0];
    let view = &builder.json["bufferViews"][image["bufferView"].as_u64().unwrap() as usize];
    let buffer = view["buffer"].as_u64().unwrap() as usize;
    let image_bytes = std::fs::read(&files[buffer]).unwrap();
    let texture = root.join("textures/checker space.png");
    std::fs::write(&texture, image_bytes).unwrap();
    files.push(texture);
    builder.json["images"][0] = json!({"uri":"textures/checker%20space.png"});
    builder.json["buffers"] = json!(buffers);
    let path = root.join("character.gltf");
    std::fs::write(&path, serde_json::to_vec(&builder.json).unwrap()).unwrap();
    files.push(path.clone());
    let (bytes, asset) = import::from_path(&path, &Default::default()).unwrap();
    assert_eq!(
        armatra::retarget::signature(&asset),
        armatra::retarget::signature(&original)
    );
    assert_eq!(asset.images[0].rgba, original.images[0].rgba);
    assert_eq!(asset.skins[0].inverse_bind, original.skins[0].inverse_bind);
    for (a, b) in asset.clips.iter().zip(&original.clips) {
        for (a, b) in a.tracks.iter().zip(&b.tracks) {
            assert_eq!(a.node, b.node);
            assert_eq!(a.property, b.property);
            assert_eq!(a.interpolation, b.interpolation);
            assert_eq!(a.times, b.times);
            assert_eq!(a.values, b.values);
        }
    }
    let managed = root.join("managed");
    let mut lib = Library::open(managed.clone()).unwrap();
    let hash = lib.import(&bytes, "External glTF", &asset).unwrap();
    lib.import(&bytes, "External glTF", &asset).unwrap();
    assert_eq!(lib.manifest.records.len(), asset.clips.len() + 1);
    drop(lib);
    for file in files {
        std::fs::remove_file(file).unwrap();
    }
    let lib = Library::open(managed).unwrap();
    let reloaded = lib.load(&hash).unwrap();
    assert_eq!(reloaded.images[0].rgba, original.images[0].rgba);
    assert_eq!(
        reloaded.clips[0].tracks[0].values,
        original.clips[0].tracks[0].values
    );
}
#[test]
fn gltf_rejects_escaping_network_missing_and_cross_buffer_views() {
    let root = directory("gltf-invalid");
    let path = root.join("bad.gltf");
    for uri in [
        "../secret.bin",
        "%2e%2e/secret.bin",
        "https://example.com/data.bin",
        "C:/private.bin",
        "/absolute.bin",
        "missing.bin",
        "data:application/octet-stream;base64,AAAA",
    ] {
        let mut b = armatra::fixtures::robot(false, false);
        b.json["buffers"][0]["uri"] = json!(uri);
        std::fs::write(&path, serde_json::to_vec(&b.json).unwrap()).unwrap();
        assert!(
            import::from_path(&path, &Default::default()).is_err(),
            "{uri}"
        );
    }
    let mut b = armatra::fixtures::robot(false, false);
    std::fs::write(root.join("data.bin"), &b.bin).unwrap();
    b.json["buffers"][0] = json!({"uri":"data.bin","byteLength":4});
    std::fs::write(&path, serde_json::to_vec(&b.json).unwrap()).unwrap();
    assert!(import::from_path(&path, &Default::default()).is_err());
}
const BVH: &str = "HIERARCHY
ROOT Hips { OFFSET 0 10 0 CHANNELS 6 Xposition Yposition Zposition Zrotation Xrotation Yrotation
 JOINT Spine { OFFSET 0 5 0 CHANNELS 3 Zrotation Xrotation Yrotation
  JOINT Head { OFFSET 0 4 0 CHANNELS 3 Yrotation Zrotation Xrotation
   End Site { OFFSET 0 2 0 }
  }
 }
}
MOTION
Frames: 2
Frame Time: 0.5
0 0 0 0 0 0 0 0 0 0 0 0
1 2 -3 90 90 0 0 0 0 0 0 0
";
#[test]
fn bvh_preserves_offsets_rotation_order_timing_motion_only_and_reload() {
    let root = directory("bvh");
    let path = root.join("ordered.bvh");
    std::fs::write(&path, BVH).unwrap();
    let (bytes, asset) = import::from_path(&path, &Default::default()).unwrap();
    assert!(asset.meshes.is_empty() && asset.skins.is_empty());
    assert_eq!(asset.nodes.len(), 4);
    assert_eq!(asset.nodes[2].parent, Some(1));
    assert_eq!(asset.clips[0].duration, 0.5);
    let identity: Vec<_> = (0..asset.nodes.len()).map(Some).collect();
    let pose = animation::evaluate(&asset, &asset.clips[0], &identity, 0.5, &mut vec![]);
    let expected =
        Quat::from_rotation_z(90f32.to_radians()) * Quat::from_rotation_x(90f32.to_radians());
    let armatra::scene::Local::Trs(root_pose) = pose[0] else {
        panic!("TRS")
    };
    assert!(root_pose.rotation.abs_diff_eq(expected, 1e-5));
    assert_eq!(root_pose.translation, Vec3::new(1., 12., -3.));
    assert!(asset.character_bounds(0).is_some());
    let mut lib = Library::open(root.join("managed")).unwrap();
    let hash = lib.import(&bytes, "BVH", &asset).unwrap();
    assert!(
        lib.manifest
            .records
            .iter()
            .all(|r| matches!(r.kind, Kind::Clip(_)))
    );
    drop(lib);
    std::fs::remove_file(path).unwrap();
    let lib = Library::open(root.join("managed")).unwrap();
    assert_eq!(
        lib.load(&hash).unwrap().clips[0].tracks[1].values,
        asset.clips[0].tracks[1].values
    );
}
#[test]
fn bvh_malformed_channels_counts_samples_and_limits_are_rejected() {
    let root = directory("bvh-invalid");
    let path = root.join("bad.bvh");
    for text in [
        BVH.replace("Frames: 2", "Frames: 120001"),
        BVH.replace("Frame Time: 0.5", "Frame Time: 0"),
        BVH.replace("Xposition Yposition", "Xposition Xposition"),
        BVH.replace("1 2 -3", "NaN 2 -3"),
        format!("{BVH} 42"),
        BVH[..BVH.len() / 2].to_owned(),
    ] {
        std::fs::write(&path, text).unwrap();
        assert!(import::from_path(&path, &Default::default()).is_err());
    }
    std::fs::write(&path, BVH).unwrap();
    let limits = import::Limits {
        decoded_bytes: 16,
        ..Default::default()
    };
    assert!(import::from_path(&path, &limits).is_err());
    let path = root.join("fake.fbx");
    std::fs::write(&path, b"Kaydara FBX Binary").unwrap();
    assert!(
        import::from_path(&path, &Default::default())
            .unwrap_err()
            .to_string()
            .contains("not supported")
    );
}

use armatra::{
    bundled, fixtures,
    grid::Grid,
    import,
    library::{Kind, Library},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn directory(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "armatra-persistence-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn seeded_library_restart_delete_and_future_version() -> anyhow::Result<()> {
    let root = directory("seed");
    let mut library = Library::open_seeded(root.clone())?;
    assert_eq!(library.manifest.records.len(), 9);
    assert!(
        library
            .manifest
            .records
            .iter()
            .all(|r| matches!(r.kind, Kind::Clip(_)))
    );
    for record in &library.manifest.records {
        let asset = library.load(&record.hash)?;
        assert!(asset.meshes.is_empty());
        assert!(!asset.clips.is_empty());
    }
    let generation = library.manifest.generation;
    let deleted = library.manifest.records[0].id.clone();
    drop(library);
    library = Library::open_seeded(root.clone())?;
    assert_eq!(library.manifest.generation, generation);
    assert_eq!(library.manifest.records.len(), 9);
    library.remove(&deleted)?;
    let deleted_generation = library.manifest.generation;
    drop(library);
    library = Library::open_seeded(root.clone())?;
    assert_eq!(library.manifest.records.len(), 8);
    assert!(library.manifest.records.iter().all(|r| r.id != deleted));
    assert_eq!(library.manifest.generation, deleted_generation);
    // A version upgrade must not restore previous completed-but-deleted IDs.
    library.seed_bundled(2, bundled::MOTIONS)?;
    assert_eq!(library.manifest.records.len(), 8);
    drop(library);
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn upgrade_adds_only_genuinely_new_content_and_adopts_existing_import() -> anyhow::Result<()> {
    let root = directory("upgrade");
    let mut library = Library::open(root.clone())?;
    let motion = &bundled::MOTIONS[0];
    let asset = import::from_bytes(motion.bytes, &import::Limits::default())?;
    library.import(motion.bytes, "Existing import of the same motion", &asset)?;
    library.seed_bundled(1, &bundled::MOTIONS[..8])?;
    assert_eq!(library.manifest.records.len(), 8);
    let deleted = library.manifest.records[1].id.clone();
    library.remove(&deleted)?;
    library.seed_bundled(2, bundled::MOTIONS)?;
    assert_eq!(library.manifest.records.len(), 8);
    assert!(library.manifest.records.iter().all(|r| r.id != deleted));
    assert_eq!(library.manifest.bundled_completed.len(), 9);
    let generation = library.manifest.generation;
    library.seed_bundled(2, bundled::MOTIONS)?;
    assert_eq!(library.manifest.generation, generation);
    drop(library);
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn grid_uses_visible_rest_geometry_at_tiny_and_huge_scales() -> anyhow::Result<()> {
    let mut intervals = vec![];
    for scale in [0.0001, 0.01, 1., 100., 10000.] {
        let mut builder = fixtures::robot(false, false);
        // Scale the imported scene, not any runtime animation/world units.
        let roots = builder.json["scenes"][0]["nodes"]
            .as_array()
            .unwrap()
            .clone();
        for node in roots {
            builder.json["nodes"][node.as_u64().unwrap() as usize]["scale"] =
                serde_json::json!([scale, scale, scale]);
        }
        // A disconnected extreme helper must not change the visible mesh size.
        builder.json["nodes"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"name":"Helper","translation":[1e20,1e20,1e20]}));
        let asset = import::from_bytes(&builder.finish(), &import::Limits::default())?;
        let grid = Grid::for_asset(&asset, asset.default_scene);
        let cells = grid.character_size / grid.interval;
        assert!((2.5..=6.5).contains(&cells), "scale {scale}: {grid:?}");
        let rest = asset.rest_pose();
        for frame in 0..100 {
            let mut pose = rest.clone();
            if let armatra::scene::Local::Trs(trs) = &mut pose[0] {
                trs.translation.y += frame as f32 * scale;
            }
            // Arbitrary animated/skinned bounds never feed grid computation.
            let _ = asset.character_pose_bounds(asset.default_scene, &pose);
            assert_eq!(grid, Grid::for_asset(&asset, asset.default_scene));
        }
        intervals.push(grid.interval);
    }
    assert!(intervals[4] / intervals[0] > 1e7);
    for invalid in [f32::NAN, f32::INFINITY, -1., 0.] {
        assert_eq!(Grid::from_size(invalid).interval, 0.5);
    }
    Ok(())
}

#[test]
fn skeleton_grid_fallback_keeps_world_scale_and_ignores_helpers() -> anyhow::Result<()> {
    let mut previous = 0.;
    for scale in [0.00001, 1., 10000.] {
        let mut builder = fixtures::robot(true, true);
        let roots = builder.json["scenes"][0]["nodes"]
            .as_array()
            .unwrap()
            .clone();
        for node in roots {
            builder.json["nodes"][node.as_u64().unwrap() as usize]["scale"] =
                serde_json::json!([scale, scale, scale]);
        }
        let root = builder.json["scenes"][0]["nodes"][0].as_u64().unwrap() as usize;
        let helper = builder.json["nodes"].as_array().unwrap().len();
        builder.json["nodes"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"name":"Helper","translation":[1e10,1e10,1e10]}));
        builder.json["nodes"][root]["children"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!(helper));
        let bytes = builder.finish();
        let asset = import::from_bytes(&bytes, &import::Limits::default())?;
        let grid = Grid::for_asset(&asset, asset.default_scene);
        assert!(grid.interval > previous, "{grid:?}");
        assert!(
            (grid.character_size / scale - 1.65).abs() < 1.,
            "World-size skeleton bound: {grid:?}"
        );
        previous = grid.interval;
    }
    Ok(())
}

#[test]
fn grid_changes_with_the_selected_rest_scene() -> anyhow::Result<()> {
    let mut builder = fixtures::cube();
    let mut large = builder.json["nodes"][0].clone();
    large["scale"] = serde_json::json!([1000., 1000., 1000.]);
    builder.json["nodes"].as_array_mut().unwrap().push(large);
    builder.json["scenes"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"nodes":[1]}));
    let asset = import::from_bytes(&builder.finish(), &import::Limits::default())?;
    let small = Grid::for_asset(&asset, 0);
    let large = Grid::for_asset(&asset, 1);
    assert!((large.interval / small.interval - 1000.).abs() < 0.01);
    Ok(())
}

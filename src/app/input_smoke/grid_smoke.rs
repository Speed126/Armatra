//! Render the actual grid shader at five orders above/below normal size.
use super::*;
pub(super) fn run(renderer: &Renderer, gpu: &Gpu) -> Result<()> {
    let previous = renderer.show_grid.replace(true);
    let result = check(renderer, gpu);
    renderer.show_grid.set(previous);
    result
}
fn check(renderer: &Renderer, gpu: &Gpu) -> Result<()> {
    let mut images = vec![];
    let mut evidence = vec![];
    for (name, scale) in [("tiny", 0.0001), ("normal", 1.), ("huge", 10000.)] {
        let mut builder = crate::fixtures::robot(false, false);
        let roots = builder.json["scenes"][0]["nodes"]
            .as_array()
            .unwrap()
            .clone();
        for root in roots {
            builder.json["nodes"][root.as_u64().unwrap() as usize]["scale"] =
                serde_json::json!([scale, scale, scale]);
        }
        let asset = import::from_bytes(&builder.finish(), &Limits::default())?;
        let resident = renderer.upload(gpu, &asset)?;
        let mut camera = Camera::default();
        camera.fit(&asset, asset.default_scene);
        let render = |pose: &[crate::scene::Local]| -> Result<image::RgbaImage> {
            let target = Target::new(&gpu.device, [256, 256]);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            renderer.render(
                gpu,
                &mut encoder,
                &target,
                &camera,
                &asset,
                &resident,
                &asset.globals(pose),
                asset.default_scene,
                false,
                false,
            );
            gpu.queue.submit([encoder.finish()]);
            crate::diagnostics::read_target(gpu, &target)
        };
        let image = render(&asset.rest_pose())?;
        let mut colors = HashMap::new();
        for pixel in image.pixels() {
            *colors.entry(pixel.0).or_insert(0usize) += 1;
        }
        let background = colors
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .unwrap()
            .0;
        let visible = image
            .pixels()
            .filter(|p| {
                (0..3)
                    .map(|c| p.0[c].abs_diff(background[c]) as u32)
                    .sum::<u32>()
                    > 4
            })
            .count();
        ensure!(
            visible > 500,
            "{name} grid must be visibly rendered, got {visible} pixels"
        );
        let identity: Vec<_> = (0..asset.nodes.len()).map(Some).collect();
        for frame in 0..8 {
            let pose = animation::evaluate(
                &asset,
                &asset.clips[0],
                &identity,
                asset.clips[0].duration * frame as f32 / 8.,
                &mut vec![],
            );
            ensure!(
                render(&pose)?.as_raw() == image.as_raw(),
                "{name} grid image changed during animation"
            );
        }
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png)?;
        write_evidence(&format!("grid-{name}.png"), png.into_inner())?;
        evidence.push(serde_json::json!({"imported_scale":scale,"grid":resident.grids[0],"visible_pixels":visible,"playback_frames_identical":8}));
        images.push(image);
    }
    for image in [&images[0], &images[2]] {
        let changed = image
            .pixels()
            .zip(images[1].pixels())
            .filter(|(a, b)| (0..3).map(|c| a.0[c].abs_diff(b.0[c]) as u32).sum::<u32>() > 8)
            .count();
        ensure!(
            changed < 800,
            "Model-relative grid images differ at {changed}/65536 pixels"
        );
    }
    write_evidence(
        "grid-rendered.json",
        serde_json::to_vec_pretty(
            &serde_json::json!({"result":"PASS","adapter":gpu.info.name,"backend":format!("{:?}",gpu.info.backend),"scales":evidence,"comparison":"Actual shader grid-only readbacks; normalized rest-fitted cameras; at most 800/65536 materially changed pixels"}),
        )?,
    )?;
    println!(
        "Rendered GPU grids at 0.0001 / 1 / 10000 scene scale, constant through animation: PASS"
    );
    Ok(())
}

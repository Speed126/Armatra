use crate::{
    animation,
    cache::ByteLru,
    compatibility, fixtures,
    import::{self, Limits},
    render::{Camera, Gpu, Renderer, Target},
    scene::{Asset, Vertex, skin_position},
};
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use std::{hint::black_box, path::Path, sync::Arc, time::Instant};
use wgpu::util::DeviceExt;

pub fn read_target(gpu: &Gpu, target: &Target) -> Result<image::RgbaImage> {
    read_texture(gpu, &target.texture)
}
pub fn read_texture(gpu: &Gpu, texture: &wgpu::Texture) -> Result<image::RgbaImage> {
    let width = texture.width();
    let height = texture.height();
    let pitch = (width * 4).div_ceil(256) * 256;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Screenshot readback"),
        size: u64::from(pitch) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pitch),
                rows_per_image: Some(height),
            },
        },
        texture.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv()??;
    let data = buffer.slice(..).get_mapped_range()?;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in data.chunks_exact(pitch as usize) {
        rgba.extend_from_slice(&row[..width as usize * 4]);
    }
    if matches!(
        texture.format(),
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    ) {
        for p in rgba.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
    }
    drop(data);
    buffer.unmap();
    image::RgbaImage::from_raw(width, height, rgba).context("readback dimensions")
}
pub fn render_sample(
    gpu: &Gpu,
    renderer: &Renderer,
    asset: &Asset,
    size: [u32; 2],
    time: f32,
) -> Result<image::RgbaImage> {
    let target = Target::new(&gpu.device, size);
    let resident = renderer.upload(gpu, asset)?;
    let mut camera = Camera::default();
    camera.fit(asset, asset.default_scene);
    let pose = if let Some(clip) = asset.clips.first() {
        animation::evaluate(
            asset,
            clip,
            &(0..asset.nodes.len()).map(Some).collect::<Vec<_>>(),
            time,
            &mut Vec::new(),
        )
    } else {
        asset.rest_pose()
    };
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.render(
        gpu,
        &mut encoder,
        &target,
        &camera,
        asset,
        &resident,
        &asset.globals(&pose),
        asset.default_scene,
        true,
        true,
    );
    gpu.queue.submit([encoder.finish()]);
    read_target(gpu, &target)
}
pub fn gpu_smoke() -> Result<()> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let (gpu, _) = pollster::block_on(Gpu::new(&instance, None))?;
    println!(
        "GPU: {} | {:?} | {} | {}",
        gpu.info.name, gpu.info.backend, gpu.info.driver, gpu.info.driver_info
    );
    let renderer = Renderer::new(&gpu);
    let asset = import::from_bytes(&fixtures::robot(false, false).finish(), &Limits::default())?;
    std::fs::create_dir_all("target/reports")?;
    let image = render_sample(&gpu, &renderer, &asset, [960, 720], 0.75)?;
    image.save("target/reports/viewport.png")?;
    let cube = import::from_bytes(&fixtures::cube().finish(), &Limits::default())?;
    render_sample(&gpu, &renderer, &cube, [640, 480], 0.)?.save("target/reports/cube.png")?;
    let mut instances = cube.clone();
    let rest = instances.nodes[0].rest.matrix();
    instances.nodes[0].rest =
        crate::scene::Local::Matrix(Mat4::from_translation(Vec3::new(-1., 0., 0.)) * rest);
    let mut reflected = instances.nodes[0].clone();
    reflected.rest = crate::scene::Local::Matrix(
        Mat4::from_translation(Vec3::new(1., 0., 0.))
            * Mat4::from_scale(Vec3::new(-1., 1.5, 1.))
            * rest,
    );
    instances.nodes.push(reflected);
    instances.default_bounds = None;
    instances.order.push(1);
    instances.scenes[0].roots.push(1);
    render_sample(&gpu, &renderer, &instances, [800, 480], 0.)?
        .save("target/reports/static-instances.png")?;
    let fixture = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "target".into())
        .join("external-fixtures/Fox.glb");
    let fox_path = fixture.as_path();
    if fox_path.exists() {
        let fox = import::from_bytes(
            &import::read_file(fox_path, &Limits::default())?,
            &Limits::default(),
        )?;
        let mut report = String::new();
        for clip in &fox.clips {
            let mut preview = fox.clone();
            preview.clips = vec![clip.clone()];
            render_sample(&gpu, &renderer, &preview, [640, 480], clip.duration * 0.37)?.save(
                fixture
                    .parent()
                    .unwrap()
                    .join(format!("Fox-{}.png", clip.name)),
            )?;
            report.push_str(&format!(
                "{}: {:.6}s, {} channels, GPU render at {:.6}s succeeded\n",
                clip.name,
                clip.duration,
                clip.tracks.len(),
                clip.duration * 0.37
            ));
        }
        std::fs::write("target/reports/fox.txt", report)?;
    }
    let mut residents = ByteLru::new(32 * 1024 * 1024);
    residents.pin([0, 1]);
    for key in 0..2 {
        let resident = renderer.upload(&gpu, &asset)?;
        let size = resident.bytes;
        residents.insert(key, resident, size);
    }
    let stable = renderer.resource_stats();
    let mut warm = Vec::new();
    for i in 0..1000 {
        let start = Instant::now();
        let resident = residents.get(&(i % 2)).context("GPU residency")?;
        black_box(resident);
        warm.push(start.elapsed().as_secs_f64() * 1e6);
        ensure!(
            renderer.resource_stats() == stable,
            "GPU resources grew during resident switching"
        );
    }
    warm.sort_by(f64::total_cmp);
    std::fs::write(
        "target/reports/gpu-switching.txt",
        format!(
            "1,000 fully resident GPU handle switches\nLive GPU assets: {}\nAccounted GPU bytes: {}\nUpload calls before and after: {}\np50: {:.3} us\np95: {:.3} us\nNo disk parsing or resource creation in measured lookup path. Counts track application-owned asset allocations, not driver heap/RSS.\n",
            stable.0, stable.1, stable.2, warm[500], warm[950]
        ),
    )?;
    ensure!(
        image
            .pixels()
            .filter(|p| p[1] > p[0].saturating_add(15) && p[1] > 80)
            .count()
            > 1000,
        "Expected visible textured robot"
    );
    // Compare the actual vertex-shader path to the CPU reference using a storage output.
    let source = include_str!("../shaders/mesh.wgsl")
        .replace("@vertex fn vs(v: Vertex)", "fn vs(v: Vertex)");
    // A compute harness calls the same blend/normal/position functions as the vertex shader.
    let compute = format!(
        "{source}\nstruct Input {{ p: vec4<f32>, n: vec4<f32>, joints0: vec4<u32>, joints1: vec4<u32>, weights0: vec4<f32>, weights1: vec4<f32> }}\n@group(2) @binding(0) var<storage, read> inputs: array<Input>;\n@group(2) @binding(1) var<storage, read_write> output: array<vec4<f32>>;\n@compute @workgroup_size(64) fn compare(@builtin(global_invocation_id) id: vec3<u32>) {{ if id.x >= arrayLength(&inputs) {{ return; }} let i=inputs[id.x]; let v=Vertex(i.p.xyz,i.n.xyz,vec2(0.),i.joints0,i.joints1,i.weights0,i.weights1); let result=vs(v); output[id.x*2u]=result.position; output[id.x*2u+1u]=vec4(result.normal,0.); }}"
    );
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("CPU/GPU LBS reference"),
            source: wgpu::ShaderSource::Wgsl(compute.into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: Some("compare"),
            compilation_options: Default::default(),
            cache: None,
        });
    let palette: Vec<_> = (0..8)
        .map(|i| {
            Mat4::from_scale_rotation_translation(
                Vec3::new(1. + i as f32 * 0.1, 0.8, 1.),
                glam::Quat::from_rotation_z(i as f32 * 0.2),
                Vec3::new(i as f32 * 0.05, 0.1, 0.),
            )
        })
        .collect();
    let mut inputs = Vec::<[u32; 24]>::new();
    let mut vertices = Vec::new();
    for i in 0..192 {
        let mut v = Vertex {
            position: [i as f32 * 0.01, 0.3, 0.1],
            normal: [0., 1., 0.],
            uv: [0.; 2],
            joints: [0, 1, 2, 3, 4, 5, 6, 7],
            weights: [1.; 8],
        };
        if i % 3 == 0 {
            v.weights[4..].fill(0.);
        }
        if i % 3 == 2 {
            v.joints[7] = 999;
            v.weights[7] = 0.;
        }
        let mut raw = [0u32; 24];
        for k in 0..3 {
            raw[k] = v.position[k].to_bits();
            raw[4 + k] = v.normal[k].to_bits();
        }
        raw[3] = 1f32.to_bits();
        raw[8..16].copy_from_slice(&v.joints);
        for k in 0..8 {
            raw[16 + k] = v.weights[k].to_bits();
        }
        inputs.push(raw);
        vertices.push(v);
    }
    let data = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&inputs),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let result_size = (inputs.len() * 32) as u64;
    let output = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: result_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: result_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let palettes = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&palette),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let mut max_error = 0f32;
    let mut max_normal_error = 0f32;
    for scale in [
        Vec3::ONE,
        Vec3::new(-1., 2., 0.5),
        Vec3::new(0.0000000001, 1., 1.),
    ] {
        let model = Mat4::from_scale(scale);
        let mut uniform = Vec::<f32>::new();
        uniform.extend(Mat4::IDENTITY.to_cols_array());
        uniform.extend(model.to_cols_array());
        uniform.extend([1.; 4]);
        uniform.extend([1., 8., 0., 0.5]);
        uniform.extend([1., 0., 0., 0.]);
        let uniforms = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let group0 = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: palettes.as_entire_binding(),
                },
            ],
        });
        let group2 = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(2),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: data.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group0, &[]);
            pass.set_bind_group(2, &group2, &[]);
            pass.dispatch_workgroups(3, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &read, 0, result_size);
        gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        read.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let bytes = read.slice(..).get_mapped_range()?;
        let floats: &[f32] = bytemuck::cast_slice(&bytes);
        for (i, v) in vertices.iter().enumerate() {
            let actual = Vec3::new(floats[i * 8], floats[i * 8 + 1], floats[i * 8 + 2]);
            let expected = model.transform_point3(skin_position(v, &palette));
            max_error = max_error.max(actual.distance(expected));
            let mut blended = Mat4::ZERO;
            let mut sum = 0.;
            for (&j, &w) in v.joints.iter().zip(&v.weights) {
                if w > 0.
                    && let Some(p) = palette.get(j as usize)
                {
                    blended += *p * w;
                    sum += w;
                }
            }
            let transform = model * (blended * (1. / sum));
            let expected_normal = if transform.determinant().abs() > 1e-7 {
                transform
                    .inverse()
                    .transpose()
                    .transform_vector3(v.normal.into())
            } else {
                transform.transform_vector3(v.normal.into())
            };
            let actual_normal = Vec3::new(floats[i * 8 + 4], floats[i * 8 + 5], floats[i * 8 + 6]);
            max_normal_error = max_normal_error.max(actual_normal.distance(expected_normal));
            ensure!(
                floats[i * 8..i * 8 + 8].iter().all(|f| f.is_finite()),
                "nonfinite GPU position/normal"
            );
        }
        drop(bytes);
        read.unmap();
    }
    ensure!(max_error < 1e-4, "CPU/GPU mismatch: {max_error}");
    ensure!(
        max_normal_error < 1e-4,
        "CPU/GPU affine normal mismatch: {max_normal_error}"
    );
    let report = format!(
        "Adapter: {}\nBackend: {:?}\nDriver: {} {}\nCases: 576 (four/eight influences, invalid zero-weight index, reflection, nonuniform and near-singular scales)\nMax position error: {max_error:.9}\nMax affine normal error: {max_normal_error:.9}\nTolerance: 0.0001\nNormal outputs: finite (affine inverse-transpose approximation)\n",
        gpu.info.name, gpu.info.backend, gpu.info.driver, gpu.info.driver_info
    );
    std::fs::write("target/reports/gpu-smoke.txt", &report)?;
    println!("{report}");
    Ok(())
}
pub fn benchmark() -> Result<()> {
    let bytes = fixtures::robot(false, false).finish();
    let a = Arc::new(import::from_bytes(&bytes, &Limits::default())?);
    let b = Arc::new(import::from_bytes(
        &fixtures::robot(true, true).finish(),
        &Limits::default(),
    )?);
    let mapping = compatibility::compile(&b, &a, &b.clips[0], false);
    let measure = |mut f: Box<dyn FnMut()>| {
        let mut data = Vec::new();
        for _ in 0..1000 {
            let start = Instant::now();
            f();
            data.push(start.elapsed().as_secs_f64() * 1e6);
        }
        data.sort_by(f64::total_cmp);
        (data[500], data[950], data[990])
    };
    let target = Arc::clone(&a);
    let source = Arc::clone(&b);
    let mut cursor = Vec::new();
    let mut time = 0f32;
    let evaluate = measure(Box::new(move || {
        time = (time + 0.016) % 3.;
        black_box(animation::evaluate(
            &target,
            &source.clips[0],
            &mapping.nodes,
            time,
            &mut cursor,
        ));
    }));
    let target = Arc::clone(&a);
    let pose = target.rest_pose();
    let fk = measure(Box::new(move || {
        black_box(target.globals(&pose));
    }));
    let mut cache = ByteLru::new(4 * 1024 * 1024);
    cache.pin([0, 1]);
    cache.insert(0, Arc::clone(&a), a.cpu_bytes());
    cache.insert(1, Arc::clone(&b), b.cpu_bytes());
    let cache_bytes = cache.bytes();
    let mut i = 0;
    let warm = measure(Box::new(move || {
        i += 1;
        black_box(cache.get(&(i % 2)).expect("resident"));
        assert_eq!(cache.bytes(), cache_bytes);
        assert_eq!(cache.len(), 2);
    }));
    let cold_bytes = bytes.clone();
    let cold = measure(Box::new(move || {
        black_box(import::from_bytes(&cold_bytes, &Limits::default()).unwrap());
    }));
    std::fs::create_dir_all("target/reports")?;
    let report = format!(
        "# Local benchmark measurements\n\nOS: {} {}. CPU: {}. Build: release (run command below).\nAsset: original robot, {} bytes, {} nodes, {} vertices, 12 joints, 2 channels.\n1,000 observations per row; wall-clock CPU timing in microseconds.\n\n| Operation | p50 µs | p95 µs | p99 µs |\n|---|---:|---:|---:|\n| TRS sample | {:.3} | {:.3} | {:.3} |\n| FK | {:.3} | {:.3} | {:.3} |\n| Resident CPU handle switch | {:.3} | {:.3} | {:.3} |\n| Cold GLB parse + PNG decode | {:.3} | {:.3} | {:.3} |\n\n1,000 resident CPU switches: 2 cache entries, {cache_bytes} accounted bytes throughout. This measures handle lookup, not complete UI/GPU frame latency or process RSS. GPU details are in gpu-smoke.txt.\n\nReproduce: `cargo run --release -- --benchmark`.\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::var("PROCESSOR_IDENTIFIER")
            .unwrap_or_else(|_| "record CPU model separately".into()),
        bytes.len(),
        a.nodes.len(),
        a.meshes
            .iter()
            .flat_map(|m| &m.primitives)
            .map(|p| p.vertices.len())
            .sum::<usize>(),
        evaluate.0,
        evaluate.1,
        evaluate.2,
        fk.0,
        fk.1,
        fk.2,
        warm.0,
        warm.1,
        warm.2,
        cold.0,
        cold.1,
        cold.2
    );
    let report = format!(
        "{report}\nNamed CPU: {}.\nOS release: {}.\n",
        std::env::var("ARMATRA_CPU").unwrap_or_else(|_| "not recorded".into()),
        std::env::var("ARMATRA_OS").unwrap_or_else(|_| "not recorded".into())
    );
    std::fs::write("target/reports/benchmarks.md", &report)?;
    println!("{report}");
    Ok(())
}
pub fn inspect(path: &Path) -> Result<()> {
    let (_, a) = import::from_path(path, &Limits::default())?;
    println!(
        "{} nodes, {} meshes, {} skins, {} clips\nWarnings: {:#?}",
        a.nodes.len(),
        a.meshes.len(),
        a.skins.len(),
        a.clips.len(),
        a.warnings
    );
    Ok(())
}

/// Local interoperability diagnosis. Outputs numeric analysis only; callers own
/// the report path and imported personal files are never rewritten.
pub fn analyze_retarget(
    source_path: &Path,
    target_path: &Path,
    output: &Path,
    settings: &crate::retarget::Settings,
) -> Result<()> {
    use crate::{
        animation, contacts, retarget,
        rig::{Anatomy, Rig},
    };
    use glam::{Mat4, Vec3};
    let limits = Limits::default();
    let source = import::from_bytes(&import::read_file(source_path, &limits)?, &limits)?;
    let target = import::from_bytes(&import::read_file(target_path, &limits)?, &limits)?;
    let clip = source.clips.first().context("Source has no clips")?;
    let sr = Rig::analyze_skin(&source, settings.source_skin);
    let tr = Rig::analyze_skin(&target, settings.target_skin);
    let describe = |asset: &crate::scene::Asset, rig: &Rig| {
        serde_json::json!({"joints":rig.joints.len(),"height":rig.height,"frame":rig.frame.map(|q|q.to_array()),
            "warnings":rig.warnings,"skins":asset.skins.iter().map(|s|s.joints.len()).collect::<Vec<_>>(),
            "nodes":rig.joints.iter().map(|j|serde_json::json!({"node":j.node,"name":asset.nodes[j.node].name,"anatomy":j.anatomy,"side":j.side,"rigid":j.rigid,"position":j.position.to_array()})).collect::<Vec<_>>()})
    };
    let plan = match retarget::compile(&source, &target, settings, &[]) {
        Ok(plan) => plan,
        Err(error) => {
            std::fs::write(
                output,
                serde_json::to_vec_pretty(
                    &serde_json::json!({"error":error.to_string(),"settings":settings,"source":describe(&source,&sr),"target":describe(&target,&tr)}),
                )?,
            )?;
            return Err(error);
        }
    };
    let landmarks = |a: &crate::scene::Asset, r: &Rig, pose: &[crate::scene::Local]| {
        let globals = a.globals(pose);
        let point = |role| {
            r.joints
                .iter()
                .find(|j| j.anatomy == role)
                .map(|j| (j.node, globals[j.node].w_axis.truncate()))
        };
        let pelvis = point(Anatomy::Pelvis);
        let head = point(Anatomy::Head);
        serde_json::json!({"pelvis":pelvis.map(|(n,p)|serde_json::json!({"node":n,"name":a.nodes[n].name,"position":p.to_array()})),
            "head":head.map(|(n,p)|serde_json::json!({"node":n,"name":a.nodes[n].name,"position":p.to_array()})),
            "torso_direction":pelvis.zip(head).map(|((_,p),(_,h))|(h-p).normalize_or_zero().to_array())})
    };
    let identity: Vec<_> = (0..source.nodes.len()).map(Some).collect();
    let mut samples = vec![];
    for fraction in [0., 0.25, 0.5, 0.75, 1.] {
        let time = fraction * clip.duration;
        let original = animation::evaluate(&source, clip, &identity, time, &mut vec![]);
        let pose = retarget::sample_checked(&source, &target, clip, &plan, time)?;
        samples.push(serde_json::json!({"time":time,"source":landmarks(&source,&sr,&original),"target":landmarks(&target,&tr,&pose),"target_motion_root":target.globals(&pose)[plan.motion_target].w_axis.truncate().to_array()}));
    }
    let mut loops = vec![];
    for policy in [
        contacts::LoopPolicy::Repeat,
        contacts::LoopPolicy::Accumulate,
    ] {
        let mut effective = plan.clone();
        effective.settings.contacts.looping = policy.clone();
        let c = contacts::compile(&source, &target, clip, &effective, Mat4::IDENTITY)?;
        let phases: Vec<_> = std::iter::once(clip.duration as f64 * 0.5)
            .chain(c.intervals.iter().map(|v| (v.start + v.end) as f64 * 0.5))
            .collect();
        let mut roots = vec![];
        for phase in phases {
            let initial = c.raw_pose_checked(&source, &target, clip, &effective, phase, true)?;
            let origin = target.globals(&initial)[plan.motion_target]
                .w_axis
                .truncate();
            let initial_ik = crate::ik::sample(&source, &target, clip, &effective, &c, phase, true);
            let initial_ik_globals = target.globals(&initial_ik.pose);
            let initial_anchors = c.active(phase, true);
            for cycle in [0, 1, 2, 5, 10, -1, 0] {
                let time = phase + cycle as f64 * clip.duration as f64;
                let pose = c.raw_pose_checked(&source, &target, clip, &effective, time, true)?;
                let root = target.globals(&pose)[plan.motion_target].w_axis.truncate();
                let expected = Vec3::from_array(c.cycle_displacement) * cycle as f32;
                anyhow::ensure!(
                    (root - origin).abs_diff_eq(expected, tr.height * 1e-4),
                    "Loop placement mismatch"
                );
                let corrected =
                    crate::ik::sample(&source, &target, clip, &effective, &c, time, true);
                let corrected_globals = target.globals(&corrected.pose);
                for joint in &tr.joints {
                    let mut ancestor = Some(joint.node);
                    let mut moves = false;
                    while let Some(node) = ancestor {
                        if node == plan.motion_target {
                            moves = true;
                            break;
                        }
                        ancestor = target.nodes[node].parent;
                    }
                    let joint_expected = if moves { expected } else { Vec3::ZERO };
                    anyhow::ensure!(
                        (corrected_globals[joint.node].w_axis.truncate()
                            - initial_ik_globals[joint.node].w_axis.truncate())
                        .abs_diff_eq(joint_expected, tr.height * 1e-4),
                        "IK loop placement mismatch at joint {}, policy {:?}, phase {}, cycle {}: observed {:?}, expected {:?}; warnings {:?}",
                        joint.node,
                        policy,
                        phase,
                        cycle,
                        corrected_globals[joint.node].w_axis.truncate()
                            - initial_ik_globals[joint.node].w_axis.truncate(),
                        joint_expected,
                        corrected.warnings
                    );
                }
                let anchors = c.active(time, true);
                anyhow::ensure!(
                    anchors.len() == initial_anchors.len(),
                    "Contact count changed across loops"
                );
                for (first, next) in initial_anchors.iter().zip(&anchors) {
                    anyhow::ensure!(
                        (next.anchor - first.anchor).abs_diff_eq(expected, tr.height * 1e-4),
                        "Contact anchor loop placement mismatch"
                    );
                }
                roots.push(serde_json::json!({"phase":phase,"cycle":cycle,"root":root.to_array(),"offset":(root-origin).to_array(),"ik_root":corrected_globals[plan.motion_target].w_axis.truncate().to_array(),"active_contacts":anchors.len(),"ik_feet":corrected.feet,"ik_warnings":corrected.warnings}));
            }
        }
        loops.push(serde_json::json!({"policy":policy,"cycle_displacement":c.cycle_displacement,"roots":roots,"contacts":c.intervals.len(),"warnings":c.warnings}));
    }
    std::fs::write(
        output,
        serde_json::to_vec_pretty(
            &serde_json::json!({"clip":clip.name,"duration":clip.duration,
        "source":describe(&source,&sr),
        "target":describe(&target,&tr),
        "motion_source":{"node":plan.motion_source,"name":source.nodes[plan.motion_source].name},
        "motion_target":{"node":plan.motion_target,"name":target.nodes[plan.motion_target].name},
        "plan":plan,"samples":samples,"loop_verification":loops}),
        )?,
    )?;
    println!(
        "Source/target pose and ten-loop placement analysis: PASS. {}",
        output.display()
    );
    Ok(())
}

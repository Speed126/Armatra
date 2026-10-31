use crate::{
    import,
    render::{Camera, Gpu, Renderer, Target},
    scene::{Vertex, skin_position},
    skinning,
};
use anyhow::{Result, ensure};
use glam::{Mat4, Quat, Vec3, Vec4};
use serde_json::json;
use wgpu::util::DeviceExt;

fn mapped(gpu: &Gpu, buffer: &wgpu::Buffer) -> Result<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv()??;
    let data = buffer.slice(..).get_mapped_range()?;
    let bytes = data.to_vec();
    drop(data);
    buffer.unmap();
    Ok(bytes)
}
pub fn run() -> Result<()> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let (gpu, _) = pollster::block_on(Gpu::new(&instance, None))?;
    ensure!(
        gpu.device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY),
        "Hardware timestamps unavailable: no GPU timing claim"
    );
    let renderer = Renderer::new(&gpu);
    let mut atlas = image::RgbaImage::new(1280, 720);
    std::fs::create_dir_all("tests/fixtures/motions")?;
    let bytes = crate::motion_fixtures::twist(false).finish();
    std::fs::write("tests/fixtures/motions/Twist-rigid.glb", &bytes)?;
    let asset = import::from_bytes(&bytes, &Default::default())?;
    let globals = asset.globals(&crate::animation::evaluate(
        &asset,
        &asset.clips[0],
        &[Some(0), Some(1), Some(2)],
        1.,
        &mut vec![],
    ));
    let resident = renderer.upload(&gpu, &asset)?;
    let mut camera = Camera::default();
    camera.fit(&asset, asset.default_scene);
    for (i, dqs) in [false, true].into_iter().enumerate() {
        renderer.dqs.set(dqs);
        let target = Target::new(&gpu.device, [640, 720]);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let warnings = renderer.render(
            &gpu,
            &mut encoder,
            &target,
            &camera,
            &asset,
            &resident,
            &globals,
            0,
            true,
            true,
        );
        ensure!(warnings.is_empty(), "Rigid DQS unexpectedly fell back");
        gpu.queue.submit([encoder.finish()]);
        let image = crate::diagnostics::read_target(&gpu, &target)?;
        image::imageops::replace(&mut atlas, &image, i as i64 * 640, 0);
    }
    atlas.save("target/reports/animation/lbs-dqs.png")?;
    let affine_bytes = crate::motion_fixtures::twist(true).finish();
    std::fs::write("tests/fixtures/motions/Twist-affine.glb", &affine_bytes)?;
    let affine = import::from_bytes(&affine_bytes, &Default::default())?;
    let resident = renderer.upload(&gpu, &affine)?;
    let target = Target::new(&gpu.device, [640, 720]);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let warnings = renderer.render(
        &gpu,
        &mut encoder,
        &target,
        &camera,
        &affine,
        &resident,
        &affine.globals(&affine.rest_pose()),
        0,
        true,
        true,
    );
    ensure!(
        warnings.iter().any(|w| w.contains("DQS fallback")),
        "Affine DQS must visibly fall back"
    );
    gpu.queue.submit([encoder.finish()]);
    crate::diagnostics::read_target(&gpu, &target)?
        .save("target/reports/animation/dqs-affine-lbs.png")?;
    let palette: Vec<_> = (0..8)
        .map(|i| {
            Mat4::from_rotation_translation(
                Quat::from_rotation_y(i as f32 * 0.37),
                Vec3::new(i as f32 * 0.01, 0.1, 0.),
            )
        })
        .collect();
    let count = 131072usize;
    let mut inputs = Vec::<[u32; 24]>::new();
    let mut vertices = Vec::new();
    for i in 0..count {
        let mut v = Vertex {
            position: [(i as f32 * 0.01).sin(), 0.3, 0.1],
            normal: [0., 1., 0.],
            uv: [0.; 2],
            joints: [0, 1, 2, 3, 4, 5, 6, 7],
            weights: [1.; 8],
        };
        if i % 3 == 0 {
            v.weights[4..].fill(0.);
        }
        if i % 11 == 0 {
            v.weights.fill(0.);
        }
        if i % 7 == 0 {
            v.joints[7] = 999;
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
    let source = include_str!("../shaders/mesh.wgsl")
        .replace("@vertex fn vs(v: Vertex)", "fn vs(v: Vertex)");
    let shader=gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor{label:Some("Actual vertex path skinning comparison"),source:wgpu::ShaderSource::Wgsl(format!("{source}\nstruct Input {{ p:vec4<f32>, n:vec4<f32>, j0:vec4<u32>,j1:vec4<u32>,w0:vec4<f32>,w1:vec4<f32> }}\n@group(2) @binding(0) var<storage,read> inputs:array<Input>; @group(2) @binding(1) var<storage,read_write> results:array<vec4<f32>>;\n@compute @workgroup_size(64) fn compare(@builtin(global_invocation_id) id:vec3<u32>) {{if id.x>=arrayLength(&inputs) {{return;}}let i=inputs[id.x];let v=Vertex(i.p.xyz,i.n.xyz,vec2(0.),i.j0,i.j1,i.w0,i.w1);let result=vs(v);results[id.x*2u]=result.position;results[id.x*2u+1u]=vec4(result.normal,0.);}}").into())});
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
    let data = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&inputs),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let output = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (count * 32) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (count * 32) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
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
    let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
        label: None,
        ty: wgpu::QueryType::Timestamp,
        count: 2,
    });
    let resolve = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let timing = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut rows = Vec::new();
    for dqs in [false, true] {
        let packed: Vec<_> = if dqs {
            palette
                .iter()
                .enumerate()
                .map(|(i, &m)| {
                    let (r, d) = skinning::dual_quaternion(m).unwrap();
                    let sign = if i % 2 == 0 { 1. } else { -1. };
                    Mat4::from_cols(
                        Vec4::from_array(r.to_array()) * sign,
                        Vec4::from_array(d.to_array()) * sign,
                        Vec4::ZERO,
                        Vec4::ZERO,
                    )
                })
                .collect()
        } else {
            palette.clone()
        };
        let palettes = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&packed),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let mut uniform = Vec::new();
        uniform.extend(Mat4::IDENTITY.to_cols_array());
        uniform.extend(Mat4::IDENTITY.to_cols_array());
        uniform.extend([1.; 4]);
        uniform.extend([1., 8., 0., 0.5]);
        uniform.extend([1., if dqs { 1. } else { 0. }, 0., 0.]);
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
        let mut times = Vec::new();
        for iteration in 0..36 {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                        query_set: &queries,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }),
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &group0, &[]);
                pass.set_bind_group(2, &group2, &[]);
                pass.dispatch_workgroups(count.div_ceil(64) as u32, 1, 1);
            }
            encoder.resolve_query_set(&queries, 0..2, &resolve, 0);
            encoder.copy_buffer_to_buffer(&resolve, 0, &timing, 0, 16);
            if iteration == 35 {
                encoder.copy_buffer_to_buffer(&output, 0, &read, 0, (count * 32) as u64);
            }
            gpu.queue.submit([encoder.finish()]);
            let bytes = mapped(&gpu, &timing)?;
            let stamps: &[u64] = bytemuck::cast_slice(&bytes);
            let ms = (stamps[1] - stamps[0]) as f64 * gpu.queue.get_timestamp_period() as f64 / 1e6;
            if iteration >= 6 {
                times.push(ms);
            }
        }
        let result = mapped(&gpu, &read)?;
        let floats: &[f32] = bytemuck::cast_slice(&result);
        let mut error = 0f32;
        let mut normal_error = 0f32;
        let duals: Vec<_> = palette
            .iter()
            .copied()
            .map(|m| skinning::dual_quaternion(m).unwrap())
            .collect();
        for (i, v) in vertices.iter().enumerate() {
            let actual = Vec3::new(floats[i * 8], floats[i * 8 + 1], floats[i * 8 + 2]);
            let expected = if dqs {
                skinning::position(v, &palette)
            } else {
                skin_position(v, &palette)
            };
            error = error.max(actual.distance(expected));
            let n = if dqs {
                skinning::blend(v, &duals).map_or(Vec3::Y, |(r, _)| r * Vec3::Y)
            } else {
                Vec3::Y
            };
            let actual = Vec3::new(floats[i * 8 + 4], floats[i * 8 + 5], floats[i * 8 + 6]);
            normal_error = normal_error.max(actual.distance(n));
        }
        ensure!(
            error < 1e-4 && normal_error < 1e-4,
            "CPU/GPU skinning error: {error}, {normal_error}"
        );
        times.sort_by(f64::total_cmp);
        rows.push(json!({"mode":if dqs{"DQS"}else{"LBS"},"vertices":count,"samples":times.len(),"gpu_p50_ms":times[15],"gpu_p95_ms":times[28],"max_cpu_gpu_position_error":error,"max_cpu_gpu_normal_error":normal_error}));
    }
    let vertex = Vertex {
        position: [0.4, 1., 0.],
        normal: [1., 0., 0.],
        uv: [0.; 2],
        joints: [0, 1, 0, 0, 0, 0, 0, 0],
        weights: [0.5, 0.5, 0., 0., 0., 0., 0., 0.],
    };
    let p = vec![Mat4::IDENTITY, Mat4::from_rotation_y(3.)];
    let lbs = skin_position(&vertex, &p);
    let dqs = skinning::position(&vertex, &p);
    crate::animation_bench::save(
        "skinning",
        &json!({"environment":crate::animation_bench::environment(),"gpu":{"name":gpu.info.name,"backend":format!("{:?}",gpu.info.backend),"driver":gpu.info.driver,"driver_info":gpu.info.driver_info},"method":"Actual mesh.wgsl vertex transform invoked by a compute harness, 131072 vertices, 4/8/zero weights and invalid indices; alternating antipodal DQ encodings; 6 warmup and 30 timestamp-query intervals per mode. Excludes rendering, CPU palette conversion/upload, and readback. LBS remains default.","rows":rows,"twist":{"rest_radius":0.4,"lbs_mid_radius":Vec3::new(lbs.x,0.,lbs.z).length(),"dqs_mid_radius":Vec3::new(dqs.x,0.,dqs.z).length()},"affine_fallback_warnings":warnings,"transition_decision":"No crossfade/inertialization promoted; optional transition changes need a separate utility/continuity study. Existing exact deterministic seek/clip-switch semantics retained."}),
    )?;
    println!(
        "LBS/DQS: hardware timings, {} CPU/GPU vertices, rigid comparison and affine fallback passed",
        count * 2
    );
    Ok(())
}

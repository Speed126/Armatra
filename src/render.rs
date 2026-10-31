use crate::scene::{Asset, Vertex};
use anyhow::{Context, Result};
use glam::{Mat4, Vec3};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use wgpu::util::DeviceExt;

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    vp: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    color: [f32; 4],
    options: [f32; 4],
    extra: [f32; 4],
}
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    pub(crate) lost: Arc<std::sync::Mutex<Option<String>>>,
}
impl Gpu {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<(Self, wgpu::Adapter)> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: surface,
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })
            .await
            .context("No compatible D3D12/Vulkan/Metal GPU. Update your graphics driver.")?;
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Armatra"),
                required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
                ..Default::default()
            })
            .await
            .context("GPU device initialization failed")?;
        crate::app_log::event(
            "gpu_initialized",
            serde_json::json!({"adapter":info.name,"backend":format!("{:?}",info.backend),"driver":info.driver,"driver_info":info.driver_info}),
        );
        device.on_uncaptured_error(Arc::new(|error| {
            crate::app_log::event(
                "gpu_uncaptured_error",
                serde_json::json!({"error":error.to_string()}),
            );
            // Preserve fatal validation/OOM semantics. Logging is not recovery.
            panic!("Unrecoverable graphics error: {error}");
        }));
        let lost = Arc::new(std::sync::Mutex::new(None));
        let callback_state = Arc::clone(&lost);
        device.set_device_lost_callback(move |reason, message| {
            crate::app_log::event(
                "gpu_device_lost",
                serde_json::json!({"reason":format!("{reason:?}"),"message":message}),
            );
            if reason != wgpu::DeviceLostReason::Destroyed
                && let Ok(mut state) = callback_state.lock()
            {
                *state = Some(message);
            }
        });
        Ok((
            Self {
                device,
                queue,
                info,
                lost,
            },
            adapter,
        ))
    }
}
#[derive(Clone)]
pub struct Camera {
    pub target: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub radius: f32,
    pub aspect: f32,
    pub needs_fit: bool,
    pub framing_bounds: Option<(Vec3, Vec3)>,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            target: Vec3::Y * 0.5,
            distance: 5.,
            yaw: 0.6,
            pitch: 0.35,
            radius: 1.,
            aspect: 1.,
            needs_fit: false,
            framing_bounds: None,
        }
    }
}
impl Camera {
    pub fn eye(&self) -> Vec3 {
        self.target
            + self.distance
                * Vec3::new(
                    self.yaw.sin() * self.pitch.cos(),
                    self.pitch.sin(),
                    self.yaw.cos() * self.pitch.cos(),
                )
    }
    pub fn matrix(&self, aspect: f32) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(
            45f32.to_radians(),
            aspect.max(0.01),
            (self.radius * 0.001).max(1e-9),
            (self.distance + self.radius * 100.).max(1e-6),
        ) * glam::camera::rh::view::look_at_mat4(self.eye(), self.target, Vec3::Y)
    }
    pub fn fit(&mut self, asset: &Asset, scene: usize) {
        self.fit_aspect(asset, scene, self.aspect);
    }
    pub fn fit_aspect(&mut self, asset: &Asset, scene: usize, aspect: f32) {
        self.aspect = aspect.clamp(0.01, 100.);
        let bounds = self
            .framing_bounds
            .or_else(|| asset.character_bounds(scene));
        if let Some((min, max)) = bounds {
            self.fit_bounds(min, max, aspect);
        }
        self.needs_fit = false;
    }
    pub fn fit_bounds(&mut self, min: Vec3, max: Vec3, aspect: f32) {
        self.aspect = aspect.clamp(0.01, 100.);
        self.target = (min + max) * 0.5;
        self.radius = (max - min).length().max(1e-6) * 0.5;
        let vertical = 22.5f32.to_radians();
        let horizontal = (vertical.tan() * self.aspect).atan();
        self.distance = self.radius / vertical.min(horizontal).sin() * 1.12;
        self.needs_fit = false;
    }
    pub fn interact(&mut self, response: &egui::Response, ui: &egui::Ui) {
        if ui.ctx().current_pass_index() != 0 {
            return;
        }
        let delta = ui.input(|i| i.pointer.delta());
        if response.dragged_by(egui::PointerButton::Primary) {
            self.yaw -= delta.x * 0.008;
            self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.5, 1.5);
        }
        if response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary)
        {
            let right = Vec3::new(self.yaw.cos(), 0., -self.yaw.sin());
            let up = (self.eye() - self.target).normalize().cross(right);
            self.target += (-right * delta.x + up * delta.y) * self.distance * 0.0015;
        }
        if response.hovered() {
            self.distance = (self.distance
                * (-ui.input(|i| i.smooth_scroll_delta.y) * 0.002).exp())
            .clamp(self.radius * 0.01, self.radius * 1000.);
        }
    }
}
pub struct Target {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sample: wgpu::TextureView,
    pub depth: wgpu::TextureView,
    pub size: [u32; 2],
}
impl Target {
    pub fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let desc = wgpu::TextureDescriptor {
            label: Some("Viewport"),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        };
        let texture = device.create_texture(&desc);
        let view = texture.create_view(&Default::default());
        // egui expects encoded UI colors; keep linear-light rendering, but avoid
        // decoding sRGB a second time when the viewport is composited into egui.
        let sample = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            ..Default::default()
        });
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Depth"),
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
                ..desc
            })
            .create_view(&Default::default());
        Self {
            texture,
            view,
            sample,
            depth,
            size,
        }
    }
}
struct Geometry {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    material: usize,
    skinned: bool,
}
struct Draw {
    node: usize,
    mesh: usize,
    primitive: usize,
    uniform: wgpu::Buffer,
    palette: wgpu::Buffer,
    bind: wgpu::BindGroup,
}
pub struct GpuAsset {
    meshes: Vec<Vec<Geometry>>,
    materials: Vec<wgpu::BindGroup>,
    draws: Vec<Draw>,
    pub bytes: usize,
    pub fitted_camera: Camera,
    pub grids: Vec<crate::grid::Grid>,
    live: Arc<Live>,
}
#[derive(Default)]
struct Live {
    assets: AtomicUsize,
    bytes: AtomicUsize,
    uploads: AtomicUsize,
}
impl Drop for GpuAsset {
    fn drop(&mut self) {
        self.live.assets.fetch_sub(1, Ordering::Relaxed);
        self.live.bytes.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct SkinningReport {
    pub requested_dqs: bool,
    pub dqs: usize,
    pub lbs: usize,
    pub unskinned: usize,
    pub hidden: usize,
}
pub struct Renderer {
    pub dqs: std::cell::Cell<bool>,
    pub show_grid: std::cell::Cell<bool>,
    pub skinning_report: std::cell::Cell<SkinningReport>,
    opaque: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    grid_pipeline: wgpu::RenderPipeline,
    draw_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    line_uniform: wgpu::Buffer,
    line_bind: wgpu::BindGroup,
    grid: wgpu::Buffer,
    grid_count: u32,
    grid_uniform: wgpu::Buffer,
    grid_bind: wgpu::BindGroup,
    skeleton: wgpu::Buffer,
    white: wgpu::BindGroup,
    live: Arc<Live>,
}
impl Renderer {
    pub fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Instance"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Material"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Mesh pipeline"),
            bind_group_layouts: &[Some(&draw_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("LBS / diffuse / lines"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/mesh.wgsl").into()),
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Uint32x4, 4 => Uint32x4, 5 => Float32x4, 6 => Float32x4];
        let pipeline = |line: bool, blend: bool, overlay: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Armatra geometry"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(if line { "line_vs" } else { "vs" }),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attrs,
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if line { "line_fs" } else { "fs" }),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: if blend {
                            Some(wgpu::BlendState::ALPHA_BLENDING)
                        } else {
                            None
                        },
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: if line {
                        wgpu::PrimitiveTopology::LineList
                    } else {
                        wgpu::PrimitiveTopology::TriangleList
                    },
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(!blend && !line),
                    depth_compare: Some(if overlay {
                        wgpu::CompareFunction::Always
                    } else {
                        wgpu::CompareFunction::LessEqual
                    }),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let opaque = pipeline(false, false, false);
        let blend = pipeline(false, true, false);
        let lines = pipeline(true, false, true);
        let grid_pipeline = pipeline(true, false, false);
        let line_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Line transforms"),
            size: size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let identity = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&[Mat4::IDENTITY]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let line_bind = bind_draw(device, &draw_layout, &line_uniform, &identity);
        let grid_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Visual grid transforms"),
            size: size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let grid_bind = bind_draw(device, &draw_layout, &grid_uniform, &identity);
        let mut vertices = Vec::new();
        for i in -8..=8 {
            let n = i as f32 * 0.5;
            let c = if i == 0 {
                crate::ui::theme::GRID_MAJOR
            } else {
                crate::ui::theme::GRID
            };
            for segment in -8..8 {
                let a = segment as f32 * 0.5;
                let b = a + 0.5;
                for (from, to) in [([a, 0., n], [b, 0., n]), ([n, 0., a], [n, 0., b])] {
                    for point in [from, to] {
                        let fade = (1. - (point[0] * point[0] + point[2] * point[2]).sqrt() / 4.2)
                            .clamp(0., 1.)
                            .powi(2);
                        let color = std::array::from_fn(|axis| {
                            crate::ui::theme::STAGE[axis] as f32
                                + (c[axis] - crate::ui::theme::STAGE[axis] as f32) * fade
                        });
                        // Existing unlit vertex-color lines; no shader or postprocess effect.
                        vertices.push(Vertex {
                            position: point,
                            normal: color,
                            uv: [0.; 2],
                            joints: [0; 8],
                            weights: [0.; 8],
                        });
                    }
                }
            }
        }
        let grid_count = vertices.len() as u32;
        let grid = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Grid/axes"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let skeleton = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Skeleton"),
            size: (65_536 * size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let white = material_bind(
            gpu,
            &material_layout,
            None,
            crate::scene::Sampler::default(),
        );
        Self {
            dqs: std::cell::Cell::new(false),
            show_grid: std::cell::Cell::new(true),
            skinning_report: std::cell::Cell::new(SkinningReport::default()),
            opaque,
            blend,
            lines,
            grid_pipeline,
            draw_layout,
            material_layout,
            line_uniform,
            line_bind,
            grid,
            grid_count,
            grid_uniform,
            grid_bind,
            skeleton,
            white,
            live: Arc::new(Live::default()),
        }
    }
    pub fn upload(&self, gpu: &Gpu, asset: &Asset) -> Result<Arc<GpuAsset>> {
        let limits = gpu.device.limits();
        for image in &asset.images {
            anyhow::ensure!(
                image.width > 0
                    && image.height > 0
                    && image.width <= limits.max_texture_dimension_2d
                    && image.height <= limits.max_texture_dimension_2d,
                "GPU texture dimensions {}×{} exceed this device's {} limit",
                image.width,
                image.height,
                limits.max_texture_dimension_2d
            );
        }
        for p in asset.meshes.iter().flat_map(|m| &m.primitives) {
            anyhow::ensure!(
                (p.vertices.len() * size_of::<Vertex>()) as u64 <= limits.max_buffer_size
                    && (p.indices.len() * 4) as u64 <= limits.max_buffer_size,
                "GPU geometry buffer exceeds this device's resource limit"
            );
        }
        let geometry: usize = asset
            .meshes
            .iter()
            .flat_map(|m| &m.primitives)
            .map(|p| p.vertices.len() * size_of::<Vertex>() + p.indices.len() * 4)
            .sum();
        let textures: usize = asset
            .materials
            .iter()
            .map(|m| m.image.map_or(4, |i| asset.images[i].rgba.len()))
            .sum();
        let draws: usize = asset
            .nodes
            .iter()
            .filter_map(|n| {
                n.mesh.map(|m| {
                    asset.meshes[m].primitives.len()
                        * (size_of::<Uniform>()
                            + 512
                            + n.skin.map_or(1, |s| asset.skins[s].joints.len()) * 64)
                })
            })
            .sum();
        let bytes = geometry + textures + draws + asset.materials.len() * 256;
        if bytes > 512 * 1024 * 1024 {
            anyhow::bail!("GPU upload exceeds 512 MiB per-asset limit");
        }
        let mut fitted_camera = Camera::default();
        fitted_camera.fit(asset, asset.default_scene);
        let device = &gpu.device;
        let meshes = asset
            .meshes
            .iter()
            .map(|m| {
                m.primitives
                    .iter()
                    .map(|p| Geometry {
                        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("Mesh vertices"),
                            contents: bytemuck::cast_slice(&p.vertices),
                            usage: wgpu::BufferUsages::VERTEX,
                        }),
                        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("Mesh indices"),
                            contents: bytemuck::cast_slice(&p.indices),
                            usage: wgpu::BufferUsages::INDEX,
                        }),
                        count: p.indices.len() as u32,
                        material: p.material,
                        skinned: p.has_skin,
                    })
                    .collect()
            })
            .collect();
        let materials = asset
            .materials
            .iter()
            .map(|m| {
                material_bind(
                    gpu,
                    &self.material_layout,
                    m.image.map(|i| &asset.images[i]),
                    m.sampler,
                )
            })
            .collect();
        let mut draws = Vec::new();
        for (node, n) in asset.nodes.iter().enumerate() {
            if let Some(mesh) = n.mesh {
                for primitive in 0..asset.meshes[mesh].primitives.len() {
                    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Draw uniform"),
                        size: size_of::<Uniform>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let palette = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Instance skin palette"),
                        size: (n.skin.map_or(1, |s| asset.skins[s].joints.len()) * 64) as u64,
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let bind = bind_draw(device, &self.draw_layout, &uniform, &palette);
                    draws.push(Draw {
                        node,
                        mesh,
                        primitive,
                        uniform,
                        palette,
                        bind,
                    });
                }
            }
        }
        self.live.assets.fetch_add(1, Ordering::Relaxed);
        self.live.bytes.fetch_add(bytes, Ordering::Relaxed);
        self.live.uploads.fetch_add(1, Ordering::Relaxed);
        Ok(Arc::new(GpuAsset {
            meshes,
            materials,
            draws,
            bytes,
            fitted_camera,
            grids: (0..asset.scenes.len())
                .map(|scene| crate::grid::Grid::for_asset(asset, scene))
                .collect(),
            live: Arc::clone(&self.live),
        }))
    }
    pub fn resource_stats(&self) -> (usize, usize, usize) {
        (
            self.live.assets.load(Ordering::Relaxed),
            self.live.bytes.load(Ordering::Relaxed),
            self.live.uploads.load(Ordering::Relaxed),
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &Target,
        camera: &Camera,
        asset: &Asset,
        resident: &GpuAsset,
        globals: &[Mat4],
        scene: usize,
        skeleton: bool,
        mesh: bool,
    ) -> Vec<String> {
        let vp = camera.matrix(target.size[0] as f32 / target.size[1].max(1) as f32);
        let uniform = |model: Mat4, color, options, double_sided: bool, dqs: bool| Uniform {
            vp: vp.to_cols_array_2d(),
            model: model.to_cols_array_2d(),
            color,
            options,
            extra: [
                u32::from(double_sided) as f32,
                u32::from(dqs) as f32,
                0.,
                0.,
            ],
        };
        gpu.queue.write_buffer(
            &self.line_uniform,
            0,
            bytemuck::bytes_of(&uniform(Mat4::IDENTITY, [1.; 4], [0.; 4], true, false)),
        );
        let visible = asset.visible_nodes(scene);
        let grid = resident
            .grids
            .get(scene)
            .copied()
            .unwrap_or_else(|| crate::grid::Grid::from_size(2.));
        gpu.queue.write_buffer(
            &self.grid_uniform,
            0,
            bytemuck::bytes_of(&uniform(
                Mat4::from_scale(Vec3::splat(grid.interval / 0.5)),
                [1.; 4],
                [0.; 4],
                true,
                false,
            )),
        );
        let visible_set: std::collections::HashSet<_> = visible.iter().copied().collect();
        let mut warnings = Vec::new();
        let mut report = SkinningReport {
            requested_dqs: self.dqs.get(),
            ..Default::default()
        };
        let mut ready = Vec::new();
        for (index, draw) in resident.draws.iter().enumerate() {
            if !mesh || !visible_set.contains(&draw.node) {
                continue;
            }
            if !globals[draw.node].is_finite() {
                report.hidden += 1;
                warnings.push(format!(
                    "Node {}: animated transform is outside finite range; instance hidden",
                    draw.node
                ));
                continue;
            }
            let primitive = &resident.meshes[draw.mesh][draw.primitive];
            let mat = &asset.materials[primitive.material];
            let palette = match asset.palette(draw.node, globals) {
                Ok(p) => p,
                Err(e) => {
                    report.hidden += 1;
                    warnings.push(e);
                    continue;
                }
            };
            let skinned = primitive.skinned && asset.nodes[draw.node].skin.is_some();
            let dual = if self.dqs.get() && skinned {
                crate::skinning::packed_palette(asset, draw.node, globals, &palette)
            } else {
                None
            };
            if self.dqs.get() && skinned && dual.is_none() {
                warnings.push(format!("Node {}: DQS fallback to LBS (affine, scale, reflection or non-rigid skin frame)",draw.node));
            }
            if !skinned {
                report.unskinned += 1;
            } else if dual.is_some() {
                report.dqs += 1;
            } else {
                report.lbs += 1;
            }
            gpu.queue.write_buffer(
                &draw.palette,
                0,
                bytemuck::cast_slice(dual.as_ref().unwrap_or(&palette)),
            );
            gpu.queue.write_buffer(
                &draw.uniform,
                0,
                bytemuck::bytes_of(&uniform(
                    globals[draw.node],
                    mat.color,
                    [
                        u32::from(skinned) as f32,
                        palette.len() as f32,
                        mat.alpha_mode as f32,
                        mat.cutoff,
                    ],
                    mat.double_sided,
                    dual.is_some(),
                )),
            );
            ready.push(index);
        }
        self.skinning_report.set(report);
        ready.sort_by(|&a, &b| {
            let da = &resident.draws[a];
            let db = &resident.draws[b];
            let ma = &asset.materials[resident.meshes[da.mesh][da.primitive].material];
            let mb = &asset.materials[resident.meshes[db.mesh][db.primitive].material];
            (ma.alpha_mode == 2)
                .cmp(&(mb.alpha_mode == 2))
                .then_with(|| {
                    if ma.alpha_mode == 2 {
                        globals[db.node]
                            .w_axis
                            .truncate()
                            .distance_squared(camera.eye())
                            .total_cmp(
                                &globals[da.node]
                                    .w_axis
                                    .truncate()
                                    .distance_squared(camera.eye()),
                            )
                    } else {
                        a.cmp(&b)
                    }
                })
        });
        let mut bones = Vec::new();
        if skeleton {
            for &i in &visible {
                if let Some(p) = asset.nodes[i].parent {
                    line(
                        &mut bones,
                        globals[p].w_axis.truncate().into(),
                        globals[i].w_axis.truncate().into(),
                        crate::ui::theme::BONE,
                    );
                }
            }
        }
        if !bones.is_empty() {
            gpu.queue
                .write_buffer(&self.skeleton, 0, bytemuck::cast_slice(&bones));
        }
        let colors = [Some(wgpu::RenderPassColorAttachment {
            view: &target.view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: crate::ui::theme::STAGE[0],
                    g: crate::ui::theme::STAGE[1],
                    b: crate::ui::theme::STAGE[2],
                    a: 1.,
                }),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("3D Viewport"),
            color_attachments: &colors,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        for index in ready {
            let draw = &resident.draws[index];
            let p = &resident.meshes[draw.mesh][draw.primitive];
            pass.set_pipeline(if asset.materials[p.material].alpha_mode == 2 {
                &self.blend
            } else {
                &self.opaque
            });
            pass.set_bind_group(0, &draw.bind, &[]);
            pass.set_bind_group(1, &resident.materials[p.material], &[]);
            pass.set_vertex_buffer(0, p.vertices.slice(..));
            pass.set_index_buffer(p.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..p.count, 0, 0..1);
        }
        pass.set_pipeline(&self.grid_pipeline);
        pass.set_bind_group(0, &self.grid_bind, &[]);
        pass.set_bind_group(1, &self.white, &[]);
        pass.set_vertex_buffer(0, self.grid.slice(..));
        if self.show_grid.get() {
            pass.draw(0..self.grid_count, 0..1);
        }
        if !bones.is_empty() {
            pass.set_pipeline(&self.lines);
            pass.set_bind_group(0, &self.line_bind, &[]);
            pass.set_vertex_buffer(0, self.skeleton.slice(..));
            pass.draw(0..bones.len() as u32, 0..1);
        }
        warnings
    }
}
fn bind_draw(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    palette: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: palette.as_entire_binding(),
            },
        ],
    })
}
fn material_bind(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    image: Option<&crate::scene::Image>,
    sampler: crate::scene::Sampler,
) -> wgpu::BindGroup {
    let (width, height, rgba) = image.map_or((1, 1, &[255u8; 4][..]), |i| {
        (i.width, i.height, i.rgba.as_slice())
    });
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sRGB base color"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        texture.size(),
    );
    let address = |v| match v {
        33071 => wgpu::AddressMode::ClampToEdge,
        33648 => wgpu::AddressMode::MirrorRepeat,
        _ => wgpu::AddressMode::Repeat,
    };
    let filter = if sampler.nearest {
        wgpu::FilterMode::Nearest
    } else {
        wgpu::FilterMode::Linear
    };
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: address(sampler.wrap_s),
        address_mode_v: address(sampler.wrap_t),
        mag_filter: filter,
        min_filter: filter,
        ..Default::default()
    });
    let view = texture.create_view(&Default::default());
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    })
}
fn line(vertices: &mut Vec<Vertex>, a: [f32; 3], b: [f32; 3], color: [f32; 3]) {
    for p in [a, b] {
        vertices.push(Vertex {
            position: p,
            normal: color,
            uv: [0.; 2],
            joints: [0; 8],
            weights: [0.; 8],
        });
    }
}

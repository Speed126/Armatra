use glam::{Mat4, Quat, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct Trs {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}
impl Default for Trs {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Local {
    Trs(Trs),
    Matrix(Mat4),
}
impl Local {
    pub fn matrix(self) -> Mat4 {
        match self {
            Self::Trs(t) => {
                Mat4::from_scale_rotation_translation(t.scale, t.rotation, t.translation)
            }
            Self::Matrix(m) => m,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub rest: Local,
    pub mesh: Option<usize>,
    pub skin: Option<usize>,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub joints: [u32; 8],
    pub weights: [f32; 8],
}
#[derive(Clone, Debug)]
pub struct Primitive {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub material: usize,
    pub has_skin: bool,
}
#[derive(Clone, Debug)]
pub struct Mesh {
    pub primitives: Vec<Primitive>,
}
#[derive(Clone, Debug)]
pub struct Skin {
    pub joints: Vec<usize>,
    pub inverse_bind: Vec<Mat4>,
}
#[derive(Clone, Debug)]
pub struct Material {
    pub color: [f32; 4],
    pub image: Option<usize>,
    pub alpha_mode: u32,
    pub cutoff: f32,
    pub double_sided: bool,
    pub sampler: Sampler,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Sampler {
    pub wrap_s: u32,
    pub wrap_t: u32,
    pub nearest: bool,
}
impl Default for Material {
    fn default() -> Self {
        Self {
            color: [1.; 4],
            image: None,
            alpha_mode: 0,
            cutoff: 0.5,
            double_sided: false,
            sampler: Sampler::default(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct Scene {
    pub name: String,
    pub roots: Vec<usize>,
}
#[derive(Clone, Debug, Default)]
pub struct Asset {
    pub nodes: Vec<Node>,
    pub order: Vec<usize>,
    pub scenes: Vec<Scene>,
    pub default_scene: usize,
    pub meshes: Vec<Mesh>,
    pub skins: Vec<Skin>,
    pub materials: Vec<Material>,
    pub images: Vec<Image>,
    pub clips: Vec<crate::animation::Clip>,
    pub warnings: Vec<String>,
    pub exact_mesh: bool,
    pub default_bounds: Option<(Vec3, Vec3)>,
}
impl Asset {
    pub fn compute_bounds(&self, scene: usize) -> Option<(Vec3, Vec3)> {
        self.geometry_bounds(scene, false)
    }
    pub fn character_bounds(&self, scene: usize) -> Option<(Vec3, Vec3)> {
        self.geometry_bounds(scene, true)
            .or_else(|| self.compute_bounds(scene))
            .or_else(|| {
                let globals = self.globals(&self.rest_pose());
                let rig = crate::rig::Rig::analyze(self);
                let mut min = Vec3::splat(f32::INFINITY);
                let mut max = Vec3::splat(f32::NEG_INFINITY);
                for j in rig.joints {
                    let p = globals[j.node].w_axis.truncate();
                    if p.is_finite() {
                        min = min.min(p);
                        max = max.max(p);
                    }
                }
                (min.is_finite() && max.is_finite()).then_some((min, max))
            })
    }
    pub fn visible_rest_bounds(&self, scene: usize) -> Option<(Vec3, Vec3)> {
        self.geometry_bounds(scene, true)
            .or_else(|| self.geometry_bounds(scene, false))
    }
    fn geometry_bounds(&self, scene: usize, skinned_only: bool) -> Option<(Vec3, Vec3)> {
        self.geometry_pose_bounds(scene, skinned_only, &self.rest_pose())
    }
    pub fn character_pose_bounds(&self, scene: usize, pose: &[Local]) -> Option<(Vec3, Vec3)> {
        self.geometry_pose_bounds(scene, true, pose)
            .or_else(|| self.geometry_pose_bounds(scene, false, pose))
            .or_else(|| {
                let global = self.globals(pose);
                let mut min = Vec3::splat(f32::INFINITY);
                let mut max = Vec3::splat(f32::NEG_INFINITY);
                for node in self.visible_nodes(scene) {
                    let p = global[node].w_axis.truncate();
                    if p.is_finite() {
                        min = min.min(p);
                        max = max.max(p);
                    }
                }
                (min.is_finite() && max.is_finite()).then_some((min, max))
            })
    }
    pub(crate) fn geometry_pose_bounds(
        &self,
        scene: usize,
        skinned_only: bool,
        pose: &[Local],
    ) -> Option<(Vec3, Vec3)> {
        let global = self.globals(pose);
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for node in self.visible_nodes(scene) {
            if skinned_only && self.nodes[node].skin.is_none() {
                continue;
            }
            let Some(mesh) = self.nodes[node].mesh else {
                continue;
            };
            let palette = match self.palette(node, &global) {
                Ok(p) => p,
                Err(_) => continue,
            };
            for primitive in &self.meshes[mesh].primitives {
                for &index in &primitive.indices {
                    let vertex = &primitive.vertices[index as usize];
                    let local = if self.nodes[node].skin.is_some() && primitive.has_skin {
                        skin_position(vertex, &palette)
                    } else {
                        vertex.position.into()
                    };
                    let p = global[node].transform_point3(local);
                    if p.is_finite() {
                        min = min.min(p);
                        max = max.max(p);
                    }
                }
            }
        }
        if min.is_finite() && max.is_finite() {
            Some((min, max))
        } else {
            None
        }
    }
    pub fn rest_pose(&self) -> Vec<Local> {
        self.nodes.iter().map(|n| n.rest).collect()
    }
    pub fn globals(&self, local: &[Local]) -> Vec<Mat4> {
        let mut global = vec![Mat4::IDENTITY; self.nodes.len()];
        for &i in &self.order {
            let m = local[i].matrix();
            global[i] = self.nodes[i].parent.map_or(m, |p| global[p] * m);
        }
        global
    }
    pub fn visible_nodes(&self, scene: usize) -> Vec<usize> {
        let mut visible = vec![false; self.nodes.len()];
        if let Some(scene) = self.scenes.get(scene) {
            for &root in &scene.roots {
                visible[root] = true;
            }
            for &i in &self.order {
                if self.nodes[i].parent.is_some_and(|p| visible[p]) {
                    visible[i] = true;
                }
            }
        }
        self.order.iter().copied().filter(|&i| visible[i]).collect()
    }
    pub fn palette(&self, node: usize, global: &[Mat4]) -> Result<Vec<Mat4>, String> {
        let n = &self.nodes[node];
        let Some(skin) = n.skin else {
            return Ok(vec![Mat4::IDENTITY]);
        };
        let mesh = global[node];
        if !mesh.determinant().is_finite()
            || mesh.determinant() == 0.
            || !mesh.inverse().is_finite()
        {
            return Err(format!(
                "Node {node}: non-invertible mesh transform; skin hidden"
            ));
        }
        let inverse = mesh.inverse();
        let palette: Vec<_> = self.skins[skin]
            .joints
            .iter()
            .zip(&self.skins[skin].inverse_bind)
            .map(|(&j, &ibm)| inverse * global[j] * ibm)
            .collect();
        if palette.iter().any(|m| !m.is_finite()) {
            return Err(format!(
                "Node {node}: non-finite animated skin palette; instance hidden"
            ));
        }
        Ok(palette)
    }
    pub fn cpu_bytes(&self) -> usize {
        self.meshes
            .iter()
            .flat_map(|m| &m.primitives)
            .map(|p| p.vertices.len() * size_of::<Vertex>() + p.indices.len() * 4)
            .sum::<usize>()
            + self.images.iter().map(|i| i.rgba.len()).sum::<usize>()
            + self
                .clips
                .iter()
                .flat_map(|c| &c.tracks)
                .map(|t| t.times.len() * 4 + t.values.len() * 16)
                .sum::<usize>()
            + self.nodes.len() * size_of::<Node>()
    }
}
pub fn skin_position(vertex: &Vertex, palette: &[Mat4]) -> Vec3 {
    let mut result = Vec3::ZERO;
    let mut sum = 0.;
    for (&j, &w) in vertex.joints.iter().zip(&vertex.weights) {
        if w > 0.
            && let Some(p) = palette.get(j as usize)
        {
            result += p.transform_point3(Vec3::from(vertex.position)) * w;
            sum += w;
        }
    }
    if sum > 1e-8 {
        result / sum
    } else {
        Vec3::from(vertex.position)
    }
}

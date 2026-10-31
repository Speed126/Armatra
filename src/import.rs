//! Bounded, self-contained GLB 2.0 import with checked dense and sparse accessors.
mod formats;
use crate::{
    animation::{Clip, Interpolation, Property, Track},
    scene::*,
};
pub use formats::from_path;
use glam::{Mat4, Quat, Vec3, Vec4};
use gltf::accessor::{DataType, Dimensions};
use std::{
    collections::{HashSet, VecDeque},
    io::{Cursor, Read},
    path::Path,
};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("Invalid GLB: {0}")]
    Container(String),
    #[error("Invalid accessor {index}: {reason}")]
    Accessor { index: usize, reason: String },
    #[error("Invalid hierarchy: {0}")]
    Hierarchy(String),
    #[error("Invalid skin: {0}")]
    Skin(String),
    #[error("Invalid animation: {0}")]
    Animation(String),
    #[error("Unsupported feature: {0}")]
    Unsupported(String),
    #[error("Resource limit exceeded: {0}")]
    Limit(String),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}
type Result<T> = std::result::Result<T, ImportError>;
#[derive(Clone, Debug)]
pub struct Limits {
    pub file_bytes: usize,
    pub elements: usize,
    pub nodes: usize,
    pub image_side: u32,
    pub decoded_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            file_bytes: 128 * 1024 * 1024,
            elements: 16_000_000,
            nodes: 32_768,
            image_side: 4096,
            decoded_bytes: 256 * 1024 * 1024,
        }
    }
}
pub fn read_file(path: &Path, limits: &Limits) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > limits.file_bytes as u64 {
        return Err(ImportError::Limit("file bytes".into()));
    }
    let mut bytes = Vec::new();
    file.take(limits.file_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limits.file_bytes {
        return Err(ImportError::Limit("file bytes".into()));
    }
    Ok(bytes)
}
pub fn container<'a>(bytes: &'a [u8], limits: &Limits) -> Result<(&'a [u8], &'a [u8])> {
    let bad = |m: &str| ImportError::Container(m.into());
    if bytes.len() > limits.file_bytes {
        return Err(ImportError::Limit("file bytes".into()));
    }
    if bytes.len() < 20 || &bytes[..4] != b"glTF" {
        return Err(bad("missing header/JSON chunk"));
    }
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().expect("bounded header"));
    if u32_at(4) != 2 || u32_at(8) as usize != bytes.len() {
        return Err(bad("version or declared length"));
    }
    let mut offset = 12usize;
    let mut json = None;
    let mut bin = None;
    while offset < bytes.len() {
        if bytes.len() - offset < 8 {
            return Err(bad("truncated chunk header"));
        }
        let len = u32_at(offset) as usize;
        let kind = u32_at(offset + 4);
        let start = offset + 8;
        let end = start
            .checked_add(len)
            .filter(|&v| v <= bytes.len())
            .ok_or_else(|| bad("chunk outside file"))?;
        if !len.is_multiple_of(4) {
            return Err(bad("chunk alignment"));
        }
        if json.is_none() && kind != 0x4e4f534a {
            return Err(bad("first chunk must be JSON"));
        }
        match kind {
            0x4e4f534a => {
                if json.replace(&bytes[start..end]).is_some() {
                    return Err(bad("duplicate JSON"));
                }
            }
            0x004e4942 if bin.replace(&bytes[start..end]).is_some() => {
                return Err(bad("duplicate BIN"));
            }
            0x004e4942 => (),
            _ => (), // GLB permits unknown chunks; they are never interpreted.
        }
        offset = end;
    }
    Ok((json.ok_or_else(|| bad("missing JSON"))?, bin.unwrap_or(&[])))
}
fn view_bytes<'a>(view: gltf::buffer::View<'_>, bin: &'a [u8]) -> Result<&'a [u8]> {
    let start = view.offset();
    let end = start
        .checked_add(view.length())
        .ok_or_else(|| ImportError::Container("view overflow".into()))?;
    bin.get(start..end)
        .ok_or_else(|| ImportError::Container("view outside BIN".into()))
}
fn component(bytes: &[u8], kind: DataType, normalized: bool) -> f32 {
    match kind {
        DataType::I8 => {
            let v = bytes[0] as i8;
            if normalized {
                (v as f32 / 127.).max(-1.)
            } else {
                v as f32
            }
        }
        DataType::U8 => {
            let v = bytes[0];
            if normalized {
                v as f32 / 255.
            } else {
                v as f32
            }
        }
        DataType::I16 => {
            let v = i16::from_le_bytes([bytes[0], bytes[1]]);
            if normalized {
                (v as f32 / 32767.).max(-1.)
            } else {
                v as f32
            }
        }
        DataType::U16 => {
            let v = u16::from_le_bytes([bytes[0], bytes[1]]);
            if normalized {
                v as f32 / 65535.
            } else {
                v as f32
            }
        }
        DataType::U32 => u32::from_le_bytes(bytes.try_into().expect("component size")) as f32,
        DataType::F32 => f32::from_le_bytes(bytes.try_into().expect("component size")),
    }
}
pub fn accessor(a: gltf::Accessor<'_>, bin: &[u8], limits: &Limits) -> Result<Vec<Vec<f32>>> {
    let error = |reason: &str| ImportError::Accessor {
        index: a.index(),
        reason: reason.into(),
    };
    let count = a.count();
    let dims = a.dimensions();
    let kind = a.data_type();
    let size = kind.size();
    let width = dims.multiplicity();
    let (columns, rows): (usize, usize) = match dims {
        Dimensions::Mat2 => (2, 2),
        Dimensions::Mat3 => (3, 3),
        Dimensions::Mat4 => (4, 4),
        _ => (1, width),
    };
    let column_bytes = if columns > 1 {
        (rows * size).div_ceil(4) * 4
    } else {
        rows * size
    };
    let element_bytes = columns * column_bytes;
    if count.checked_mul(width).is_none_or(|n| n > limits.elements) {
        return Err(ImportError::Limit("accessor components".into()));
    }
    if a.normalized() && matches!(kind, DataType::F32 | DataType::U32) {
        return Err(error("invalid normalized component type"));
    }
    let decode = |data: &[u8], offset: usize| -> Result<Vec<f32>> {
        let mut values = Vec::with_capacity(width);
        for c in 0..columns {
            for r in 0..rows {
                let start = offset
                    .checked_add(c * column_bytes + r * size)
                    .ok_or_else(|| error("component overflow"))?;
                let bytes = data
                    .get(start..start + size)
                    .ok_or_else(|| error("component outside view"))?;
                let v = component(bytes, kind, a.normalized());
                if !v.is_finite() {
                    return Err(error("non-finite component"));
                }
                values.push(v);
            }
        }
        Ok(values)
    };
    let mut out = vec![vec![0.; width]; count];
    if let Some(view) = a.view() {
        let stride = view.stride().unwrap_or(element_bytes);
        if stride < element_bytes
            || stride % size != 0
            || !a.offset().is_multiple_of(size)
            || !(view.offset() + a.offset()).is_multiple_of(size)
        {
            return Err(error("invalid stride/alignment"));
        }
        let data = view_bytes(view, bin)?;
        let end = if count == 0 {
            a.offset()
        } else {
            (count - 1)
                .checked_mul(stride)
                .and_then(|n| n.checked_add(a.offset()))
                .and_then(|n| n.checked_add(element_bytes))
                .ok_or_else(|| error("range overflow"))?
        };
        if end > data.len() {
            return Err(error("range exceeds buffer view"));
        }
        for (i, values) in out.iter_mut().enumerate() {
            *values = decode(data, a.offset() + i * stride)?;
        }
    } else if a.offset() != 0 {
        return Err(error("offset without buffer view"));
    }
    if let Some(sparse) = a.sparse() {
        if sparse.count() > count {
            return Err(error("sparse count"));
        }
        let indices = sparse.indices();
        let values = sparse.values();
        let index_size = indices.index_type().size();
        let ids = view_bytes(indices.view(), bin)?;
        let vals = view_bytes(values.view(), bin)?;
        let mut previous = None;
        for i in 0..sparse.count() {
            let offset = i
                .checked_mul(index_size)
                .and_then(|v| v.checked_add(indices.offset()))
                .ok_or_else(|| error("sparse overflow"))?;
            let raw = ids
                .get(offset..offset + index_size)
                .ok_or_else(|| error("sparse index range"))?;
            let id = match index_size {
                1 => raw[0] as usize,
                2 => u16::from_le_bytes(raw.try_into().expect("size")) as usize,
                _ => u32::from_le_bytes(raw.try_into().expect("size")) as usize,
            };
            if id >= count || previous.is_some_and(|p| id <= p) {
                return Err(error("sparse indices must be increasing and in range"));
            }
            previous = Some(id);
            let offset = i
                .checked_mul(element_bytes)
                .and_then(|v| v.checked_add(values.offset()))
                .ok_or_else(|| error("sparse values overflow"))?;
            out[id] = decode(vals, offset)?;
        }
    }
    Ok(out)
}
fn require(
    a: &gltf::Accessor<'_>,
    dims: Dimensions,
    kinds: &[DataType],
    normalized: Option<bool>,
) -> Result<()> {
    if a.dimensions() != dims
        || !kinds.contains(&a.data_type())
        || normalized.is_some_and(|n| a.normalized() != n)
    {
        return Err(ImportError::Accessor {
            index: a.index(),
            reason: "semantic component type/shape".into(),
        });
    }
    Ok(())
}
pub fn from_bytes(bytes: &[u8], limits: &Limits) -> Result<Asset> {
    let (json, bin) = container(bytes, limits)?;
    let raw: serde_json::Value =
        serde_json::from_slice(json).map_err(|e| ImportError::Container(e.to_string()))?;
    if let Some(required) = raw.get("extensionsRequired").and_then(|v| v.as_array())
        && required
            .iter()
            .any(|v| v.as_str() != Some("KHR_materials_pbrSpecularGlossiness"))
    {
        return Err(ImportError::Unsupported(format!(
            "required extensions: {required:?}"
        )));
    }
    let glb = gltf::Gltf::from_slice(bytes).map_err(|e| ImportError::Container(e.to_string()))?;
    let doc = &glb.document;
    if doc.nodes().len() > limits.nodes {
        return Err(ImportError::Limit("nodes".into()));
    }
    if doc.animations().len() > 4096
        || doc.skins().len() > 4096
        || doc
            .animations()
            .map(|a| a.channels().count())
            .sum::<usize>()
            > 65536
    {
        return Err(ImportError::Limit("animations, skins, or channels".into()));
    }
    if doc.materials().len() > 4096
        || doc.images().len() > 1024
        || doc.meshes().map(|m| m.primitives().len()).sum::<usize>() > 65536
    {
        return Err(ImportError::Limit(
            "materials, images, or primitives".into(),
        ));
    }
    for b in doc.buffers() {
        if !matches!(b.source(), gltf::buffer::Source::Bin) {
            return Err(ImportError::Unsupported(
                "external or data-URI buffer; use an embedded BIN".into(),
            ));
        }
        if b.index() != 0 || b.length() > bin.len() || bin.len() - b.length() > 3 {
            return Err(ImportError::Container("BIN length or buffer count".into()));
        }
    }
    for view in doc.views() {
        view_bytes(view, bin)?;
    }
    let total = doc.accessors().try_fold(0usize, |n, a| {
        n.checked_add(a.count().checked_mul(a.dimensions().multiplicity())?)
    });
    if total.is_none_or(|n| n > limits.elements) {
        return Err(ImportError::Limit("total accessor components".into()));
    }
    // Validate every accessor, including ones not used by the supported renderer.
    for a in doc.accessors() {
        accessor(a, bin, limits)?;
    }
    let mut asset = Asset {
        exact_mesh: true,
        ..Asset::default()
    };
    if let Some(ext) = raw.get("extensionsUsed") {
        asset.warnings.push(format!(
            "Optional extensions are approximated/ignored: {ext}"
        ));
    }
    for node in doc.nodes() {
        let rest = match node.transform() {
            gltf::scene::Transform::Matrix { matrix } => {
                Local::Matrix(Mat4::from_cols_array_2d(&matrix))
            }
            gltf::scene::Transform::Decomposed {
                translation,
                rotation,
                scale,
            } => {
                let q = Quat::from_array(rotation);
                if !q.is_finite() || q.length_squared() < 1e-10 || (q.length() - 1.).abs() > 0.01 {
                    return Err(ImportError::Hierarchy("invalid rest quaternion".into()));
                }
                Local::Trs(Trs {
                    translation: translation.into(),
                    rotation: q.normalize(),
                    scale: scale.into(),
                })
            }
        };
        if !rest.matrix().is_finite() {
            return Err(ImportError::Hierarchy("non-finite rest transform".into()));
        }
        if node.weights().is_some_and(|w| w.iter().any(|&v| v != 0.)) {
            asset.exact_mesh = false;
            asset
                .warnings
                .push("Nonzero default node morph weights: deformation unsupported".into());
        }
        asset.nodes.push(Node {
            name: node.name().unwrap_or("").into(),
            parent: None,
            children: node.children().map(|c| c.index()).collect(),
            rest,
            mesh: node.mesh().map(|m| m.index()),
            skin: node.skin().map(|s| s.index()),
        });
    }
    for i in 0..asset.nodes.len() {
        for child in asset.nodes[i].children.clone() {
            if child >= asset.nodes.len() || asset.nodes[child].parent.replace(i).is_some() {
                return Err(ImportError::Hierarchy(
                    "invalid child or multiple parents".into(),
                ));
            }
        }
    }
    let mut queue: VecDeque<_> = asset
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.parent.is_none())
        .map(|(i, _)| i)
        .collect();
    while let Some(i) = queue.pop_front() {
        asset.order.push(i);
        queue.extend(asset.nodes[i].children.iter().copied());
    }
    if asset.order.len() != asset.nodes.len() {
        return Err(ImportError::Hierarchy("cycle detected".into()));
    }
    if asset
        .globals(&asset.rest_pose())
        .iter()
        .any(|m| !m.is_finite())
    {
        return Err(ImportError::Hierarchy(
            "rest hierarchy overflows finite transform space".into(),
        ));
    }
    for scene in doc.scenes() {
        let roots: Vec<_> = scene.nodes().map(|n| n.index()).collect();
        let mut seen = HashSet::new();
        if roots
            .iter()
            .any(|&i| asset.nodes[i].parent.is_some() || !seen.insert(i))
        {
            return Err(ImportError::Hierarchy(
                "scene root has parent or is duplicated".into(),
            ));
        }
        asset.scenes.push(Scene {
            name: scene.name().unwrap_or("Scene").into(),
            roots,
        });
    }
    asset.default_scene = doc.default_scene().map_or(0, |s| s.index());
    if asset.scenes.is_empty() {
        asset.scenes.push(Scene {
            name: "Unspecified scene".into(),
            roots: Vec::new(),
        });
        asset
            .warnings
            .push("No scene defined; hierarchy is available for animation mapping".into());
    }
    let mut image_bytes = 0usize;
    for img in doc.images() {
        let gltf::image::Source::View { view, mime_type } = img.source() else {
            return Err(ImportError::Unsupported(
                "external or data-URI image".into(),
            ));
        };
        if mime_type != "image/png" && mime_type != "image/jpeg" {
            return Err(ImportError::Unsupported(format!(
                "image format {mime_type}"
            )));
        }
        let mut reader = image::ImageReader::new(Cursor::new(view_bytes(view, bin)?))
            .with_guessed_format()
            .map_err(ImportError::Io)?;
        let mut image_limits = image::Limits::default();
        image_limits.max_image_width = Some(limits.image_side);
        image_limits.max_image_height = Some(limits.image_side);
        image_limits.max_alloc = Some(limits.decoded_bytes.saturating_sub(image_bytes) as u64);
        reader.limits(image_limits);
        let decoded = reader
            .decode()
            .map_err(|e| ImportError::Limit(format!("image decode: {e}")))?
            .into_rgba8();
        image_bytes = image_bytes
            .checked_add(decoded.len())
            .filter(|&n| n <= limits.decoded_bytes)
            .ok_or_else(|| ImportError::Limit("decoded images".into()))?;
        asset.images.push(Image {
            width: decoded.width(),
            height: decoded.height(),
            rgba: decoded.into_raw(),
        });
    }
    for material in doc.materials() {
        let pbr = material.pbr_metallic_roughness();
        let legacy = material.pbr_specular_glossiness();
        let mut m = Material {
            color: legacy
                .as_ref()
                .map_or_else(|| pbr.base_color_factor(), |s| s.diffuse_factor()),
            alpha_mode: match material.alpha_mode() {
                gltf::material::AlphaMode::Opaque => 0,
                gltf::material::AlphaMode::Mask => 1,
                gltf::material::AlphaMode::Blend => 2,
            },
            cutoff: material.alpha_cutoff().unwrap_or(0.5),
            double_sided: material.double_sided(),
            ..Material::default()
        };
        if let Some(info) = legacy
            .as_ref()
            .map_or_else(|| pbr.base_color_texture(), |s| s.diffuse_texture())
        {
            if info.tex_coord() != 0 {
                asset
                    .warnings
                    .push("Base color TEXCOORD_1+ unsupported".into());
            } else {
                m.image = Some(info.texture().source().index());
            }
            let s = info.texture().sampler();
            m.sampler = Sampler {
                wrap_s: s.wrap_s().as_gl_enum(),
                wrap_t: s.wrap_t().as_gl_enum(),
                nearest: matches!(s.mag_filter(), Some(gltf::texture::MagFilter::Nearest)),
            };
        }
        if legacy.is_some() {
            asset.warnings.push("KHR_materials_pbrSpecularGlossiness: diffuse factor/texture preview; specular and glossiness lighting are approximated".into());
        }
        if material.normal_texture().is_some()
            || material.occlusion_texture().is_some()
            || material.emissive_texture().is_some()
            || pbr.metallic_roughness_texture().is_some()
            || pbr.metallic_factor() != 0.
            || material.emissive_factor() != [0.; 3]
        {
            asset.warnings.push("Material uses PBR/normal/occlusion/emissive features approximated by the basic diffuse preview".into());
        }
        asset.materials.push(m);
    }
    let fallback_material = asset.materials.len();
    asset.materials.push(Material::default());
    let mut geometry_bytes = image_bytes;
    for mesh in doc.meshes() {
        let mut output = Mesh {
            primitives: Vec::new(),
        };
        if mesh.weights().is_some_and(|w| w.iter().any(|&v| v != 0.)) {
            asset.exact_mesh = false;
            asset
                .warnings
                .push("Nonzero mesh morph defaults unsupported".into());
        }
        for p in mesh.primitives() {
            if p.morph_targets().len() != 0 {
                asset.warnings.push(
                    "Morph targets present: only undeformed base mesh preview is available".into(),
                );
            }
            if p.mode() != gltf::mesh::Mode::Triangles {
                asset.exact_mesh = false;
                asset.warnings.push(format!(
                    "Unsupported primitive mode {:?}: skipped",
                    p.mode()
                ));
                continue;
            }
            let get = |semantic| -> Result<Option<Vec<Vec<f32>>>> {
                p.get(&semantic)
                    .map(|a| accessor(a, bin, limits))
                    .transpose()
            };
            let position_accessor = p
                .get(&gltf::Semantic::Positions)
                .ok_or_else(|| ImportError::Container("primitive missing POSITION".into()))?;
            require(
                &position_accessor,
                Dimensions::Vec3,
                &[DataType::F32],
                Some(false),
            )?;
            let positions = accessor(position_accessor, bin, limits)?;
            let normals = get(gltf::Semantic::Normals)?;
            let uv = get(gltf::Semantic::TexCoords(0))?;
            let j0 = get(gltf::Semantic::Joints(0))?;
            let j1 = get(gltf::Semantic::Joints(1))?;
            let w0 = get(gltf::Semantic::Weights(0))?;
            let w1 = get(gltf::Semantic::Weights(1))?;
            for (semantic, a) in p.attributes() {
                if a.count() != positions.len() {
                    return Err(ImportError::Container(
                        "vertex attribute count mismatch".into(),
                    ));
                }
                match semantic {
                    gltf::Semantic::Joints(set) => {
                        require(
                            &a,
                            Dimensions::Vec4,
                            &[DataType::U8, DataType::U16],
                            Some(false),
                        )?;
                        if set > 1 {
                            return Err(ImportError::Unsupported(
                                "more than eight influences".into(),
                            ));
                        }
                    }
                    gltf::Semantic::Weights(set) => {
                        require(
                            &a,
                            Dimensions::Vec4,
                            &[DataType::F32, DataType::U8, DataType::U16],
                            None,
                        )?;
                        if a.data_type() != DataType::F32 && !a.normalized() {
                            return Err(ImportError::Container(
                                "integer weights must be normalized".into(),
                            ));
                        }
                        if set > 1 {
                            return Err(ImportError::Unsupported(
                                "more than eight influences".into(),
                            ));
                        }
                    }
                    gltf::Semantic::Normals => {
                        require(&a, Dimensions::Vec3, &[DataType::F32], Some(false))?
                    }
                    gltf::Semantic::TexCoords(0) => require(
                        &a,
                        Dimensions::Vec2,
                        &[DataType::F32, DataType::U8, DataType::U16],
                        None,
                    )?,
                    gltf::Semantic::Colors(_) | gltf::Semantic::Tangents => asset.warnings.push(
                        "Vertex colors/tangents are not represented by this material preview"
                            .into(),
                    ),
                    _ => (),
                }
            }
            if j0.is_some() != w0.is_some()
                || j1.is_some() != w1.is_some()
                || j1.is_some() && j0.is_none()
            {
                return Err(ImportError::Skin("unpaired joints/weights".into()));
            }
            let mut vertices = Vec::with_capacity(positions.len());
            for (i, position) in positions.iter().enumerate() {
                let mut v = Vertex {
                    position: position[..3].try_into().expect("validated position shape"),
                    normal: [0., 1., 0.],
                    uv: [0.; 2],
                    joints: [0; 8],
                    weights: [0.; 8],
                };
                if let Some(n) = &normals {
                    v.normal = n[i][..3].try_into().expect("shape");
                }
                if let Some(uv) = &uv {
                    v.uv = uv[i][..2].try_into().expect("shape");
                }
                for (offset, joints, weights) in [(0, &j0, &w0), (4, &j1, &w1)] {
                    if let (Some(j), Some(w)) = (joints, weights) {
                        for k in 0..4 {
                            v.joints[offset + k] = j[i][k] as u32;
                            v.weights[offset + k] = w[i][k];
                        }
                    }
                }
                if v.weights.iter().any(|&w| w < 0.) {
                    return Err(ImportError::Skin("negative weight".into()));
                }
                let sum: f32 = v.weights.iter().sum();
                if j0.is_some() && sum <= 1e-8 {
                    return Err(ImportError::Skin("zero-sum influences".into()));
                }
                if sum > 1e-8 {
                    for w in &mut v.weights {
                        *w /= sum;
                    }
                }
                vertices.push(v);
            }
            let indices: Vec<u32> = if let Some(a) = p.indices() {
                require(
                    &a,
                    Dimensions::Scalar,
                    &[DataType::U8, DataType::U16, DataType::U32],
                    Some(false),
                )?;
                accessor(a, bin, limits)?
                    .iter()
                    .map(|v| v[0] as u32)
                    .collect()
            } else {
                (0..vertices.len() as u32).collect()
            };
            if !indices.len().is_multiple_of(3)
                || indices.iter().any(|&i| i as usize >= vertices.len())
            {
                return Err(ImportError::Container(
                    "triangle indices out of range or incomplete".into(),
                ));
            }
            if normals.is_none() {
                let mut sums = vec![Vec3::ZERO; vertices.len()];
                for t in indices.as_chunks::<3>().0 {
                    let a = Vec3::from(vertices[t[0] as usize].position);
                    let b = Vec3::from(vertices[t[1] as usize].position);
                    let c = Vec3::from(vertices[t[2] as usize].position);
                    let normal = (b - a).cross(c - a);
                    for &i in t {
                        sums[i as usize] += normal;
                    }
                }
                for (v, n) in vertices.iter_mut().zip(sums) {
                    v.normal = n.try_normalize().unwrap_or(Vec3::Y).into();
                }
            }
            geometry_bytes = geometry_bytes
                .checked_add(vertices.len() * size_of::<Vertex>() + indices.len() * 4)
                .filter(|&n| n <= limits.decoded_bytes)
                .ok_or_else(|| {
                    ImportError::Limit(
                        "expanded geometry bytes (including reused accessors)".into(),
                    )
                })?;
            output.primitives.push(Primitive {
                vertices,
                indices,
                material: p.material().index().unwrap_or(fallback_material),
                has_skin: j0.is_some(),
            });
        }
        asset.meshes.push(output);
    }
    let instances: usize = asset
        .nodes
        .iter()
        .filter_map(|n| n.mesh.map(|m| asset.meshes[m].primitives.len()))
        .sum();
    if instances > 65536 {
        return Err(ImportError::Limit("draw instances".into()));
    }
    let draw_work = asset
        .nodes
        .iter()
        .filter_map(|n| n.mesh)
        .flat_map(|m| &asset.meshes[m].primitives)
        .try_fold(0usize, |n, p| n.checked_add(p.indices.len()));
    if draw_work.is_none_or(|n| n > limits.elements) {
        return Err(ImportError::Limit("instanced index/vertex workload".into()));
    }
    for skin in doc.skins() {
        if skin.joints().count() > 4096 {
            return Err(ImportError::Limit("skin joints".into()));
        }
        let joints: Vec<_> = skin.joints().map(|j| j.index()).collect();
        let unique: HashSet<_> = joints.iter().collect();
        geometry_bytes = geometry_bytes
            .checked_add(joints.len() * (size_of::<Mat4>() + size_of::<usize>()))
            .filter(|&n| n <= limits.decoded_bytes)
            .ok_or_else(|| ImportError::Limit("expanded skin data".into()))?;
        if joints.is_empty() || unique.len() != joints.len() || joints.len() > 4096 {
            return Err(ImportError::Skin(
                "empty, duplicated, or excessive joints".into(),
            ));
        }
        let inverse_bind = if let Some(a) = skin.inverse_bind_matrices() {
            require(&a, Dimensions::Mat4, &[DataType::F32], Some(false))?;
            if a.count() < joints.len() {
                return Err(ImportError::Skin("too few inverse bind matrices".into()));
            }
            accessor(a, bin, limits)?
                .iter()
                .take(joints.len())
                .map(|v| Mat4::from_cols_slice(v))
                .collect()
        } else {
            vec![Mat4::IDENTITY; joints.len()]
        };
        asset.skins.push(Skin {
            joints,
            inverse_bind,
        });
    }
    for node in &asset.nodes {
        if let Some(skin) = node.skin {
            let mesh = node
                .mesh
                .ok_or_else(|| ImportError::Skin("skin attachment missing mesh".into()))?;
            for p in &asset.meshes[mesh].primitives {
                if !p.has_skin
                    || p.vertices.iter().any(|v| {
                        v.joints
                            .iter()
                            .any(|&j| j as usize >= asset.skins[skin].joints.len())
                    })
                {
                    return Err(ImportError::Skin(
                        "missing influences or palette index out of range".into(),
                    ));
                }
            }
        }
    }
    for animation in doc.animations() {
        let mut clip = Clip {
            name: animation.name().unwrap_or("Animation").into(),
            tracks: Vec::new(),
            duration: 0.,
            supported: true,
        };
        let mut targets = HashSet::new();
        for channel in animation.channels() {
            let node = channel.target().node().index();
            let property = match channel.target().property() {
                gltf::animation::Property::Translation => Property::Translation,
                gltf::animation::Property::Rotation => Property::Rotation,
                gltf::animation::Property::Scale => Property::Scale,
                gltf::animation::Property::MorphTargetWeights => Property::Weights,
            };
            if !targets.insert((node, property)) {
                return Err(ImportError::Animation("duplicate channel target".into()));
            }
            if property != Property::Weights && matches!(asset.nodes[node].rest, Local::Matrix(_)) {
                return Err(ImportError::Animation(
                    "TRS animation targets a matrix node".into(),
                ));
            }
            let sampler = channel.sampler();
            let input = sampler.input();
            let output = sampler.output();
            geometry_bytes = geometry_bytes
                .checked_add(input.count() * 4 + output.count() * 16)
                .filter(|&n| n <= limits.decoded_bytes)
                .ok_or_else(|| {
                    ImportError::Limit("expanded animation data (including reused samplers)".into())
                })?;
            require(&input, Dimensions::Scalar, &[DataType::F32], Some(false))?;
            let times: Vec<_> = accessor(input, bin, limits)?.iter().map(|v| v[0]).collect();
            if times.is_empty() || times[0] < 0. || times.windows(2).any(|w| w[0] >= w[1]) {
                return Err(ImportError::Animation(
                    "key times must be nonempty, nonnegative and strictly increasing".into(),
                ));
            }
            clip.duration = clip.duration.max(*times.last().expect("nonempty"));
            if property == Property::Weights {
                clip.supported = false;
                asset.warnings.push(
                    "Morph-weight animation unsupported; exact playback disabled for this clip"
                        .into(),
                );
                continue;
            }
            require(
                &output,
                if property == Property::Rotation {
                    Dimensions::Vec4
                } else {
                    Dimensions::Vec3
                },
                &[DataType::F32],
                Some(false),
            )?;
            let interpolation = match sampler.interpolation() {
                gltf::animation::Interpolation::Step => Interpolation::Step,
                gltf::animation::Interpolation::Linear => Interpolation::Linear,
                gltf::animation::Interpolation::CubicSpline => Interpolation::Cubic,
            };
            let mut values: Vec<_> = accessor(output, bin, limits)?
                .iter()
                .map(|v| Vec4::new(v[0], v[1], v[2], *v.get(3).unwrap_or(&0.)))
                .collect();
            let multiplier = if interpolation == Interpolation::Cubic {
                3
            } else {
                1
            };
            if values.len() != times.len() * multiplier {
                return Err(ImportError::Animation("sampler output count".into()));
            }
            if property == Property::Rotation {
                for (i, v) in values.iter_mut().enumerate() {
                    if multiplier == 1 || i % 3 == 1 {
                        if v.length_squared() < 1e-10 || (v.length() - 1.).abs() > 0.01 {
                            return Err(ImportError::Animation("invalid key quaternion".into()));
                        }
                        *v = v.normalize();
                    }
                }
            }
            clip.tracks.push(Track {
                node,
                property,
                interpolation,
                times,
                values,
            });
        }
        asset.clips.push(clip);
    }
    asset.warnings.sort();
    asset.warnings.dedup();
    asset.default_bounds = asset.compute_bounds(asset.default_scene);
    if asset
        .default_bounds
        .is_some_and(|(min, max)| min.abs().max_element() > 1e12 || max.abs().max_element() > 1e12)
    {
        return Err(ImportError::Limit(
            "world bounds exceed the safe preview range (1e12 units)".into(),
        ));
    }
    Ok(asset)
}

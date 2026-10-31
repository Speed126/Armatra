//! File imports normalize once to immutable GLB; the checked asset decoder and
//! content-addressed manifest remain the single playback/storage representation.
use super::{Limits, from_bytes, read_file};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::path::{Component, Path};
mod bvh;

pub fn from_path(path: &Path, limits: &Limits) -> Result<(Vec<u8>, crate::scene::Asset)> {
    let bytes = match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "glb" => read_file(path, limits)?,
        "gltf" => gltf(path, limits)?,
        "bvh" => bvh::normalize(
            &read_file(path, limits)?,
            limits,
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("BVH motion"),
        )?,
        "fbx" => bail!(
            "FBX skeletal animation import is not supported. See README.md for supported inputs and limitations."
        ),
        _ => bail!("Supported imports: GLB, local glTF, and BVH skeletal motion"),
    };
    ensure!(
        bytes.len() <= limits.file_bytes,
        "Normalized GLB exceeds file budget"
    );
    let asset = from_bytes(&bytes, limits)?;
    Ok((bytes, asset))
}

fn resource(root: &Path, uri: &str, limits: &Limits) -> Result<Vec<u8>> {
    ensure!(
        !uri.starts_with("data:"),
        "Data URIs are not supported by local glTF import; use local buffer/image files"
    );
    let decoded = percent_encoding::percent_decode_str(uri)
        .decode_utf8()
        .context("Resource URI must be UTF-8")?;
    ensure!(
        !decoded.contains([':', '\\', '?', '#', '\0']),
        "Only local relative resource URIs are supported"
    );
    let relative = Path::new(decoded.as_ref());
    ensure!(
        !relative.is_absolute()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir)),
        "External resource must remain inside the selected asset directory"
    );
    let resolved = root
        .join(relative)
        .canonicalize()
        .with_context(|| format!("Missing glTF resource: {uri}"))?;
    ensure!(
        resolved.starts_with(root),
        "External resource resolves outside the asset directory"
    );
    read_file(&resolved, limits).with_context(|| format!("Reading glTF resource {uri}"))
}
fn append(bin: &mut Vec<u8>, data: &[u8], limits: &Limits) -> Result<usize> {
    let offset = bin.len().checked_add(3).context("Buffer offset overflow")? & !3;
    ensure!(
        offset
            .checked_add(data.len())
            .is_some_and(|n| n <= limits.file_bytes),
        "Combined resource budget exceeded"
    );
    bin.resize(offset, 0);
    bin.extend_from_slice(data);
    Ok(offset)
}
fn gltf(path: &Path, limits: &Limits) -> Result<Vec<u8>> {
    let bytes = read_file(path, limits)?;
    ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "glTF JSON exceeds 16 MiB budget"
    );
    let mut doc: Value = serde_json::from_slice(&bytes).context("Invalid glTF JSON")?;
    ensure!(
        doc["nodes"]
            .as_array()
            .is_none_or(|a| a.len() <= limits.nodes),
        "Node budget exceeded"
    );
    // Validate the original declarations before resolving and rebasing them.
    gltf::Gltf::from_slice(&bytes).context("Invalid glTF declarations")?;
    let root = path
        .canonicalize()?
        .parent()
        .context("Asset directory")?
        .to_path_buf();
    let mut bin = Vec::new();
    let mut buffers = Vec::new();
    if let Some(array) = doc["buffers"].as_array() {
        for buffer in array {
            let uri = buffer["uri"]
                .as_str()
                .context("glTF buffer needs a local URI")?;
            let data = resource(&root, uri, limits)?;
            let declared = buffer["byteLength"]
                .as_u64()
                .context("Missing buffer byteLength")?;
            ensure!(
                declared <= data.len() as u64,
                "External buffer is shorter than declared: {uri}"
            );
            let declared = usize::try_from(declared)?;
            let offset = append(&mut bin, &data[..declared], limits)?;
            buffers.push((offset, declared));
        }
    }
    if let Some(views) = doc["bufferViews"].as_array_mut() {
        for view in views {
            let index = usize::try_from(view["buffer"].as_u64().context("Missing view buffer")?)?;
            let (base, len) = *buffers
                .get(index)
                .context("View references missing buffer")?;
            let start = usize::try_from(view["byteOffset"].as_u64().unwrap_or(0))?;
            let length =
                usize::try_from(view["byteLength"].as_u64().context("Missing view length")?)?;
            ensure!(
                start.checked_add(length).is_some_and(|end| end <= len),
                "View exceeds its original buffer"
            );
            view["buffer"] = json!(0);
            view["byteOffset"] = json!(base + start);
        }
    } else {
        doc["bufferViews"] = json!([]);
    }
    let images = doc["images"].as_array().cloned().unwrap_or_default();
    for (index, image) in images.iter().enumerate() {
        if let Some(uri) = image["uri"].as_str() {
            let data = resource(&root, uri, limits)?;
            let mime = match image::guess_format(&data)? {
                image::ImageFormat::Png => "image/png",
                image::ImageFormat::Jpeg => "image/jpeg",
                _ => bail!("glTF textures must be PNG or JPEG"),
            };
            let offset = append(&mut bin, &data, limits)?;
            let views = doc["bufferViews"].as_array_mut().unwrap();
            let view = views.len();
            views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":data.len()}));
            let object = doc["images"][index]
                .as_object_mut()
                .context("Invalid image")?;
            object.remove("uri");
            object.insert("bufferView".into(), json!(view));
            object.insert("mimeType".into(), json!(mime));
        }
    }
    doc["buffers"] = json!([{"byteLength":bin.len()}]);
    encode(&doc, &bin, limits)
}
pub(super) fn encode(doc: &Value, data: &[u8], limits: &Limits) -> Result<Vec<u8>> {
    let mut json = serde_json::to_vec(doc)?;
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bin = data.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let size = 28usize
        .checked_add(json.len())
        .and_then(|n| n.checked_add(bin.len()))
        .context("GLB size overflow")?;
    ensure!(
        size <= limits.file_bytes && size <= u32::MAX as usize,
        "Normalized GLB exceeds file budget"
    );
    let mut out = Vec::with_capacity(size);
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend((size as u32).to_le_bytes());
    out.extend((json.len() as u32).to_le_bytes());
    out.extend(0x4e4f534au32.to_le_bytes());
    out.extend(json);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(0x004e4942u32.to_le_bytes());
    out.extend(bin);
    Ok(out)
}

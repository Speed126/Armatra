//! Local, user-provided interoperability probes, intentionally separate from
//! the distributed annotated corpus and its accuracy claims.
use crate::{
    animation,
    import::{self, Limits},
    render::{Gpu, Renderer},
    rig::Rig,
};
use anyhow::{Result, ensure};
use serde_json::json;
pub fn run() -> Result<()> {
    let paths: Vec<_> = std::env::args_os().skip(2).collect();
    ensure!(
        !paths.is_empty(),
        "Usage: armatra --character-smoke model.glb [another.glb ...]"
    );
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let (gpu, _) = pollster::block_on(Gpu::new(&instance, None))?;
    let renderer = Renderer::new(&gpu);
    let mut reports = Vec::new();
    std::fs::create_dir_all("target/reports/characters")?;
    for (index, path) in paths.iter().enumerate() {
        let name = format!("character-{index}");
        let bytes = import::read_file(std::path::Path::new(path), &Limits::default())?;
        let a = import::from_bytes(&bytes, &Limits::default())?;
        let identity: Vec<_> = (0..a.nodes.len()).map(Some).collect();
        let mut samples = 0;
        let mut palettes = 0;
        for clip in &a.clips {
            for step in 0..=32 {
                let time = clip.duration * step as f32 / 32.;
                let pose = animation::evaluate(&a, clip, &identity, time, &mut Vec::new());
                let g = a.globals(&pose);
                ensure!(
                    g.iter().all(|m| m.is_finite()),
                    "Non-finite {name} at {time}"
                );
                for (i, n) in a
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.skin.is_some() && n.mesh.is_some())
                {
                    let _ = n;
                    let palette = a.palette(i, &g).map_err(anyhow::Error::msg)?;
                    ensure!(palette.iter().all(|m| m.is_finite()), "palette {name}");
                    palettes += 1;
                }
                samples += 1;
            }
        }
        let image = crate::diagnostics::render_sample(
            &gpu,
            &renderer,
            &a,
            [900, 900],
            a.clips.first().map_or(0., |c| c.duration * 0.3),
        )?;
        image.save(format!("target/reports/characters/{name}.png"))?;
        let rigid = Rig::analyze_skin(&a, Some(0));
        reports.push(json!({"name":name,"hash":blake3::hash(&bytes).to_hex().to_string(),"file_bytes":bytes.len(),"nodes":a.nodes.len(),"meshes":a.meshes.len(),"skins":a.skins.len(),"clips":a.clips.len(),"vertices":a.meshes.iter().flat_map(|m|&m.primitives).map(|p|p.vertices.len()).sum::<usize>(),"sampled_poses":samples,"finite_palettes":palettes,"cpu_bytes":a.cpu_bytes(),"rigid_first_skin_joints":rigid.joints.iter().filter(|d|d.rigid).count(),"first_skin_joints":rigid.joints.len(),"orientation_known":rigid.frame.is_some(),"warnings":a.warnings,"rig_warnings":rigid.warnings}));
    }
    let report = json!({"adapter":gpu.info.name,"backend":format!("{:?}",gpu.info.backend),"driver":gpu.info.driver,"driver_info":gpu.info.driver_info,"assets":reports,"scope":"Local import, source animation/FK/palettes and GPU rendering. These assets are not annotated mapping ground truth or an automatic cross-rig accuracy claim."});
    std::fs::write(
        "target/reports/characters/report.json",
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

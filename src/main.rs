fn main() -> anyhow::Result<()> {
    armatra::app_log::init();
    let result = run();
    armatra::app_log::event(
        "process_exit",
        serde_json::json!({"success":result.is_ok(),"error":result.as_ref().err().map(|e|format!("{e:#}"))}),
    );
    result
}
fn run() -> anyhow::Result<()> {
    if std::env::args().any(|a| a == "--skinning-benchmark") {
        return armatra::skinning_bench::run();
    }
    if std::env::args().any(|a| a == "--cache-benchmark") {
        return armatra::app::cache_benchmark();
    }
    if std::env::args().any(|a| a == "--motion-benchmark") {
        return armatra::animation_bench::motion_benchmark();
    }
    if std::env::args().any(|a| a == "--acl-validation") {
        return armatra::animation_bench::acl_validation();
    }
    if std::env::args().any(|a| a == "--deformation-benchmark") {
        return armatra::animation_bench::deformation_benchmark();
    }
    if std::env::args().any(|a| a == "--compression-benchmark") {
        return armatra::animation_bench::compression_benchmark();
    }
    if std::env::args().any(|a| a == "--character-smoke") {
        return armatra::character_probe::run();
    }
    if std::env::args().any(|a| a == "--retarget-comparison") {
        return armatra::retarget_bench::comparison();
    }
    if std::env::args().any(|a| a == "--retarget-preview") {
        return armatra::retarget_bench::preview();
    }
    if std::env::args().any(|a| a == "--retarget-benchmark") {
        return armatra::retarget_bench::run();
    }
    if std::env::args().any(|a| a == "--write-retarget-samples") {
        return armatra::rig_corpus::write_samples();
    }
    if std::env::args().any(|a| a == "--ui-smoke") {
        return armatra::app::ui_smoke();
    }
    if std::env::args().any(|a| a == "--write-samples") {
        return armatra::fixtures::write_samples();
    }
    if std::env::args().any(|a| a == "--gpu-smoke") {
        return armatra::diagnostics::gpu_smoke();
    }
    if std::env::args().any(|a| a == "--benchmark") {
        return armatra::diagnostics::benchmark();
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--analyze-retarget") {
        anyhow::ensure!(
            args.len() >= 4 && (args.len() - 4).is_multiple_of(2),
            "Usage: --analyze-retarget source.glb target.glb report.json [--source-skin N] [--target-skin N]"
        );
        let mut settings = armatra::retarget::Settings::default();
        for pair in args[4..].as_chunks::<2>().0 {
            let skin: usize = pair[1]
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid skin index"))?
                .parse()?;
            match pair[0].to_str() {
                Some("--source-skin") => settings.source_skin = Some(skin),
                Some("--target-skin") => settings.target_skin = Some(skin),
                _ => anyhow::bail!("Unknown retarget analysis option"),
            }
        }
        return armatra::diagnostics::analyze_retarget(
            std::path::Path::new(&args[1]),
            std::path::Path::new(&args[2]),
            std::path::Path::new(&args[3]),
            &settings,
        );
    }
    if args.first().is_some_and(|a| a == "--cache-replay") {
        let path = std::path::Path::new(
            args.get(1)
                .ok_or_else(|| anyhow::anyhow!("Usage: armatra --cache-replay trace.json"))?,
        );
        anyhow::ensure!(
            std::fs::metadata(path)?.len() <= 16 * 1024 * 1024,
            "Trace byte budget exceeded"
        );
        let trace = serde_json::from_slice(&std::fs::read(path)?)?;
        return armatra::cache_policies::benchmark(&trace);
    }
    if args.first().is_some_and(|a| a == "--seed-retarget-library") {
        anyhow::ensure!(
            args.len() == 4,
            "Usage: armatra --seed-retarget-library library source.glb target.glb"
        );
        return armatra::retarget_bench::seed_library(
            std::path::Path::new(&args[1]),
            std::path::Path::new(&args[2]),
            std::path::Path::new(&args[3]),
        );
    }
    if args.first().is_some_and(|a| a == "--inspect") {
        return armatra::diagnostics::inspect(std::path::Path::new(
            args.get(1)
                .ok_or_else(|| anyhow::anyhow!("Usage: armatra --inspect file.glb"))?,
        ));
    }
    anyhow::ensure!(
        !args.iter().any(|a| a.to_string_lossy().starts_with("--")),
        "Unknown option. Diagnostic commands: --compression-benchmark, --deformation-benchmark, --acl-validation, --motion-benchmark, --cache-benchmark, --skinning-benchmark. See README.md for usage and diagnostics."
    );
    armatra::app::run(
        std::env::args_os()
            .skip(1)
            .map(std::path::PathBuf::from)
            .collect(),
    )
}

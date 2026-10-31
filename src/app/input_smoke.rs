//! Synthetic egui input integration checks. No operating-system input injection.
use super::*;
use anyhow::ensure;
mod apply_smoke;
mod grid_smoke;
mod mapping_smoke;
mod preview_smoke;

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_theme(egui::Theme::Dark);
    ctx
}
fn frame(
    e: &mut Editor,
    renderer: &Renderer,
    gpu: &Gpu,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> [u32; 2] {
    let ctx = e.ctx.clone();
    let mut dimensions = [0, 0];
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |root| {
            dimensions = e.ui(root, egui::TextureId::User(999), renderer, gpu);
        },
    );
    // This harness checks UI responses, not rasterization (native capture covers that).
    output.textures_delta.clear();
    dimensions
}
fn input(e: &mut Editor, r: &Renderer, g: &Gpu, events: Vec<egui::Event>) {
    frame(e, r, g, egui::vec2(1440., 900.), events);
}
fn click(e: &mut Editor, r: &Renderer, g: &Gpu, position: egui::Pos2) {
    input(
        e,
        r,
        g,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    input(
        e,
        r,
        g,
        vec![egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
}
fn drag(
    e: &mut Editor,
    r: &Renderer,
    g: &Gpu,
    from: egui::Pos2,
    delta: egui::Vec2,
    button: egui::PointerButton,
) {
    input(
        e,
        r,
        g,
        vec![
            egui::Event::PointerMoved(from),
            egui::Event::PointerButton {
                pos: from,
                button,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    input(e, r, g, vec![egui::Event::PointerMoved(from + delta)]);
    input(
        e,
        r,
        g,
        vec![egui::Event::PointerButton {
            pos: from + delta,
            button,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
}
fn settle(e: &mut Editor, r: &Renderer, g: &Gpu) -> Result<()> {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    let mut stable = 0;
    while stable < 3 {
        e.drain(r, g);
        input(e, r, g, Vec::new());
        if e.jobs == 0 && !e.motion.previews.pending() {
            stable += 1;
        } else {
            stable = 0;
        }
        ensure!(Instant::now() < deadline, "UI load timed out: {}", e.status);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    Ok(())
}
fn select(e: &mut Editor, r: &Renderer, g: &Gpu, id: &str) -> Result<()> {
    if e.ui_state.workspace == Workspace::Retarget {
        let animate = e
            .input_regions
            .as_ref()
            .context("input regions")?
            .animate_open
            .center();
        click(e, r, g, animate);
    }
    // Search is the stable way to reach an asset in a scrollable collection.
    e.search = e
        .records
        .iter()
        .find(|record| record.id == id)
        .context("record")?
        .name
        .clone();
    e.ui_state.collection = Collection::All;
    input(e, r, g, Vec::new());
    let rect = *e
        .input_regions
        .as_ref()
        .context("input regions")?
        .records
        .get(id)
        .context("library entry region")?;
    click(e, r, g, rect.center());
    settle(e, r, g)
}
pub fn run() -> Result<()> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let (gpu, _) = pollster::block_on(Gpu::new(&instance, None))?;
    let renderer = Renderer::new(&gpu);
    let run_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let root = std::env::var_os("ARMATRA_REPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/reports"))
        .join(format!("ui-smoke-{}-{run_id}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    if std::env::var("ARMATRA_SMOKE_SCOPE").as_deref() == Ok("mapping") {
        return mapping_smoke::run(&renderer, &gpu, &root);
    }
    if std::env::var("ARMATRA_SMOKE_SCOPE").as_deref() == Ok("apply") {
        return apply_smoke::run(&renderer, &gpu, &root);
    }
    if std::env::var("ARMATRA_SMOKE_SCOPE").as_deref() == Ok("preview") {
        return preview_smoke::run(&renderer, &gpu, &root);
    }
    if std::env::var("ARMATRA_SMOKE_SCOPE").as_deref() == Ok("repair") {
        return repair_workflow(&renderer, &gpu, &root);
    }
    if std::env::var("ARMATRA_SMOKE_SCOPE").as_deref() == Ok("formats") {
        return format_import_workflow(&renderer, &gpu, &root);
    }
    if std::env::var("ARMATRA_SMOKE_SCOPE").as_deref() == Ok("library") {
        return motion_search(&renderer, &gpu, &root);
    }
    if std::env::var("ARMATRA_SMOKE_SCOPE").as_deref() == Ok("retarget") {
        return retarget_mapping(&renderer, &gpu, &root);
    }
    grid_smoke::run(&renderer, &gpu)?;
    repair_workflow(&renderer, &gpu, &root)?;
    mapping_smoke::run(&renderer, &gpu, &root)?;
    apply_smoke::run(&renderer, &gpu, &root)?;
    preview_smoke::run(&renderer, &gpu, &root)?;
    format_import_workflow(&renderer, &gpu, &root)?;
    let mut empty = Editor::new(Library::open_seeded(root.join("empty"))?, context());
    empty.input_regions = Some(InputRegions::default());
    settle(&mut empty, &renderer, &gpu)?;
    let regions = empty.input_regions.as_ref().unwrap();
    ensure!(
        regions.empty_sample.is_positive()
            && regions.empty_import.is_positive()
            && regions.seek.is_positive(),
        "Fresh stage has working import and Library affordances"
    );
    let sample = regions.empty_sample.center();
    click(&mut empty, &renderer, &gpu, sample);
    settle(&mut empty, &renderer, &gpu)?;
    ensure!(
        empty.model.is_none() && empty.motion.entries.len() == 9 && empty.records.len() == 9,
        "Fresh-stage Browse motions opens the seeded Library without a default model"
    );
    write_evidence("empty-interactions.json",b"{\"result\":\"PASS\",\"sample_click_loads_motion_without_model\":true,\"native_import_dialog\":\"unverified\"}")?;
    drop(empty);
    let mut library = Library::open(root.join("managed"))?;
    let mut second = crate::fixtures::robot(false, false);
    second.json["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"] =
        serde_json::json!([1., 0.7, 0.8, 1.]);
    let mut originals = Vec::new();
    for (name, bytes) in [
        ("Robot A", crate::fixtures::robot(false, false).finish()),
        ("Robot B", second.finish()),
        (
            "Independent Wave",
            crate::fixtures::robot(true, true).finish(),
        ),
    ] {
        let path = root.join(format!("{name}.glb"));
        std::fs::write(&path, &bytes)?;
        let asset = import::from_bytes(&bytes, &Limits::default())?;
        library.import(&bytes, name, &asset)?;
        originals.push(path);
    }
    let records = library.manifest.records.clone();
    let models: Vec<_> = records
        .iter()
        .filter(|r| r.kind == Kind::Model)
        .cloned()
        .collect();
    let clips: Vec<_> = records
        .iter()
        .filter(|r| matches!(r.kind, Kind::Clip(_)))
        .cloned()
        .collect();
    let mut e = Editor::new(library, context());
    e.input_regions = Some(InputRegions::default());
    settle(&mut e, &renderer, &gpu)?;
    select(&mut e, &renderer, &gpu, &models[0].id)?;
    select(&mut e, &renderer, &gpu, &clips[2].id)?;
    ensure!(
        matches!(
            e.mapping.as_ref().map(|m| &m.status),
            Some(compatibility::Compatibility::Exact)
        ),
        "independent clip mapping"
    );
    let play = e.input_regions.as_ref().unwrap().play.center();
    click(&mut e, &renderer, &gpu, play);
    ensure!(!e.playback.playing, "pause button");
    click(&mut e, &renderer, &gpu, play);
    ensure!(e.playback.playing, "play button");
    let seek = e.input_regions.as_ref().unwrap().seek;
    click(
        &mut e,
        &renderer,
        &gpu,
        egui::pos2(seek.left() + seek.width() * 0.8, seek.center().y),
    );
    let high = e.playback.time;
    click(
        &mut e,
        &renderer,
        &gpu,
        egui::pos2(seek.left() + seek.width() * 0.2, seek.center().y),
    );
    ensure!(
        e.playback.time < high && high > 2. && e.playback.time < 1.,
        "backward scrub"
    );
    let old_speed = e.playback.speed;
    let speed = e.input_regions.as_ref().unwrap().speed.center();
    drag(
        &mut e,
        &renderer,
        &gpu,
        speed,
        egui::vec2(45., 0.),
        egui::PointerButton::Primary,
    );
    ensure!(e.playback.speed != old_speed, "speed drag");
    let viewport = e.input_regions.as_ref().unwrap().viewport.center();
    let yaw = e.camera.yaw;
    drag(
        &mut e,
        &renderer,
        &gpu,
        viewport,
        egui::vec2(65., 25.),
        egui::PointerButton::Primary,
    );
    ensure!((e.camera.yaw - yaw).abs() > 0.1, "orbit input");
    let target = e.camera.target;
    drag(
        &mut e,
        &renderer,
        &gpu,
        viewport,
        egui::vec2(30., 20.),
        egui::PointerButton::Secondary,
    );
    ensure!(e.camera.target.distance(target) > 0.001, "pan input");
    let distance = e.camera.distance;
    input(
        &mut e,
        &renderer,
        &gpu,
        vec![
            egui::Event::PointerMoved(viewport),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0., 120.),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Default::default(),
            },
        ],
    );
    for _ in 0..5 {
        input(&mut e, &renderer, &gpu, Vec::new());
    }
    ensure!(e.camera.distance < distance, "zoom input");
    let fit = e.input_regions.as_ref().unwrap().fit.center();
    click(&mut e, &renderer, &gpu, fit);
    ensure!(
        e.camera.distance > e.camera.radius && e.camera.distance.is_finite(),
        "fit button"
    );
    let large = frame(&mut e, &renderer, &gpu, egui::vec2(1440., 900.), Vec::new());
    let small = frame(&mut e, &renderer, &gpu, egui::vec2(1100., 720.), Vec::new());
    ensure!(
        small[0] < large[0] && small[1] < large[1],
        "viewport resize dimensions"
    );
    interaction_polish(&mut e, &renderer, &gpu)?;
    println!(
        "Keyboard focus, viewport-control isolation, 1280Ã—720 / 1440Ã—900 at 100%, 125%, 150%: PASS"
    );
    settle(&mut e, &renderer, &gpu)?;
    for record in models.iter().chain(&clips) {
        select(&mut e, &renderer, &gpu, &record.id)?;
    }
    let stable = renderer.resource_stats();
    let cpu_bytes = e.cpu.bytes();
    let cpu_count = e.cpu.len();
    let mut times = Vec::new();
    for i in 0..1000 {
        let start = Instant::now();
        select(&mut e, &renderer, &gpu, &models[i % 2].id)?;
        select(&mut e, &renderer, &gpu, &clips[i % 3].id)?;
        times.push(start.elapsed().as_secs_f64() * 1e6);
        ensure!(
            renderer.resource_stats() == stable
                && e.cpu.bytes() == cpu_bytes
                && e.cpu.len() == cpu_count,
            "resident UI switch resource growth"
        );
        ensure!(
            e.jobs == 0 && e.pending.is_empty(),
            "fully resident UI switch scheduled disk work"
        );
    }
    times.sort_by(f64::total_cmp);
    drop(e);
    for original in originals {
        std::fs::remove_file(original)?;
    }
    let deadline = Instant::now() + std::time::Duration::from_secs(3);
    let reopened = loop {
        match Library::open(root.join("managed")) {
            Ok(l) => break l,
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(error);
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    };
    let mut e = Editor::new(reopened, context());
    e.input_regions = Some(InputRegions::default());
    settle(&mut e, &renderer, &gpu)?;
    select(&mut e, &renderer, &gpu, &models[0].id)?;
    select(&mut e, &renderer, &gpu, &clips[2].id)?;
    ensure!(
        e.model.is_some()
            && e.source.is_some()
            && matches!(
                e.mapping.as_ref().map(|m| &m.status),
                Some(compatibility::Compatibility::Exact)
            ),
        "restart without original source files"
    );
    std::fs::create_dir_all("target/reports")?;
    let report = format!(
        "Automated egui input integration smoke\nAdapter: {} / {:?}\nPlay/pause buttons: PASS\nBackward timeline scrub: PASS\nPlayback speed drag: PASS\nOrbit/pan/wheel zoom: PASS\nFrame Character button: PASS\nViewport resize layout: PASS\nSpace shortcut / text input focus: PASS\nViewport controls do not orbit the camera: PASS\n1440x900 and 1280x720 at 100%, 125%, 150% interface scale: PASS\nExact cross-file library selection: PASS\n1,000 resident model+clip selections through UI: PASS\nCPU assets: {cpu_count}, accounted bytes: {cpu_bytes}\nGPU assets: {}, accounted bytes: {}, total uploads unchanged: {}\nSelection cycle p50: {:.3} us; p95: {:.3} us\nEach cycle includes search-filter updates, two clicks and UI settle frames; this is a synthetic egui event benchmark, not OS input-to-photon latency or process RSS.\nFresh Editor/library after closing and deleting all generated source files: PASS\nNative window/surface rendering is checked separately by scripts/smoke-mapping.ps1, smoke-apply.ps1 and smoke-library.ps1. OS-level manual input is not claimed by this test.\n",
        gpu.info.name, gpu.info.backend, stable.0, stable.1, stable.2, times[500], times[950]
    );
    retarget_mapping(&renderer, &gpu, &root)?;
    motion_search(&renderer, &gpu, &root)?;
    write_evidence("ui-smoke.txt", &report)?;
    println!("{report}");
    Ok(())
}

fn interaction_polish(e: &mut Editor, renderer: &Renderer, gpu: &Gpu) -> Result<()> {
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    };
    settle(e, renderer, gpu)?;
    let was_playing = e.playback.playing;
    input(e, renderer, gpu, vec![key(egui::Key::Space)]);
    ensure!(e.playback.playing != was_playing, "Space toggles playback");
    input(
        e,
        renderer,
        gpu,
        vec![egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    let search = e.input_regions.as_ref().unwrap().assets_search.center();
    click(e, renderer, gpu, search);
    let was_playing = e.playback.playing;
    let previous_search_length = e.search.len();
    input(
        e,
        renderer,
        gpu,
        vec![key(egui::Key::Space), egui::Event::Text(" ".into())],
    );
    ensure!(
        e.playback.playing == was_playing && e.search.len() == previous_search_length + 1,
        "typing Space does not play"
    );
    let yaw = e.camera.yaw;
    let fit = e.input_regions.as_ref().unwrap().fit.center();
    click(e, renderer, gpu, fit);
    ensure!(e.camera.yaw == yaw, "viewport chrome does not orbit camera");
    e.search.clear();
    for scale in [1., 1.25, 1.5] {
        e.ctx.set_zoom_factor(scale);
        for size in [egui::vec2(1440., 900.), egui::vec2(1280., 720.)] {
            for _ in 0..3 {
                frame(e, renderer, gpu, size, Vec::new());
            }
            let r = e.input_regions.as_ref().unwrap();
            ensure!(
                r.viewport.width() > 180. && r.viewport.height() > 120. && r.seek.width() > 400.,
                "usable layout at {size:?}, scale {scale}"
            );
            ensure!(
                !r.viewport.intersects(r.play) && !r.viewport.intersects(r.fit),
                "separate camera and control regions"
            );
        }
    }
    e.ctx.set_zoom_factor(1.);
    settle(e, renderer, gpu)
}

fn motion_search(renderer: &Renderer, gpu: &Gpu, root: &std::path::Path) -> Result<()> {
    let managed = root.join("motion-managed");
    let mut library = Library::open(managed.clone())?;
    for (name, builder) in crate::motion_fixtures::motions()
        .into_iter()
        .filter(|(n, _)| ["Walk", "Fast-walk", "Slow-walk", "Wave", "Jump"].contains(&n.as_str()))
    {
        let bytes = builder.finish();
        let asset = import::from_bytes(&bytes, &Limits::default())?;
        library.import(&bytes, &name, &asset)?;
    }
    let mut e = Editor::new(library, context());
    e.input_regions = Some(InputRegions::default());
    settle(&mut e, renderer, gpu)?;
    let position = e.input_regions.as_ref().unwrap().motion_open.center();
    click(&mut e, renderer, gpu, position);
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.motion.open && e.motion.entries.len() == 5,
        "Background motion index"
    );
    e.motion.query = e
        .motion
        .entries
        .iter()
        .position(|e| e.name == "Walk")
        .unwrap();
    let record = |e: &Editor, name: &str| {
        e.records
            .iter()
            .find(|r| {
                r.name.split(" \u{b7} ").next() == Some(name) && matches!(r.kind, Kind::Clip(_))
            })
            .unwrap()
            .clone()
    };
    let fast = record(&e, "Fast-walk");
    let jump = record(&e, "Jump");
    e.request_with_preview(&jump, Some(0.433), renderer, gpu);
    settle(&mut e, renderer, gpu)?;
    let before = (e.playback.time, e.source_generation, e.clip_generation.0);
    let row = e.input_regions.as_ref().unwrap().motion_results[&fast.id];
    click(
        &mut e,
        renderer,
        gpu,
        row.left_top() + egui::vec2(190., 20.),
    );
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.motion.selected_result.as_ref() == Some(&fast.id)
            && e.actual_motion_id() == Some(jump.id.as_str()),
        "Selection independent of active preview"
    );
    ensure!(
        (e.playback.time, e.source_generation, e.clip_generation.0) == before,
        "Row selection never requests or seeks playback"
    );
    let (&(_, column), &(pose, time)) = e
        .input_regions
        .as_ref()
        .unwrap()
        .motion_medoid_previews
        .iter()
        .filter(|((id, _), _)| id == &fast.id)
        .max_by_key(|((_, c), _)| *c)
        .unwrap();
    click(&mut e, renderer, gpu, pose.center());
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.actual_motion_id() == Some(fast.id.as_str())
            && (e.playback.time - time).abs() < 1e-6
            && e.motion.pending_preview.is_none(),
        "Actual medoid action activates correct clip at displayed time"
    );
    let fast_asset = Arc::clone(&e.source.as_ref().unwrap().1);
    // Hold the managed-library lock: the worker cannot complete before the newer cache hit.
    e.cpu = ByteLru::new(64 * 1024 * 1024);
    e.cpu.insert(
        fast.hash.clone(),
        Arc::clone(&fast_asset),
        fast_asset.cpu_bytes(),
    );
    let library = Arc::clone(&e.library);
    let held = library.lock().unwrap();
    ensure!(
        e.request_with_preview(&jump, Some(0.72), renderer, gpu),
        "Delayed request accepted"
    );
    ensure!(
        e.actual_motion_id() == Some(fast.id.as_str()) && e.motion.pending_preview.is_some(),
        "Pending load never marks wrong clip Previewing"
    );
    let stale = e.clip_generation.0;
    e.request_with_preview(&fast, Some(0.61), renderer, gpu);
    drop(held);
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.actual_motion_id() == Some(fast.id.as_str())
            && e.playback.time == 0.61
            && e.source_generation > stale,
        "Superseded load cannot activate or seek"
    );
    // Two requests for the same uncached clip: only the newest seek is consumed.
    e.cpu = ByteLru::new(64 * 1024 * 1024);
    let held = library.lock().unwrap();
    e.request_with_preview(&jump, Some(0.18), renderer, gpu);
    e.request_with_preview(&jump, Some(1.17), renderer, gpu);
    drop(held);
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.actual_motion_id() == Some(jump.id.as_str())
            && e.playback.time == 1.17
            && e.motion.pending_preview.is_none(),
        "Latest intent survives stale completion of same clip"
    );
    let mut missing = fast.clone();
    missing.hash = "0".repeat(64);
    missing.id = "missing-preview".into();
    e.request_with_preview(&missing, Some(0.3), renderer, gpu);
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.actual_motion_id() == Some(jump.id.as_str())
            && e.playback.time == 1.17
            && e.motion.pending_preview.is_none(),
        "Failed preview preserves actual playback and clears intent"
    );
    e.cpu = ByteLru::new(64 * 1024 * 1024);
    let held = library.lock().unwrap();
    e.request_with_preview(&missing, Some(0.21), renderer, gpu);
    e.request_with_preview(&jump, Some(1.39), renderer, gpu);
    drop(held);
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.actual_motion_id() == Some(jump.id.as_str())
            && e.playback.time == 1.39
            && e.motion.pending_preview.is_none(),
        "Stale error cannot discard newer pending intent"
    );
    e.jobs = 2;
    ensure!(
        !e.request_with_preview(&missing, Some(0.9), renderer, gpu)
            && e.motion.pending_preview.is_none(),
        "Declined load cannot leave false preview intent"
    );
    e.jobs = 0;
    ensure!(
        e.motion.previews.bytes() > 0 && e.motion.previews.bytes() <= 16 * 1024 * 1024,
        "Bounded real-pose texture accounting"
    );
    write_evidence("research/search-ui.txt",format!("{} / {:?}\nActual egui input: index five clips; select Fast-walk while Jump previews; medoid column {column} at {time:.6}s activates Fast-walk. Delayed/superseded/same-clip/failed/declined preview generations: PASS. Real-pose texture cache {} bytes <= 16 MiB. Synthetic input, not manual OS interaction.\n",gpu.info.name,gpu.info.backend,e.motion.previews.bytes()).as_bytes())?;
    write_evidence(
        "library-interactions.json",
        &serde_json::to_vec_pretty(
            &serde_json::json!({"result":"PASS","managed_library":managed,"selected_independent":true,"medoid_column":column,"medoid_time":time,"delayed":true,"superseded":true,"same_clip_latest_intent":true,"failed":true,"declined":true,"stale_failure_preserves_new_intent":true,"pose_texture_bytes":e.motion.previews.bytes(),"input":"synthetic egui events"}),
        )?,
    )?;
    Ok(())
}

pub(super) fn cache_workload() -> Result<()> {
    use crate::cache_policies::{Access, AssetCost, Trace};
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let (gpu, _) = pollster::block_on(Gpu::new(&instance, None))?;
    let renderer = Renderer::new(&gpu);
    let root = PathBuf::from("artifacts").join(format!("cache-trace-{}", std::process::id()));
    let mut library = Library::open(root)?;
    let mut sources: Vec<_> = crate::motion_fixtures::motions()
        .into_iter()
        .map(|(n, b)| (n, b.finish()))
        .collect();
    sources.extend(
        crate::animation_bench::corpus()?
            .into_iter()
            .map(|(n, b, _)| (n, b)),
    );
    let mut assets = Vec::new();
    for (name, bytes) in sources {
        let asset = import::from_bytes(&bytes, &Limits::default())?;
        let hash = library.import(&bytes, &name, &asset)?;
        if assets.iter().any(|a: &AssetCost| a.key == hash) {
            continue;
        }
        let mut decode = 0.;
        let mut upload = 0.;
        let mut gpu_bytes = 0;
        for _ in 0..5 {
            let start = Instant::now();
            std::hint::black_box(import::from_bytes(&bytes, &Limits::default())?);
            decode += start.elapsed().as_secs_f64() * 1e6;
            let start = Instant::now();
            let resident = renderer.upload(&gpu, &asset)?;
            gpu.queue.submit([]);
            gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
            upload += start.elapsed().as_secs_f64() * 1e6;
            gpu_bytes = resident.bytes;
        }
        assets.push(AssetCost {
            key: hash,
            name,
            cpu_bytes: asset.cpu_bytes(),
            gpu_bytes,
            decode_us: decode / 5.,
            upload_us: upload / 5.,
        });
    }
    let records: Vec<_> = assets
        .iter()
        .map(|a| {
            library
                .manifest
                .records
                .iter()
                .find(|r| r.hash == a.key && r.kind == Kind::Model)
                .unwrap()
                .clone()
        })
        .collect();
    let mut editor = Editor::new(library, context());
    editor.input_regions = Some(InputRegions::default());
    editor.cpu = ByteLru::new(assets.iter().map(|a| a.cpu_bytes).sum::<usize>() / 2);
    editor.gpu_cache = ByteLru::new(assets.iter().map(|a| a.gpu_bytes).sum::<usize>() / 2);
    let mut sequence = Vec::new();
    for _ in 0..4 {
        for i in 0..assets.len() {
            sequence.push(("browse", i));
        }
    }
    for i in 0..40 {
        sequence.push(("revisit", i % 2));
    }
    let heavy = assets
        .iter()
        .enumerate()
        .max_by_key(|(_, a)| a.cpu_bytes)
        .unwrap()
        .0;
    for i in 0..48 {
        sequence.push((
            "mixed-size",
            if i % 3 == 0 {
                heavy
            } else {
                (i / 3) % assets.len()
            },
        ));
    }
    let mut accesses = Vec::new();
    for (scenario, i) in sequence {
        let record = &records[i];
        editor.search = record.name.clone();
        settle(&mut editor, &renderer, &gpu)?;
        let cpu_hit = editor.cpu.contains(&record.hash);
        let gpu_hit = editor.gpu_cache.contains(&record.hash);
        let start = Instant::now();
        select(&mut editor, &renderer, &gpu, &record.id)?;
        let switch_us = start.elapsed().as_secs_f64() * 1e6;
        ensure!(
            editor.model.as_ref().is_some_and(|m| m.0 == record.hash),
            "Recorded selection must activate expected model"
        );
        accesses.push(Access {
            key: record.hash.clone(),
            scenario: scenario.into(),
            observed_cpu_hit: cpu_hit,
            observed_gpu_hit: gpu_hit,
            switch_us,
            cpu_resident_bytes: editor.cpu.bytes(),
            gpu_resident_bytes: editor.gpu_cache.bytes(),
        });
    }
    let mut environment = crate::animation_bench::environment();
    environment["gpu"] = serde_json::json!({"name":gpu.info.name,"backend":format!("{:?}",gpu.info.backend),"driver":gpu.info.driver,"driver_info":gpu.info.driver_info});
    let trace = Trace {
        environment,
        assets,
        accesses,
    };
    crate::animation_bench::save("cache-trace", &serde_json::to_value(&trace)?)?;
    crate::cache_policies::benchmark(&trace)?;
    println!(
        "Cache trace: {} actual egui selections, {} measured asset costs",
        trace.accesses.len(),
        trace.assets.len()
    );
    Ok(())
}

fn retarget_mapping(renderer: &Renderer, gpu: &Gpu, root: &std::path::Path) -> Result<()> {
    let location = root.join("retarget-managed");
    let mut library = Library::open(location.clone())?;
    let (source, _) = crate::rig_corpus::humanoid(crate::rig_corpus::Variant::default(), true);
    let (target, _) = crate::rig_corpus::humanoid(
        crate::rig_corpus::Variant {
            ambiguous: true,
            proportions: true,
            ..Default::default()
        },
        false,
    );
    for (name, bytes) in [
        ("Walk", source.finish()),
        ("Ambiguous target", target.finish()),
    ] {
        std::fs::write(root.join(format!("{name}.glb")), &bytes)?;
        let asset = import::from_bytes(&bytes, &Limits::default())?;
        library.import(&bytes, name, &asset)?;
    }
    let target = library
        .manifest
        .records
        .iter()
        .find(|r| r.name == "Ambiguous target" && r.kind == Kind::Model)
        .context("target")?
        .id
        .clone();
    let clip = library
        .manifest
        .records
        .iter()
        .find(|r| matches!(r.kind, Kind::Clip(_)))
        .context("clip")?
        .id
        .clone();
    let mut e = Editor::new(library, context());
    e.input_regions = Some(InputRegions::default());
    settle(&mut e, renderer, gpu)?;
    select(&mut e, renderer, gpu, &target)?;
    select(&mut e, renderer, gpu, &clip)?;
    ensure!(
        e.retarget.is_some(),
        "compiled automatic retarget: {}",
        e.status
    );
    ensure!(
        e.retarget
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .any(|r| r.source == 7 && r.confidence == retarget::Confidence::Ambiguous),
        "deliberate arm ambiguity surfaced"
    );
    e.mapping_overrides.push(Override {
        source: 7,
        target: None,
    });
    e.request_plan(true);
    settle(&mut e, renderer, gpu)?;
    let open = e.input_regions.as_ref().unwrap().mapping_open.center();
    click(&mut e, renderer, gpu, open);
    settle(&mut e, renderer, gpu)?;
    ensure!(e.mapping_open, "mapping editor button");
    e.mapping_filter = "UpperArm.L".into();
    settle(&mut e, renderer, gpu)?;
    let row = e.input_regions.as_ref().unwrap().mapping_rows[&7].center();
    click(&mut e, renderer, gpu, row);
    ensure!(e.selected_pair == Some(7), "pair highlights");
    ensure!(
        e.input_regions.as_ref().unwrap().mapping_drawn_pair == Some(7),
        "workspace_mapping_row_selects_both_joints: same-frame canvas selection"
    );
    let combo = e.input_regions.as_ref().unwrap().mapping_combos[&7].center();
    click(&mut e, renderer, gpu, combo);
    input(&mut e, renderer, gpu, Vec::new());
    let mut choice = *e
        .input_regions
        .as_ref()
        .unwrap()
        .mapping_options
        .get(&(7, Some(7)))
        .context("target option")?;
    // Comfortable row heights put later choices outside the popup. Scroll the
    // real dropdown before clicking, just as a user would.
    for _ in 0..12 {
        let popup = e.input_regions.as_ref().unwrap().mapping_popup;
        if popup.contains_rect(choice) {
            break;
        }
        let direction = if choice.center().y > popup.center().y {
            -1.
        } else {
            1.
        };
        input(
            &mut e,
            renderer,
            gpu,
            vec![
                egui::Event::PointerMoved(popup.center()),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0., direction * 100.),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: Default::default(),
                },
            ],
        );
        for _ in 0..4 {
            input(&mut e, renderer, gpu, Vec::new());
        }
        choice = e.input_regions.as_ref().unwrap().mapping_options[&(7, Some(7))];
    }
    ensure!(
        e.input_regions
            .as_ref()
            .unwrap()
            .mapping_popup
            .contains_rect(choice),
        "target option visible after scrolling: option {choice:?}, popup {:?}",
        e.input_regions.as_ref().unwrap().mapping_popup
    );
    click(&mut e, renderer, gpu, choice.center());
    ensure!(
        matches!(
            e.ui_state.correspondence.operation,
            crate::ui::correspondence::Operation::Selecting(_)
        ),
        "Dropdown proposes without silently committing"
    );
    // Let the dismissed picker retire its hit-test area before confirming.
    input(&mut e, renderer, gpu, Vec::new());
    input(&mut e, renderer, gpu, Vec::new());
    let confirm = e.input_regions.as_ref().unwrap().mapping_controls["confirm"].center();
    click(&mut e, renderer, gpu, confirm);
    ensure!(
        e.mapping_overrides
            .iter()
            .any(|o| o.source == 7 && o.target == Some(7)),
        "actual egui target selection"
    );
    let workspace_cases = workspace_retarget_interactions(&mut e, renderer, gpu)?;
    let save = e.input_regions.as_ref().unwrap().mapping_save.center();
    click(&mut e, renderer, gpu, save);
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.retarget
            .as_ref()
            .unwrap()
            .overrides
            .iter()
            .any(|o| o.source == 7 && o.target == Some(7)),
        "saved compiled correction: {}",
        e.status
    );
    workspace_timeline_interactions(&mut e, renderer, gpu)?;
    workspace_drawer_interactions(&mut e, renderer, gpu)?;
    drop(e);
    let mut reopened = Editor::new(Library::open(location.clone())?, context());
    reopened.input_regions = Some(InputRegions::default());
    settle(&mut reopened, renderer, gpu)?;
    select(&mut reopened, renderer, gpu, &target)?;
    select(&mut reopened, renderer, gpu, &clip)?;
    ensure!(
        reopened
            .retarget
            .as_ref()
            .context("reloaded plan")?
            .overrides
            .iter()
            .any(|o| o.source == 7 && o.target == Some(7)),
        "correction survives fresh editor/library"
    );
    ensure!(
        reopened.selected_pair.is_none(),
        "Saved mapping-profile restoration does not imply joint-selection persistence"
    );
    let open = reopened
        .input_regions
        .as_ref()
        .unwrap()
        .mapping_open
        .center();
    click(&mut reopened, renderer, gpu, open);
    settle(&mut reopened, renderer, gpu)?;
    let reset = reopened
        .input_regions
        .as_ref()
        .unwrap()
        .mapping_reset
        .center();
    click(&mut reopened, renderer, gpu, reset);
    settle(&mut reopened, renderer, gpu)?;
    ensure!(
        reopened.mapping_overrides.is_empty()
            && reopened
                .retarget
                .as_ref()
                .is_some_and(|p| !p.overrides.is_empty()),
        "Reset is pending until explicit Apply / Preview"
    );
    let apply = reopened.input_regions.as_ref().unwrap().mapping_controls["apply"].center();
    click(&mut reopened, renderer, gpu, apply);
    settle(&mut reopened, renderer, gpu)?;
    ensure!(
        reopened.mapping_overrides.is_empty()
            && reopened
                .retarget
                .as_ref()
                .is_some_and(|p| p.overrides.is_empty()),
        "Apply / Preview compiles reset edits without reloading a saved override"
    );
    let source_model = reopened
        .records
        .iter()
        .find(|r| r.kind == Kind::Model && r.id != target)
        .context("source model")?
        .id
        .clone();
    select(&mut reopened, renderer, gpu, &source_model)?;
    select(&mut reopened, renderer, gpu, &target)?;
    let resources = renderer.resource_stats();
    for _ in 0..100 {
        select(&mut reopened, renderer, gpu, &source_model)?;
        select(&mut reopened, renderer, gpu, &target)?;
        ensure!(
            reopened.jobs == 0 && renderer.resource_stats() == resources,
            "resident retarget switch started work or uploaded resources"
        );
    }
    write_evidence(
        "retarget-ui-smoke.txt",
        format!(
            "Adapter: {} / {:?}\nActual egui events: select foreign rig and clip, open mapping editor, select/highlight a pair, choose target from dropdown, save mapping: PASS\nDeliberately ambiguous arm mapping is corrected from unmatched to target joint 7.\nFresh Editor + Library reloads the saved correction: PASS\nReset Overrides remains pending until explicit Apply / Preview; old saved corrections are not reloaded: PASS\n100 resident foreign-rig model switches: no disk/preprocessing jobs and unchanged GPU resource/upload counts: PASS\nProfile checksum/corruption/stale-algorithm/rig/settings tests: tests/retarget.rs.\n",
            gpu.info.name, gpu.info.backend
        ),
    )?;
    write_evidence(
        "retarget-interactions.json",
        serde_json::to_string_pretty(
            &serde_json::json!({"kind":"Synthetic egui interaction assertions; not OS input or native raster evidence","adapter":gpu.info.name,"backend":format!("{:?}",gpu.info.backend),"cases":workspace_cases,"saved_profile_restores_mapping":true,"joint_selection_persisted":false,"reset_does_not_reload_stale_profile":true,"resident_model_switches":100,"managed_library":location,"fixtures":[root.join("Walk.glb"),root.join("Ambiguous target.glb")]}),
        )?,
    )?;
    Ok(())
}

fn write_evidence(name: &str, bytes: impl AsRef<[u8]>) -> Result<()> {
    let root = std::env::var_os("ARMATRA_REPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/reports"));
    let path = root.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

fn workspace_drawer_interactions(e: &mut Editor, r: &Renderer, g: &Gpu) -> Result<()> {
    use crate::ui::drawers::Kind as Drawer;
    fn press(e: &mut Editor, r: &Renderer, g: &Gpu, size: egui::Vec2, pos: egui::Pos2) {
        frame(
            e,
            r,
            g,
            size,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        frame(
            e,
            r,
            g,
            size,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
    }
    let original = e.ui_state.preferences.clone();
    e.ui_state.preferences.assets_width = 282.;
    e.ui_state.preferences.inspector_width = 302.;
    e.ui_state.preferences.timeline_height = 194.;
    let expected = serde_json::to_value(&e.ui_state.preferences)?;
    let mut evidence = vec![];
    for (workspace, scale) in [(Workspace::Animate, 1.5), (Workspace::Retarget, 1.25)] {
        e.ui_state.workspace = workspace;
        e.mapping_open = workspace == Workspace::Retarget;
        e.motion.open = false;
        e.ctx.set_zoom_factor(1.);
        for _ in 0..4 {
            frame(e, r, g, egui::vec2(1440., 900.), vec![]);
        }
        let normal = e
            .input_regions
            .as_ref()
            .map(|r| (r.viewport, r.mapping_canvas, r.seek))
            .unwrap();
        e.ctx.set_zoom_factor(scale);
        let size = egui::vec2(1280., 720.) / scale;
        for _ in 0..4 {
            frame(e, r, g, size, vec![]);
        }
        let regions = e.input_regions.as_ref().unwrap();
        let before = (regions.viewport, regions.mapping_canvas, regions.seek);
        let selection = e.selected_pair;
        let time = e.playback.time;
        let cycle = e.playback.cycle;
        let yaw = e.camera.yaw;
        let open = regions.assets_drawer.center();
        press(e, r, g, size, open);
        frame(e, r, g, size, vec![]);
        ensure!(
            e.ui_state.drawer.open == Some(Drawer::Assets),
            "Assets drawer opens at {scale}"
        );
        let regions = e.input_regions.as_ref().unwrap();
        ensure!(
            (regions.viewport, regions.mapping_canvas, regions.seek) == before,
            "Assets cannot reallocate canvas or timeline"
        );
        ensure!(
            regions.drawer.width() == 304.,
            "304-point foreground overlay"
        );
        let search = regions.assets_search.center();
        let old_search = e.search.clone();
        let playing = e.playback.playing;
        press(e, r, g, size, search);
        frame(
            e,
            r,
            g,
            size,
            vec![
                egui::Event::Key {
                    key: egui::Key::Space,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                },
                egui::Event::Text(" ".into()),
            ],
        );
        ensure!(
            e.playback.playing == playing && e.search.len() == old_search.len() + 1,
            "Focused drawer search suppresses Space playback shortcut"
        );
        e.search = old_search;
        frame(
            e,
            r,
            g,
            size,
            vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        let regions = e.input_regions.as_ref().unwrap();
        let detail = regions.detail_drawer.center();
        press(e, r, g, size, detail);
        frame(e, r, g, size, vec![]);
        ensure!(
            e.ui_state.drawer.open == Some(Drawer::Detail),
            "One drawer switches to Detail"
        );
        let regions = e.input_regions.as_ref().unwrap();
        ensure!(
            (regions.viewport, regions.mapping_canvas, regions.seek) == before,
            "Detail cannot reallocate canvas or timeline"
        );
        let opener = e.ui_state.drawer.opener;
        frame(
            e,
            r,
            g,
            size,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        ensure!(
            e.ui_state.drawer.open.is_none() && e.ctx.memory(|m| m.focused()) == opener,
            "Escape closes and restores opener focus"
        );
        press(e, r, g, size, open);
        frame(e, r, g, size, vec![]);
        let outside = if workspace == Workspace::Animate {
            e.input_regions.as_ref().unwrap().seek.right_center()
        } else {
            e.input_regions
                .as_ref()
                .unwrap()
                .mapping_canvas
                .right_center()
        };
        press(e, r, g, size, outside);
        ensure!(
            e.ui_state.drawer.open.is_none()
                && e.playback.time == time
                && e.playback.cycle == cycle
                && e.camera.yaw == yaw
                && e.selected_pair == selection,
            "Outside dismissal consumes selection/seek/orbit press"
        );
        press(e, r, g, size, open);
        frame(e, r, g, size, vec![]);
        let close = e.input_regions.as_ref().unwrap().drawer_close.center();
        press(e, r, g, size, close);
        ensure!(
            e.ui_state.drawer.open.is_none()
                && e.ctx.memory(|m| m.focused()) == e.ui_state.drawer.opener,
            "Close returns focus"
        );
        e.ctx.set_zoom_factor(1.);
        for _ in 0..4 {
            frame(e, r, g, egui::vec2(1440., 900.), vec![]);
        }
        ensure!(
            serde_json::to_value(&e.ui_state.preferences)? == expected,
            "Normal preferences restored unchanged after compact resizing"
        );
        ensure!(
            e.input_regions
                .as_ref()
                .map(|r| (r.viewport, r.mapping_canvas, r.seek))
                .unwrap()
                == normal,
            "Normal canvas and timeline geometry restored after resize"
        );
        let root = e.library.lock().unwrap().root.clone();
        e.ui_state.preferences.save(&root)?;
        ensure!(
            serde_json::to_value(Preferences::load(&root))? == expected,
            "Preferences restore from disk after compact resize"
        );
        evidence.push(serde_json::json!({"workspace":format!("{workspace:?}"),"scale":scale,"canvas_and_timeline_unchanged":true,"selection_preserved":true,"outside_press_consumed":true,"escape_close_focus":true,"preferences_restored":true}));
    }
    e.ui_state.preferences = original;
    e.ui_state.drawer.open = None;
    write_evidence(
        "drawer-interactions.json",
        &serde_json::to_vec_pretty(&evidence)?,
    )?;
    settle(e, r, g)
}

fn workspace_timeline_interactions(e: &mut Editor, r: &Renderer, g: &Gpu) -> Result<()> {
    let animate = e.input_regions.as_ref().unwrap().animate_open.center();
    click(e, r, g, animate);
    settle(e, r, g)?;
    let state = (
        e.playback.time,
        e.playback.cycle,
        e.playback.speed,
        e.playback.looping,
        e.playback.playing,
    );
    let duration = e.source.as_ref().unwrap().1.clips[e.source.as_ref().unwrap().2].duration;
    e.playback.playing = false;
    e.playback.speed = -1.25;
    e.playback.looping = true;
    e.playback.cycle = 4;
    e.playback.time = duration * 0.25;
    input(e, r, g, vec![]);
    let track = e.input_regions.as_ref().unwrap().seek;
    let y = e.input_regions.as_ref().unwrap().timeline_clip.center().y;
    let hover = egui::pos2(
        crate::ui::timeline::x_at(duration * 0.75, track, duration),
        y,
    );
    let previous = (e.playback.time, e.playback.cycle, e.cursors.len());
    input(e, r, g, vec![egui::Event::PointerMoved(hover)]);
    ensure!(
        previous == (e.playback.time, e.playback.cycle, e.cursors.len()),
        "workspace_timeline_hover_does_not_seek or reset cycle/cursors"
    );
    ensure!(
        (e.input_regions.as_ref().unwrap().timeline_hover.unwrap() - duration * 0.75).abs() < 1e-5,
        "Hover time is projected from actual track"
    );
    click(e, r, g, egui::pos2(track.left(), y));
    ensure!(
        e.playback.time == 0. && e.playback.cycle == 0,
        "workspace_timeline_endpoints_are_exact: left"
    );
    click(e, r, g, egui::pos2(track.right(), y));
    ensure!(
        e.playback.time == duration,
        "workspace_timeline_endpoints_are_exact: right"
    );
    drag(
        e,
        r,
        g,
        egui::pos2(track.left() + track.width() * 0.8, y),
        egui::vec2(-track.width() * 0.6, 0.),
        egui::PointerButton::Primary,
    );
    ensure!(
        (e.playback.time - duration * 0.2).abs() < 0.001,
        "workspace_timeline_click_and_drag_seek"
    );
    ensure!(
        e.playback.speed == -1.25 && e.playback.looping,
        "Seek preserves signed speed and looping"
    );
    for (physical, scale) in [
        (egui::vec2(1440., 900.), 1.),
        (egui::vec2(1280., 720.), 1.5),
    ] {
        e.ctx.set_zoom_factor(scale);
        for _ in 0..3 {
            frame(e, r, g, physical / scale, vec![]);
        }
        let regions = e.input_regions.as_ref().unwrap();
        let contacts = e.contacts.as_ref().context("retarget contact data")?;
        ensure!(
            regions.timeline_contacts.len() == contacts.intervals.len()
                && !regions.timeline_contacts.is_empty(),
            "workspace_contacts_match_model_intervals: every real interval painted"
        );
        for bar in &regions.timeline_contacts {
            ensure!(
                contacts.intervals.iter().any(|i| i.source == bar.source
                    && i.target == bar.target
                    && i.start == bar.start
                    && i.end == bar.end),
                "Contact geometry retains real interval identity"
            );
            ensure!(
                (bar.rect.left() - crate::ui::timeline::x_at(bar.start, regions.seek, duration))
                    .abs()
                    < 1e-4
                    && (bar.rect.right()
                        - crate::ui::timeline::x_at(bar.end, regions.seek, duration))
                    .abs()
                        < 1e-4,
                "Exact contact boundaries at {scale} scale"
            );
            ensure!(
                regions.seek.contains_rect(bar.rect),
                "Contact lane inside bounded timeline at {physical:?} / {scale}"
            );
        }
    }
    e.ctx.set_zoom_factor(1.);
    settle(e, r, g)?;
    (
        e.playback.time,
        e.playback.cycle,
        e.playback.speed,
        e.playback.looping,
        e.playback.playing,
    ) = state;
    let retarget = e.input_regions.as_ref().unwrap().mapping_open.center();
    click(e, r, g, retarget);
    settle(e, r, g)?;
    write_evidence(
        "timeline-interactions.json",
        serde_json::to_string_pretty(
            &serde_json::json!({"kind":"Synthetic egui input/state assertions","workspace_timeline_hover_does_not_seek":true,"workspace_timeline_endpoints_are_exact":true,"workspace_timeline_click_and_drag_seek":true,"workspace_contacts_match_model_intervals":true,"signed_speed_and_loop_preserved":true,"normal_and_150_percent_geometry":true,"contact_source":"Actual retarget contacts; analyzed rig sides, no node-index convention"}),
        )?,
    )
}

fn workspace_retarget_interactions(
    e: &mut Editor,
    renderer: &Renderer,
    gpu: &Gpu,
) -> Result<Vec<&'static str>> {
    use crate::ui::correspondence::State;
    settle(e, renderer, gpu)?;
    let source = e.input_regions.as_ref().unwrap().mapping_joints[&(0, 8)];
    click(e, renderer, gpu, source);
    ensure!(
        e.selected_pair == Some(8)
            && e.input_regions.as_ref().unwrap().mapping_drawn_pair == Some(8),
        "workspace_retarget_joint_click_selects_mapping_row: source and canvas same frame"
    );
    let target = e.input_regions.as_ref().unwrap().mapping_joints[&(1, 7)];
    click(e, renderer, gpu, target);
    ensure!(
        e.selected_pair == Some(7)
            && e.input_regions.as_ref().unwrap().mapping_drawn_pair == Some(7),
        "target click resolves source correspondence"
    );
    e.mapping_filter.clear();
    settle(e, renderer, gpu)?;
    let row = e.input_regions.as_ref().unwrap().mapping_rows[&7].center();
    click(e, renderer, gpu, row);
    input(
        e,
        renderer,
        gpu,
        vec![egui::Event::Key {
            key: egui::Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    ensure!(
        e.selected_pair == Some(8)
            && e.input_regions.as_ref().unwrap().mapping_drawn_pair == Some(8),
        "Keyboard mapping row navigation synchronizes canvas: selected {:?}, drawn {:?}, focus {:?}",
        e.selected_pair,
        e.input_regions.as_ref().unwrap().mapping_drawn_pair,
        e.ctx.memory(|m| m.focused())
    );
    input(
        e,
        renderer,
        gpu,
        vec![egui::Event::Key {
            key: egui::Key::ArrowDown,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    let target = e.input_regions.as_ref().unwrap().mapping_joints[&(1, 7)];
    click(e, renderer, gpu, target);
    let key = e.retarget.as_ref().unwrap().key.clone();
    let generation = e.plan_generation.0;
    for node in [8, 7] {
        let control = e.input_regions.as_ref().unwrap().mapping_reassign.center();
        click(e, renderer, gpu, control);
        ensure!(
            matches!(
                e.ui_state.correspondence.operation,
                crate::ui::correspondence::Operation::Selecting(_)
            ),
            "Map Joints opens confirmation selection"
        );
        let source = e.input_regions.as_ref().unwrap().mapping_joints[&(0, 7)];
        click(e, renderer, gpu, source);
        let target = e.input_regions.as_ref().unwrap().mapping_joints[&(1, node)];
        click(e, renderer, gpu, target);
        let confirm = e.input_regions.as_ref().unwrap().mapping_controls["confirm"].center();
        click(e, renderer, gpu, confirm);
        ensure!(
            e.mapping_overrides
                .iter()
                .any(|o| o.source == 7 && o.target == Some(node))
                && matches!(
                    e.ui_state.correspondence.operation,
                    crate::ui::correspondence::Operation::Idle
                ),
            "workspace_manual_reassignment_marks_plan_dirty: actual target hit"
        );
        ensure!(
            e.retarget.as_ref().unwrap().key == key
                && e.plan_generation.0 == generation
                && !e.plan_pending,
            "Reassignment does not compile or change animation plan"
        );
        ensure!(
            e.input_regions
                .as_ref()
                .unwrap()
                .mapping_selected_points
                .len()
                == 2,
            "Both selected endpoints painted immediately"
        );
    }
    for side in 0..2 {
        let rect = e.input_regions.as_ref().unwrap().mapping_canvas;
        let from = egui::pos2(
            if side == 0 {
                rect.left() + 40.
            } else {
                rect.right() - 40.
            },
            rect.top() + 70.,
        );
        let before = e.ui_state.correspondence.cameras;
        drag(
            e,
            renderer,
            gpu,
            from,
            egui::vec2(30., 15.),
            egui::PointerButton::Primary,
        );
        ensure!(
            e.ui_state.correspondence.cameras[0].yaw == e.ui_state.correspondence.cameras[1].yaw
                && e.ui_state.correspondence.cameras[0].pitch
                    == e.ui_state.correspondence.cameras[1].pitch
                && (0..2).all(
                    |s| e.ui_state.correspondence.cameras[s].pan == before[s].pan
                        && e.ui_state.correspondence.cameras[s].zoom == before[s].zoom
                )
                && e.ui_state.correspondence.cameras != before,
            "workspace_linked_orbit_updates_both_cameras from either side"
        );
    }
    e.ui_state.correspondence.unlinked = false;
    let canvas = e.input_regions.as_ref().unwrap().mapping_canvas;
    let pointer = canvas.left_top() + egui::vec2(40., 60.);
    let target = e.ui_state.correspondence.cameras[1];
    let pan = e.ui_state.correspondence.cameras[0].pan;
    drag(
        e,
        renderer,
        gpu,
        pointer,
        egui::vec2(18., -12.),
        egui::PointerButton::Secondary,
    );
    ensure!(
        e.ui_state.correspondence.cameras[1] == target
            && e.ui_state.correspondence.cameras[0].pan != pan,
        "Unlinked pan leaves opposite camera unchanged"
    );
    let zoom = e.ui_state.correspondence.cameras[0].zoom;
    input(
        e,
        renderer,
        gpu,
        vec![
            egui::Event::PointerMoved(pointer),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0., 80.),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Default::default(),
            },
        ],
    );
    ensure!(
        e.ui_state.correspondence.cameras[0].zoom > zoom
            && e.ui_state.correspondence.cameras[1] == target,
        "Wheel zoom applies only to unlinked hovered rig"
    );
    // Let smooth-wheel input finish before framing with the actual toolbar button.
    for _ in 0..30 {
        input(e, renderer, gpu, vec![]);
    }
    let frame_button = e.input_regions.as_ref().unwrap().mapping_frame.center();
    click(e, renderer, gpu, frame_button);
    ensure!(
        e.ui_state.correspondence.cameras == State::default().cameras,
        "Frame resets both cameras without starting an orbit"
    );
    e.ui_state.correspondence = State::default();
    let preferences = (
        e.ui_state.preferences.assets_width,
        e.ui_state.preferences.inspector_width,
        e.ui_state.preferences.timeline_height,
    );
    for (physical, scale) in [
        (egui::vec2(1440., 900.), 1.),
        (egui::vec2(1280., 720.), 1.25),
        (egui::vec2(1280., 720.), 1.5),
    ] {
        e.ctx.set_zoom_factor(scale);
        for _ in 0..3 {
            frame(e, renderer, gpu, physical / scale, vec![]);
        }
        let r = e.input_regions.as_ref().unwrap();
        ensure!(
            r.mapping_rows.get(&7).is_some_and(|r| r.height() >= 30.),
            "Selected mapping row must be visible at {physical:?} / {scale}: {:?}",
            r.mapping_rows.get(&7)
        );
        ensure!(
            r.mapping_canvas.width() > 500. && r.mapping_canvas.height() > 180.,
            "Retarget primary canvas visible at {physical:?} / {scale}"
        );
        ensure!(
            r.mapping_labels.len() == 2 && r.mapping_selected_points.len() == 2,
            "Selected labels and endpoints visible"
        );
        for label in &r.mapping_labels {
            ensure!(
                r.mapping_canvas.contains_rect(*label),
                "Selected label inside canvas"
            );
            ensure!(
                !r.mapping_selected_points
                    .iter()
                    .any(|p| label.expand(14.).contains(*p)),
                "Selected label clear of both joint markers"
            );
            ensure!(
                !r.mapping_selected_bones
                    .iter()
                    .any(|b| label.intersects(egui::Rect::from_two_pos(b[0], b[1]).expand(3.))),
                "Selected label clear of emphasized outgoing bone"
            );
        }
        ensure!(
            preferences
                == (
                    e.ui_state.preferences.assets_width,
                    e.ui_state.preferences.inspector_width,
                    e.ui_state.preferences.timeline_height
                ),
            "Compact Retarget must not overwrite saved normal preferences"
        );
    }
    e.ctx.set_zoom_factor(1.);
    settle(e, renderer, gpu)?;
    Ok(vec![
        "workspace_retarget_joint_click_selects_mapping_row: PASS",
        "workspace_target_joint_selects_source_correspondence: PASS",
        "workspace_mapping_row_selects_both_joints_in_same_frame: PASS",
        "workspace_mapping_list_keyboard_navigation: PASS",
        "workspace_manual_reassignment_marks_plan_dirty_without_compile: PASS",
        "workspace_linked_orbit_updates_both_cameras_from_either_side: PASS",
        "workspace_linked_independent_pan_wheel_and_frame_toolbar: PASS",
        "workspace_layout_1440_900_100_and_1280_720_125_150: PASS",
        "workspace_selected_mapping_row_is_visible: PASS",
        "workspace_selected_labels_clear_markers_and_bones: PASS",
        "workspace_retarget_compact_preserves_normal_preferences: PASS",
    ])
}

fn repair_workflow(renderer: &Renderer, gpu: &Gpu, root: &std::path::Path) -> Result<()> {
    legacy_profile_preview(renderer, gpu, root)?;
    let seeded = root.join("included-samples");
    let mut included = Editor::new(Library::open_seeded(seeded.clone())?, context());
    included.input_regions = Some(InputRegions::default());
    included.request_motion_index();
    settle(&mut included, renderer, gpu)?;
    ensure!(
        included.records.len() == 9 && included.motion.entries.len() == 9,
        "Bundled motions immediately indexed"
    );
    let id = included.records[0].id.clone();
    included.motion.selected_result = Some(id.clone());
    input(&mut included, renderer, gpu, vec![]);
    let delete = included.input_regions.as_ref().unwrap().library_controls["delete"].center();
    click(&mut included, renderer, gpu, delete);
    settle(&mut included, renderer, gpu)?;
    ensure!(
        included.records.len() == 8,
        "Delete motion button removes a normal library item"
    );
    drop(included);
    let included = Library::open_seeded(seeded)?;
    ensure!(
        included.manifest.records.len() == 8
            && included.manifest.records.iter().all(|r| r.id != id),
        "Deleted seed stays deleted on restart"
    );
    drop(included);
    let managed = root.join("repair-three-managed");
    let mut library = Library::open(managed.clone())?;
    let mut builder = crate::fixtures::robot(false, false);
    let clip = builder.json["animations"][0].clone();
    builder.json["animations"] = serde_json::json!([clip.clone(), clip.clone(), clip]);
    for (i, clip) in builder.json["animations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        clip["name"] = serde_json::json!(format!("Unclassified motion {}", i + 1));
    }
    let bytes = builder.finish();
    let asset = import::from_bytes(&bytes, &Limits::default())?;
    ensure!(
        asset
            .clips
            .iter()
            .all(|c| crate::motion_search::summarize(&asset, c).is_err()),
        "Fixture must fail descriptors"
    );
    library.import(&bytes, "Repair fixture", &asset)?;
    let fixture = root.join("three-unclassified.glb");
    std::fs::write(&fixture, &bytes)?;
    let mut e = Editor::new(library, context());
    e.input_regions = Some(InputRegions::default());
    e.request_motion_index();
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.motion.entries.is_empty() && e.motion.diagnostics.len() == 3,
        "Three per-clip failures"
    );
    ensure!(
        e.input_regions.as_ref().unwrap().motion_results.len() == 3,
        "Three visible records when all summaries fail"
    );
    let clips: Vec<_> = e
        .records
        .iter()
        .filter(|r| matches!(r.kind, Kind::Clip(_)))
        .cloned()
        .collect();
    for clip in &clips {
        ensure!(
            e.motion.diagnostics[&clip.id].playable
                && !e.motion.diagnostics[&clip.id].reason.is_empty(),
            "Playback and descriptor eligibility separated"
        );
        let button = e.input_regions.as_ref().unwrap().motion_preview_buttons[&clip.id];
        click(&mut e, renderer, gpu, button.center());
        settle(&mut e, renderer, gpu)?;
        ensure!(
            e.actual_motion_id() == Some(clip.id.as_str()),
            "Fallback preview selects original clip ID"
        );
        e.prepare_source_preview(renderer, gpu);
        ensure!(
            e.source_preview.is_some(),
            "Actual source GPU preview exists without descriptor"
        );
    }
    e.motion.selected_result = Some(clips[1].id.clone());
    e.request_motion_index();
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.motion.selected_result.as_ref() == Some(&clips[1].id),
        "Refresh preserves selection"
    );
    let generation = e.motion.generation.0;
    e.tx.send(Job::Motion {
        generation: generation.saturating_sub(1),
        entries: vec![],
        skipped: 0,
        diagnostics: HashMap::new(),
    })?;
    e.drain(renderer, gpu);
    ensure!(
        e.motion.diagnostics.len() == 3 && e.motion.selected_result.as_ref() == Some(&clips[1].id),
        "Stale index result discarded"
    );
    // Every utility body fits and accepts wheel and page keys at compact DPI.
    e.motion.open = false;
    e.ui_state.workspace = Workspace::Animate;
    for scale in [1.25, 1.5] {
        e.ctx.set_zoom_factor(scale);
        let size = egui::vec2(1280., 720.) / scale;
        for name in ["help", "settings", "labs"] {
            e.ui_state.help = name == "help";
            e.ui_state.settings = name == "settings";
            e.ui_state.labs = name == "labs";
            for _ in 0..4 {
                frame(&mut e, renderer, gpu, size, vec![]);
            }
            let regions = e.input_regions.as_ref().unwrap();
            let window = regions.utility_windows[name];
            let (body, before) = regions.utility_scrolls[name];
            ensure!(
                egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(window),
                "{name} bounded at {scale}: {window:?}"
            );
            frame(
                &mut e,
                renderer,
                gpu,
                size,
                vec![
                    egui::Event::PointerMoved(body.center()),
                    egui::Event::Key {
                        key: egui::Key::PageDown,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    },
                ],
            );
            for _ in 0..3 {
                frame(&mut e, renderer, gpu, size, vec![]);
            }
            let after = e.input_regions.as_ref().unwrap().utility_scrolls[name].1;
            ensure!(
                after > before,
                "{name} PageDown scrolls at {scale}: {before} -> {after}"
            );
            frame(
                &mut e,
                renderer,
                gpu,
                size,
                vec![
                    egui::Event::PointerMoved(body.center()),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0., 120.),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Unknown,
                        modifiers: Default::default(),
                    },
                ],
            );
            for _ in 0..6 {
                frame(&mut e, renderer, gpu, size, vec![]);
            }
            ensure!(
                e.input_regions.as_ref().unwrap().utility_scrolls[name].1 < after,
                "{name} mouse wheel scrolls"
            );
        }
    }
    e.ui_state.help = false;
    e.ui_state.settings = false;
    e.ui_state.labs = false;
    e.ctx.set_zoom_factor(1.);
    drop(e);
    // Restart from immutable blobs; the source fixture can be removed.
    std::fs::remove_file(&fixture)?;
    let mut e = Editor::new(Library::open(managed.clone())?, context());
    e.input_regions = Some(InputRegions::default());
    e.request_motion_index();
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.input_regions.as_ref().unwrap().motion_results.len() == 3,
        "Restart shows all three"
    );
    let (name, builder) = crate::motion_fixtures::motions()
        .into_iter()
        .find(|(n, _)| n == "Walk")
        .unwrap();
    let path = root.join("new-motion.glb");
    std::fs::write(&path, builder.finish())?;
    e.import(Some(path));
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.records
            .iter()
            .filter(|r| matches!(r.kind, Kind::Clip(_)))
            .count()
            == 4
            && e.motion.entries.len() == 1,
        "New import reindexes mixed eligible/ineligible clips: {name}"
    );
    let mut unsupported = crate::fixtures::robot(false, false);
    let input = unsupported.accessor(&[0., 1.], "SCALAR", 1);
    let output = unsupported.accessor(&[0., 1.], "SCALAR", 1);
    unsupported.json["animations"] = serde_json::json!([{"name":"Unsupported weights","samplers":[{"input":input,"output":output}],"channels":[{"sampler":0,"target":{"node":12,"path":"weights"}}]}]);
    let path = root.join("unsupported.glb");
    std::fs::write(&path, unsupported.finish())?;
    e.import(Some(path));
    settle(&mut e, renderer, gpu)?;
    let unsupported = e
        .records
        .iter()
        .find(|r| r.name.starts_with("Unsupported weights"))
        .unwrap();
    ensure!(
        !e.motion.diagnostics[&unsupported.id].playable
            && e.motion.diagnostics[&unsupported.id]
                .reason
                .contains("Unsupported"),
        "Imported unplayable clip retains an explicit per-clip error"
    );
    write_evidence(
        "repair-interactions.json",
        serde_json::to_vec_pretty(
            &serde_json::json!({"result":"PASS","managed_library":managed,"three_visible":true,"per_clip_reasons":true,"preview_switching":true,"stale_generation":true,"refresh_selection":true,"restart":true,"new_import_mixed":true,"compact_windows_keyboard_and_wheel":[1.25,1.5],"input":"Synthetic egui events; actual source GPU upload; native captures separately"}),
        )?,
    )?;
    println!(
        "Repair workflow: imported records, descriptor failures, real previews, refresh/restart/new import/stale results, utility scrolling: PASS"
    );
    Ok(())
}

fn format_import_workflow(renderer: &Renderer, gpu: &Gpu, root: &std::path::Path) -> Result<()> {
    let managed = root.join("format-managed");
    let mut b = crate::fixtures::robot(false, false);
    b.json["buffers"][0] = serde_json::json!({"uri":"external.bin","byteLength":b.bin.len()});
    let external = root.join("external.bin");
    std::fs::write(&external, &b.bin)?;
    let gltf = root.join("external.gltf");
    std::fs::write(&gltf, serde_json::to_vec(&b.json)?)?;
    let mut e = Editor::new(Library::open(managed.clone())?, context());
    e.input_regions = Some(InputRegions::default());
    e.import(Some(gltf.clone()));
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.model.is_some() && e.source.is_some() && e.records.len() == 2,
        "Async glTF import activates mesh, skin and animation"
    );
    let bvh = root.join("Imported skeletal motion.bvh");
    std::fs::write(
        &bvh,
        "HIERARCHY ROOT Hips { OFFSET 0 1 0 CHANNELS 6 Xposition Yposition Zposition Zrotation Xrotation Yrotation JOINT Head { OFFSET 0 1 0 CHANNELS 3 Zrotation Xrotation Yrotation End Site { OFFSET 0 0.2 0 } } } MOTION Frames: 3 Frame Time: 0.5 0 0 0 0 0 0 0 0 0 0.1 -0.2 0 20 0 0 0 0 0 0.2 -0.4 0 40 0 0 0 0 0",
    )?;
    e.import(Some(bvh.clone()));
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.source
            .as_ref()
            .is_some_and(|s| s.1.meshes.is_empty() && s.1.clips[0].duration == 1.),
        "Async BVH motion import selects evaluated source"
    );
    e.request_motion_index();
    settle(&mut e, renderer, gpu)?;
    e.prepare_source_preview(renderer, gpu);
    ensure!(
        e.source_preview.is_some()
            && e.records
                .iter()
                .filter(|r| matches!(r.kind, Kind::Clip(_)))
                .count()
                == 2,
        "BVH source GPU preview and both imported records"
    );
    let hashes: Vec<_> = e.records.iter().map(|r| r.hash.clone()).collect();
    drop(e);
    for path in [external, gltf, bvh] {
        std::fs::remove_file(path)?;
    }
    let library = Library::open(managed.clone())?;
    for hash in hashes {
        library.load(&hash)?;
    }
    write_evidence(
        "format-import-interactions.json",
        serde_json::to_vec_pretty(
            &serde_json::json!({"result":"PASS","managed_library":managed,"async_gltf_mesh_skin_animation":true,"async_bvh_motion":true,"gpu_skeleton_preview":true,"originals_removed":true,"persistent_reload":true,"input":"Engine import jobs and actual GPU upload; OS picker/drop unverified"}),
        )?,
    )?;
    println!(
        "Format import workflow: glTF mesh/skin/animation, BVH motion, GPU skeleton preview, source removal and managed reload: PASS"
    );
    Ok(())
}

fn legacy_profile_preview(renderer: &Renderer, gpu: &Gpu, root: &std::path::Path) -> Result<()> {
    let managed = root.join("legacy-loop-profile");
    let mut library = Library::open(managed.clone())?;
    let (builder, _) = crate::rig_corpus::humanoid(Default::default(), true);
    let sb = builder.finish();
    let source = import::from_bytes(&sb, &Default::default())?;
    let (builder, _) = crate::rig_corpus::humanoid(
        crate::rig_corpus::Variant {
            proportions: true,
            ..Default::default()
        },
        false,
    );
    let tb = builder.finish();
    let target = import::from_bytes(&tb, &Default::default())?;
    let sh = library.import(&sb, "Profile source", &source)?;
    let th = library.import(&tb, "Profile target", &target)?;
    let mut settings = Settings::default();
    settings.contacts.looping = crate::contacts::LoopPolicy::Accumulate;
    let plan = retarget::compile(&source, &target, &settings, &[])?;
    let path = crate::profile::save(&managed, &plan, &source, &target)?;
    let saved = std::fs::read(&path)?;
    let mut e = Editor::new(library, context());
    e.input_regions = Some(InputRegions::default());
    let model = e
        .records
        .iter()
        .find(|r| r.hash == th && r.kind == Kind::Model)
        .unwrap()
        .clone();
    let clip = e
        .records
        .iter()
        .find(|r| r.hash == sh && matches!(r.kind, Kind::Clip(_)))
        .unwrap()
        .clone();
    e.request(&model, renderer, gpu);
    settle(&mut e, renderer, gpu)?;
    e.request(&clip, renderer, gpu);
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.plan_origin == "Saved profile"
            && e.retarget.as_ref().unwrap().settings.contacts.looping
                == crate::contacts::LoopPolicy::Accumulate,
        "Legacy Accumulate profile restored verbatim"
    );
    ensure!(
        e.contacts.as_ref().unwrap().settings.looping == crate::contacts::LoopPolicy::Repeat
            && e.contacts.as_ref().unwrap().cycle_displacement == [0.; 3],
        "Effective repeat preview overrides stored loop safely"
    );
    e.playback.cycle = 9;
    e.set_preview_placement(true);
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.playback.cycle == 0
            && e.contacts.as_ref().unwrap().settings.looping
                == crate::contacts::LoopPolicy::Accumulate,
        "Policy switch clears offset and recomputes contacts"
    );
    e.playback.cycle = -3;
    e.set_preview_placement(false);
    settle(&mut e, renderer, gpu)?;
    ensure!(
        e.playback.cycle == 0 && e.contacts.as_ref().unwrap().cycle_displacement == [0.; 3],
        "Switch back clears accumulated offsets"
    );
    ensure!(
        saved == std::fs::read(path)?,
        "Preview must not rewrite saved profile"
    );
    write_evidence("legacy-loop-profile.json",b"{\"result\":\"PASS\",\"stored_accumulate_restored\":true,\"effective_repeat\":true,\"switch_resets_cycle_and_recomputes\":true,\"profile_bytes_unchanged\":true}")?;
    Ok(())
}

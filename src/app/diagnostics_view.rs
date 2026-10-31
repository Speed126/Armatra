use super::*;
impl Editor {
    pub(super) fn diagnostic_text(&self, gpu: &Gpu) -> String {
        let mut out = format!(
            "ARMATRA {} / Animation workstation\nBuild: {}\nAdapter: {} / {:?}\nDriver: {}\nRequested DQS: {}\nLast completed render: {:?}\nModel: {:?}\nMotion: {:?}\nTime: {:.6}, cycle {}, speed {}, loop {}\nCPU: {} bytes / {} assets\nGPU estimate: {} bytes / {} assets\nMotion pose textures: {} bytes\nJobs: {}\n",
            env!("CARGO_PKG_VERSION"),
            option_env!("ARMATRA_BUILD_COMMIT").unwrap_or("unrecorded"),
            gpu.info.name,
            gpu.info.backend,
            format_args!("{} {}", gpu.info.driver, gpu.info.driver_info),
            self.dqs,
            self.skinning_report,
            self.model.as_ref().map(|m| &m.0),
            self.actual_motion_id(),
            self.playback.time,
            self.playback.cycle,
            self.playback.speed,
            self.playback.looping,
            self.cpu.bytes(),
            self.cpu.len(),
            self.gpu_cache.bytes(),
            self.gpu_cache.len(),
            self.motion.previews.bytes(),
            self.jobs
        );
        out.push_str(&format!(
            "Application log: {}\nLibrary preview: {:?}\nDisplayed previews: {}\n",
            crate::app_log::path()
                .map_or_else(|| "unavailable".into(), |p| p.display().to_string()),
            self.motion.preview_error,
            self.motion.displayed_count
        ));
        for (origin, messages) in [
            ("Render", Some(self.render_warnings.as_slice())),
            (
                "Target",
                self.model.as_ref().map(|m| m.1.warnings.as_slice()),
            ),
            (
                "Source",
                self.source.as_ref().map(|s| s.1.warnings.as_slice()),
            ),
        ] {
            if let Some(messages) = messages {
                for message in messages {
                    out.push_str(&format!("{origin}: {message}\n"));
                }
            }
        }
        out
    }
    pub(super) fn labs_ui(&mut self, ctx: &egui::Context, gpu: &Gpu) {
        let mut open = self.ui_state.labs;
        let area = ctx.content_rect().shrink(12.);
        let window=egui::Window::new("Labs / Diagnostics").fade_in(false).fade_out(false).default_pos(egui::pos2(420.,92.)).default_width(510.).max_width(area.width()).constrain_to(area).open(&mut open).show(ctx,|ui| {
            ui.spacing_mut().item_spacing.y=6.;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Skinning & runtime").font(theme::font(20.,500)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| {if widgets::glyph_button(ui,widgets::Icon::Diagnostics,"Copy report",false).clicked() {ctx.copy_text(self.diagnostic_text(gpu));}});
            });
            let scroll=super::labs::scroll_body(ui,"labs-body",(area.height()-130.).max(80.), |ui| {
            ui.add_space(8.);ui.separator();
            ui.horizontal(|ui| {
                ui.vertical(|ui| {ui.set_width(200.);ui.label(egui::RichText::new("REQUESTED").font(theme::font(11.,500)).color(theme::SECONDARY));ui.label(egui::RichText::new(if self.dqs {"Experimental DQS"}else{"Linear blend skinning"}).font(theme::font(15.,500)));});
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("LAST COMPLETED RENDER").font(theme::font(11.,500)).color(theme::SECONDARY));
                    let r=self.skinning_report;
                    let effective=if self.model.is_none() && self.source_preview.is_none() {"No model".into()}else if r.requested_dqs!=self.dqs {"Updating next frame...".into()}else if r.dqs>0&&r.lbs>0 {format!("{} DQS / {} LBS",r.dqs,r.lbs)}else if r.dqs>0 {format!("DQS / {} instances",r.dqs)}else if r.lbs>0 {format!("LBS / {} instances",r.lbs)}else if r.unskinned>0 {"Unskinned geometry".into()}else{"No visible mesh instances".into()};
                    ui.label(egui::RichText::new(effective).font(theme::font(15.,500)).color(if self.dqs&&r.lbs>0 {theme::WARNING}else{theme::TEXT}));
                });
            });
            widgets::toggle(ui,widgets::Icon::Skeleton,"Experimental DQS",&mut self.dqs).on_hover_text("Rigid skin frames only. Affine/non-rigid transforms retain the existing LBS fallback.");
            widgets::section(ui,"RUNTIME");
            egui::Grid::new("labs-runtime").num_columns(2).min_row_height(16.).spacing(egui::vec2(24.,6.)).show(ui,|ui| {
                for (label,value) in [("Frame",format!("{:.1} ms",self.frame_ms)),("CPU cache",format!("{:.2} MiB / {} assets",self.cpu.bytes() as f32/1048576.,self.cpu.len())),("GPU estimate",format!("{:.2} MiB / {} assets",self.gpu_cache.bytes() as f32/1048576.,self.gpu_cache.len())),("Motion pose textures",format!("{:.2} MiB / 16 MiB budget",self.motion.previews.bytes() as f32/1048576.)),("Adapter",gpu.info.name.clone()),("Backend / driver",format!("{:?} / {}",gpu.info.backend,gpu.info.driver_info)),("Engine load slots",format!("{} / 2",self.jobs))] {
                    ui.label(egui::RichText::new(label).font(theme::font(12.,400)).color(theme::SECONDARY));ui.label(egui::RichText::new(value).font(theme::font(12.,400)));ui.end_row();
                }
            });
            ui.add_space(8.);ui.separator();widgets::section(ui,"NOTICES");
            let mut notices=std::collections::BTreeMap::<(&str,&str),usize>::new();
            for (origin,messages) in [("Render",Some(self.render_warnings.as_slice())),("Target",self.model.as_ref().map(|m|m.1.warnings.as_slice())),("Source",self.source.as_ref().map(|s|s.1.warnings.as_slice()))] {if let Some(messages)=messages {for message in messages {*notices.entry((origin,message)).or_default()+=1;}}}
            if notices.is_empty() {ui.label(egui::RichText::new("No reported notices.").font(theme::font(12.,400)).color(theme::SUCCESS));}
            egui::ScrollArea::vertical().id_salt("labs-notices").max_height(120.).show(ui,|ui| {
                for ((origin,message),count) in notices {ui.horizontal_wrapped(|ui| {ui.label(egui::RichText::new(format!("{origin} / {count}x")).font(theme::font(11.,500)).color(theme::WARNING));ui.add(egui::Label::new(egui::RichText::new(message).font(theme::font(12.,400)).color(theme::WARNING)).selectable(true));});}
            });
            egui::CollapsingHeader::new("Technical details & limitations").show(ui,|ui| {
                ui.label("DQS supports rigid skin frames. Affine transforms, scale, reflection and non-rigid frames keep the existing linear blend fallback. Counts above come from the renderer's actual draw decisions, not the requested checkbox.");
                ui.label("Playback uses original curves and byte-LRU caches. GPU memory is an estimate, not process RSS. Motion similarity in Library is experimental and separate from retarget compatibility.");
                ui.label("Renderer and interface checks are available with --gpu-smoke and --ui-smoke. See README.md.");
                ui.add(egui::Label::new(self.diagnostic_text(gpu)).selectable(true));
            });
            });
            if let Some(r)=&mut self.input_regions {r.utility_scrolls.insert("labs",(scroll.inner_rect,scroll.state.clamped_offset().y));}
        });
        if let (Some(window), Some(r)) = (window, &mut self.input_regions) {
            r.utility_windows.insert("labs", window.response.rect);
        }
        self.ui_state.labs = open;
    }
    pub(super) fn icon_qa(&self, ctx: &egui::Context) {
        use widgets::Icon::*;
        let icons = [
            Animate,
            Assets,
            Automatic,
            Check,
            Chevron,
            Close,
            Contact,
            Diagnostics,
            Frame,
            Ik,
            Inspector,
            Joint,
            Library,
            Link,
            Loop,
            Manual,
            Mapping,
            Mesh,
            Minus,
            Orbit,
            Pause,
            Play,
            Plus,
            Retarget,
            Reverse,
            Root,
            Save,
            Search,
            Skeleton,
            Start,
        ];
        egui::Window::new("Interface vector / type diagnostics").fade_in(false).fade_out(false).fixed_pos(egui::pos2(64.,60.)).resizable(false).show(ctx,|ui| {
            ui.label(egui::RichText::new("Original vectors / anonymous normal + active pairs").font(theme::font(18.,500)));
            ui.label(egui::RichText::new("16 / 20 / 24 logical points. Optical variants at 16 and 20. Index key is recorded in the diagnostic session.").font(theme::font(11.,400)).color(theme::SECONDARY));
            for size in [16.,20.,24.] {
                ui.label(egui::RichText::new(format!("{size:.0} PT")).font(theme::font(11.,500)).color(theme::SECONDARY));
                egui::Grid::new(format!("icon-grid-{size}")).num_columns(10).spacing(egui::vec2(8.,4.)).show(ui,|ui| {
                    for (i,icon) in icons.iter().enumerate() {
                        let (rect,_)=ui.allocate_exact_size(egui::vec2(64.,30.),egui::Sense::hover());
                        for (column,color) in [(0.,theme::SECONDARY),(1.,theme::ORANGE)] {crate::ui::icons::paint(ui.painter(),egui::Rect::from_center_size(rect.left_center()+egui::vec2(16.+32.*column,0.),egui::vec2(size,size)),*icon,color);}
                        if (i+1)%10==0 {ui.end_row();}
                    }
                });
            }
            ui.horizontal(|ui| {for weight in [400,500,600] {ui.label(egui::RichText::new(format!("Armatra UI {weight} / UpperArm.L  00.433")).font(theme::font(12.,weight)));}});
        });
    }
}

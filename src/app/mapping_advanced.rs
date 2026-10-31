use super::*;
use crate::retarget::RootPolicy;
pub(super) fn skin_selector(
    ui: &mut egui::Ui,
    asset: &Asset,
    selected: &mut Option<usize>,
    side: usize,
    mut regions: Option<&mut InputRegions>,
) -> bool {
    let before = *selected;
    let name = if side == 0 { "Source" } else { "Target" };
    if asset.skins.is_empty() {
        ui.add(egui::Label::new(egui::RichText::new("Animated node hierarchy · No skin").size(11.)).truncate()).on_hover_text("The supplied animated hierarchy provides rest-pose context; no skin index is required.");
        return false;
    }
    let label = |i: usize| {
        let skin = &asset.skins[i];
        let node = skin
            .joints
            .iter()
            .copied()
            .find(|&n| crate::rig::semantics(&asset.nodes[n].name).0 == crate::rig::Anatomy::Pelvis)
            .or_else(|| skin.joints.first().copied());
        format!(
            "{name} skin {i} · {} joints · {}",
            skin.joints.len(),
            node.map_or("Empty", |n| asset.nodes[n].name.as_str())
        )
    };
    let text = selected
        .or((asset.skins.len() == 1).then_some(0))
        .filter(|&i| i < asset.skins.len())
        .map_or_else(
            || format!("Select {name} skin ({} skins)", asset.skins.len()),
            label,
        );
    let response = egui::ComboBox::from_id_salt(("rig-skin", side))
        .truncate()
        .width(ui.available_width())
        .selected_text(&text)
        .show_ui(ui, |ui| {
            let b = ui.selectable_value(
                selected,
                None,
                if asset.skins.len() == 1 {
                    "Automatic · skin 0"
                } else {
                    "Select a skin…"
                },
            );
            if let Some(r) = &mut regions {
                r.skin_options.insert((side, None), b.rect);
            }
            for i in 0..asset.skins.len() {
                let b = ui.selectable_value(selected, Some(i), label(i));
                if let Some(r) = &mut regions {
                    r.skin_options.insert((side, Some(i)), b.rect);
                }
            }
        })
        .response
        .on_hover_text(&text);
    if let Some(r) = regions {
        r.mapping_controls.insert(
            if side == 0 {
                "source-skin"
            } else {
                "target-skin"
            },
            response.rect,
        );
    }
    before != *selected
}
impl Editor {
    pub(super) fn mapping_advanced(&mut self, ui: &mut egui::Ui, source: &Asset, target: &Asset) {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("source-skin")
                .selected_text(
                    self.mapping_settings
                        .source_skin
                        .map_or("Source: auto".into(), |s| format!("Source skin {s}")),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.mapping_settings.source_skin,
                        None,
                        "Auto / single rig",
                    );
                    for i in 0..source.skins.len() {
                        ui.selectable_value(
                            &mut self.mapping_settings.source_skin,
                            Some(i),
                            format!("Skin {i}: {} joints", source.skins[i].joints.len()),
                        );
                    }
                });
            egui::ComboBox::from_id_salt("target-skin")
                .selected_text(
                    self.mapping_settings
                        .target_skin
                        .map_or("Target: auto".into(), |s| format!("Target skin {s}")),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.mapping_settings.target_skin,
                        None,
                        "Auto / single rig",
                    );
                    for i in 0..target.skins.len() {
                        ui.selectable_value(
                            &mut self.mapping_settings.target_skin,
                            Some(i),
                            format!("Skin {i}: {} joints", target.skins[i].joints.len()),
                        );
                    }
                });
            egui::ComboBox::from_id_salt("root-policy")
                .selected_text(format!("Root: {:?}", self.mapping_settings.root))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.mapping_settings.root,
                        RootPolicy::ScaleHeight,
                        "Scale by height",
                    );
                    ui.selectable_value(
                        &mut self.mapping_settings.root,
                        RootPolicy::Unscaled,
                        "Unscaled",
                    );
                    ui.selectable_value(
                        &mut self.mapping_settings.root,
                        RootPolicy::InPlace,
                        "In place",
                    );
                });
        });
        let mut explicit = self.mapping_settings.alignment_override.is_some();
        if ui
            .checkbox(
                &mut explicit,
                "Use explicit scene-axis alignment (for unknown orientation)",
            )
            .changed()
        {
            self.mapping_settings.alignment_override = explicit.then_some([0., 0., 0., 1.]);
        }
        if let Some(q) = &mut self.mapping_settings.alignment_override {
            ui.horizontal_wrapped(|ui| {
                ui.label("Source → target quaternion (x y z w)");
                for v in q.iter_mut() {
                    ui.add(egui::DragValue::new(v).speed(0.01));
                }
                if ui.button("Normalize").clicked() {
                    let quat = glam::Quat::from_array(*q);
                    if quat.is_finite() && quat.length_squared() > 1e-8 {
                        *q = quat.normalize().to_array();
                    }
                }
            });
        }
        egui::CollapsingHeader::new("Contacts and ground policy").show(ui,|ui|{
                use crate::contacts::{Ground,LoopPolicy};
                let c=&mut self.mapping_settings.contacts;
                let mut ground=match c.ground {Ground::Disabled=>0,Ground::RestFeet=>1,Ground::Plane{..}=>2};let old=ground;
                egui::ComboBox::from_id_salt("ground-policy").selected_text(["Disabled","Rest foot-joint plane","Explicit plane"][ground]).show_ui(ui,|ui|{ui.selectable_value(&mut ground,0,"Disabled");ui.selectable_value(&mut ground,1,"Rest foot-joint plane (requires known frames)");ui.selectable_value(&mut ground,2,"Explicit plane");});
                if ground!=old {c.ground=match ground {0=>Ground::Disabled,1=>Ground::RestFeet,_=>Ground::Plane{source_normal:[0.,1.,0.],source_height:0.,target_height:0.}};}
                if let Ground::Plane{source_normal,source_height,target_height}=&mut c.ground {
                    ui.horizontal_wrapped(|ui|{ui.label("Source normal");for v in source_normal {ui.add(egui::DragValue::new(v).speed(0.01));}});
                    ui.horizontal_wrapped(|ui|{ui.label("Source / target foot-joint heights");ui.add(egui::DragValue::new(source_height).speed(0.01));ui.add(egui::DragValue::new(target_height).speed(0.01));});
                }
                egui::ComboBox::from_id_salt("contact-loop").selected_text(format!("Stored profile loop: {:?}",c.looping)).show_ui(ui,|ui|{ui.selectable_value(&mut c.looping,LoopPolicy::Accumulate,"Accumulate locomotion displacement");ui.selectable_value(&mut c.looping,LoopPolicy::Repeat,"Repeat at origin each cycle");});
                ui.label("Workstation preview uses the placement selected in the transport / Settings. The stored profile policy is preserved separately.");
                ui.horizontal_wrapped(|ui|{ui.label("Enter / exit speed (heights per second)");ui.add(egui::DragValue::new(&mut c.enter_speed).speed(0.005).range(0.0..=2.));ui.add(egui::DragValue::new(&mut c.exit_speed).speed(0.005).range(0.0..=2.));});
                ui.horizontal_wrapped(|ui|{ui.label("Enter / exit height (character heights)");ui.add(egui::DragValue::new(&mut c.enter_height).speed(0.002).range(0.0..=0.5));ui.add(egui::DragValue::new(&mut c.exit_height).speed(0.002).range(0.0..=0.5));});
                ui.horizontal_wrapped(|ui|{ui.label("Transition seconds");ui.add(egui::DragValue::new(&mut c.transition).speed(0.01).range(0.01..=1.));ui.checkbox(&mut c.project_ground,"Project target anchors to plane");});
                ui.label("Plane describes foot-joint reference height, not mesh soles. Compile / preview applies settings.");
            });
        egui::CollapsingHeader::new("Pelvis correction limits").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Stiffness");
                ui.add(
                    egui::DragValue::new(&mut self.mapping_settings.ik.pelvis_stiffness)
                        .speed(0.05)
                        .range(0.0..=10.),
                );
                ui.label("Maximum displacement (character heights)");
                ui.add(
                    egui::DragValue::new(&mut self.mapping_settings.ik.max_pelvis_height)
                        .speed(0.005)
                        .range(0.0..=0.25),
                );
            });
        });
    }
}

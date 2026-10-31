use super::*;
use crate::{
    rig::Rig,
    ui::{
        correspondence::{Hit, Operation},
        icons::Icon,
    },
};

#[derive(Default)]
pub(super) struct History {
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    context: Option<(String, String, Option<usize>, Option<usize>)>,
}
struct Edit {
    before: Vec<Override>,
    after: Vec<Override>,
    selected_before: Option<usize>,
    selected_after: Option<usize>,
}
impl Editor {
    pub(super) fn mapping_dirty(&self) -> bool {
        self.retarget.as_ref().is_none_or(|p| {
            p.settings != self.mapping_settings || p.overrides != self.mapping_overrides
        })
    }
    pub(super) fn mapping_evidence(&self) -> serde_json::Value {
        let operation = match &self.ui_state.correspondence.operation {
            Operation::Idle => serde_json::json!({"state":"idle"}),
            Operation::Drag {
                source,
                target,
                active,
                pointer,
                ..
            } => {
                serde_json::json!({"state":"drag","source":source,"target":target,"active":active,"pointer":[pointer.x,pointer.y]})
            }
            Operation::Selecting(s) => {
                serde_json::json!({"state":"confirmation","source":s.source,"target":s.target,"awaiting":s.awaiting,"replace":s.replace})
            }
        };
        serde_json::json!({"operation":operation,"undo":self.mapping_history.undo.len(),"redo":self.mapping_history.redo.len()})
    }
    pub(super) fn mapping_pending(&self, source: usize) -> bool {
        self.retarget.as_ref().is_none_or(|p| {
            p.settings != self.mapping_settings
                || p.overrides.iter().find(|o| o.source == source)
                    != self.mapping_overrides.iter().find(|o| o.source == source)
        })
    }
    pub(super) fn mapping_context(&mut self) {
        let key = self.source.as_ref().zip(self.model.as_ref()).map(|(s, t)| {
            (
                s.0.clone(),
                t.0.clone(),
                self.mapping_settings.source_skin,
                self.mapping_settings.target_skin,
            )
        });
        if self.mapping_history.context != key {
            self.mapping_history = History {
                context: key,
                ..Default::default()
            };
            self.ui_state.correspondence.operation = Operation::Idle;
        }
    }
    fn valid_mapping(&self, source: usize, target: Option<usize>) -> Result<(), String> {
        if self.plan_pending {
            return Err("Wait for the current preview compilation.".into());
        }
        let (Some((_, s, _)), Some((_, t, _))) = (&self.source, &self.model) else {
            return Err("Choose a source and target first.".into());
        };
        if !Rig::analyze_skin(s, self.mapping_settings.source_skin)
            .joints
            .iter()
            .any(|j| j.node == source)
            || target.is_some_and(|n| {
                !Rig::analyze_skin(t, self.mapping_settings.target_skin)
                    .joints
                    .iter()
                    .any(|j| j.node == n)
            })
        {
            return Err("Select joints from the current source and target skins.".into());
        }
        Ok(())
    }
    pub(super) fn edit_overrides(&mut self, after: Vec<Override>, selected: Option<usize>) -> bool {
        if self.plan_pending || after == self.mapping_overrides {
            return false;
        }
        let mut sources = std::collections::HashSet::new();
        let mut targets = std::collections::HashSet::new();
        for o in &after {
            if self.valid_mapping(o.source, o.target).is_err()
                || !sources.insert(o.source)
                || o.target.is_some_and(|n| !targets.insert(n))
            {
                self.status =
                    "Mapping edit rejected: joint IDs and manual targets must be valid and unique."
                        .into();
                return false;
            }
        }
        self.mapping_history.undo.push(Edit {
            before: self.mapping_overrides.clone(),
            after: after.clone(),
            selected_before: self.selected_pair,
            selected_after: selected,
        });
        if self.mapping_history.undo.len() > 128 {
            self.mapping_history.undo.remove(0);
        }
        self.mapping_history.redo.clear();
        self.mapping_overrides = after;
        self.selected_pair = selected;
        self.status = if self.mapping_dirty() {
            "Manual mapping · Pending apply. Apply & Preview updates animation; Save Mapping persists it."
        } else {
            "Correspondence restored · Matches the current preview."
        }
        .into();
        if self.mapping_dirty() {
            self.pending_mapping();
        } else {
            self.mapping_message(
                "Applied",
                "Correspondence matches the active preview.".into(),
            );
        }
        self.ctx
            .request_discard("Show confirmed mapping edit immediately");
        true
    }
    pub(super) fn assign_joint(&mut self, source: usize, target: Option<usize>) -> bool {
        self.commit_joint(source, target, false)
    }
    fn commit_joint(&mut self, source: usize, target: Option<usize>, replace: bool) -> bool {
        if let Err(reason) = self.valid_mapping(source, target) {
            self.status = reason;
            return false;
        }
        let conflict = self
            .mapping_overrides
            .iter()
            .find(|o| o.source != source && target.is_some() && o.target == target)
            .map(|o| o.source);
        if conflict.is_some() && !replace {
            self.status = "Target joint already mapped. Review and confirm the replacement.".into();
            return false;
        }
        if conflict.is_none()
            && self
                .mapping_overrides
                .iter()
                .any(|o| o.source == source && o.target == target)
        {
            self.selected_pair = Some(source);
            return false;
        }
        let mut next = self.mapping_overrides.clone();
        if let Some(old) = conflict {
            for o in &mut next {
                if o.source == old {
                    o.target = None;
                }
            }
        }
        next.retain(|o| o.source != source);
        next.push(Override { source, target });
        self.edit_overrides(next, Some(source))
    }
    pub(super) fn restore_auto(&mut self, source: usize) {
        let mut next = self.mapping_overrides.clone();
        next.retain(|o| o.source != source);
        if self.edit_overrides(next, Some(source)) {
            self.status = if self.mapping_dirty() {
                "Automatic mapping restored · Pending apply. Apply & Preview updates animation."
            } else {
                "Automatic mapping restored · Matches the current preview."
            }
            .into();
            if self.mapping_dirty() {
                self.pending_mapping();
            } else {
                self.mapping_message(
                    "Applied",
                    "Mapping history matches the active preview.".into(),
                );
            }
        }
    }
    pub(super) fn undo_mapping(&mut self, redo: bool) {
        if self.plan_pending {
            return;
        }
        let edit = if redo {
            self.mapping_history.redo.pop()
        } else {
            self.mapping_history.undo.pop()
        };
        if let Some(edit) = edit {
            self.mapping_overrides = if redo {
                edit.after.clone()
            } else {
                edit.before.clone()
            };
            self.selected_pair = if redo {
                edit.selected_after
            } else {
                edit.selected_before
            };
            if redo {
                self.mapping_history.undo.push(edit);
            } else {
                self.mapping_history.redo.push(edit);
            }
            self.ctx
                .request_discard("Show mapping undo or redo immediately");
            self.ui_state.correspondence.operation = Operation::Idle;
            self.status = if self.mapping_dirty() {
                "Mapping history restored · Pending apply. Apply & Preview updates animation."
            } else {
                "Mapping history restored · Matches the current preview."
            }
            .into();
        }
    }
    pub(super) fn mapping_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.current_pass_index() != 0
            || ctx.text_edit_focused()
            || egui::Popup::is_any_open(ctx)
            || self.ui_state.help
            || self.ui_state.settings
            || self.ui_state.labs
            || self.ui_state.mapping_advanced
        {
            return;
        }
        let (open, cancel, undo, redo) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::M) && !i.modifiers.command && !i.modifiers.alt,
                i.key_pressed(egui::Key::Escape),
                i.modifiers.command && i.key_pressed(egui::Key::Z) && !i.modifiers.shift,
                i.modifiers.command
                    && (i.key_pressed(egui::Key::Y)
                        || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
            )
        });
        if cancel {
            self.ui_state.correspondence.operation = Operation::Idle;
            self.ui_state.correspondence.navigation = None;
        }
        if open {
            self.ui_state.correspondence.operation.begin(None, None);
        }
        if undo || redo {
            self.undo_mapping(redo);
        }
    }
    pub(super) fn mapping_actions(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(!self.plan_pending,|ui|{
            let map=widgets::glyph_button(ui,Icon::MapJoints,"Map Joints",matches!(self.ui_state.correspondence.operation,Operation::Selecting(_))).on_hover_text("Create a correspondence · M. Select source and target, then confirm. Middle-drag or Shift + left-drag a source joint also works.");
            if let Some(r)=&mut self.input_regions {r.mapping_reassign=map.rect;r.mapping_controls.insert("map",map.rect);}
            if map.clicked(){self.ui_state.correspondence.operation.begin(None,None);}
            for (name,icon,redo,tip) in [("undo",Icon::Undo,false,"Undo mapping edit · Ctrl+Z"),("redo",Icon::Redo,true,"Redo mapping edit · Ctrl+Y / Ctrl+Shift+Z")] {
                let enabled=if redo {!self.mapping_history.redo.is_empty()}else{!self.mapping_history.undo.is_empty()};
                let button=ui.add_enabled_ui(enabled,|ui|widgets::icon(ui,icon,false,tip)).inner;
                if let Some(r)=&mut self.input_regions {r.mapping_controls.insert(name,button.rect);}
                if button.clicked() && ui.ctx().current_pass_index()==0 {self.undo_mapping(redo);}
            }
        });
    }
    pub(super) fn mapping_panel(
        &mut self,
        ctx: &egui::Context,
        detail: egui::Rect,
        rigs: [&Rig; 2],
        assets: [&Asset; 2],
    ) {
        let Operation::Selecting(mut selection) = self.ui_state.correspondence.operation.clone()
        else {
            return;
        };
        let area = ctx.content_rect().shrink(10.);
        let width = (detail.width() - 32.)
            .clamp(210., 300.)
            .min(area.width() - 16.);
        let mut cancel = false;
        let mut confirm = false;
        let conflict = selection
            .source
            .zip(selection.target)
            .and_then(|(s, t)| {
                self.mapping_overrides
                    .iter()
                    .find(|o| o.source != s && o.target == Some(t))
            })
            .map(|o| o.source);
        let valid = selection
            .source
            .zip(selection.target)
            .map(|(s, t)| self.valid_mapping(s, Some(t)));
        let panel=egui::Window::new("Create correspondence").id(egui::Id::unique("mapping-confirmation"))
            .title_bar(false).resizable(false).collapsible(false).fade_in(false).fade_out(false)
            .fixed_pos(detail.min+egui::vec2(8.,8.)).default_width(width).max_width(width).constrain_to(area)
            .frame(egui::Frame::window(&ctx.style_of(egui::Theme::Dark)).fill(theme::PANEL).stroke(egui::Stroke::new(1.,theme::ORANGE.gamma_multiply(0.65))).corner_radius(4.).inner_margin(12.))
            .show(ctx,|ui|{
                ui.set_width(width-24.);
                ui.horizontal(|ui|{
                    let (r,_)=ui.allocate_exact_size(egui::vec2(20.,20.),egui::Sense::hover());widgets::paint_icon(ui,r,Icon::MapJoints,theme::ORANGE);
                    let heading=ui.add(egui::Label::new(egui::RichText::new("Create correspondence").font(theme::font(15.,500))).sense(egui::Sense::click()));
                    if heading.clicked(){heading.request_focus();}
                    if heading.has_focus(){ui.painter().rect_stroke(heading.rect.expand(2.),2.,egui::Stroke::new(1.,theme::ORANGE),egui::StrokeKind::Inside);if ui.input(|i|i.key_pressed(egui::Key::Enter)){confirm=true;}}
                });
                ui.add_space(4.);
                ui.label(egui::RichText::new(match selection.awaiting {Some(0)=>"Select a source joint",Some(_)=>"Select a target joint",None=>"Review the proposed connection"}).size(11.).color(theme::SECONDARY));
                let body=egui::ScrollArea::vertical().id_salt("mapping-confirmation-body").max_height((area.height()-200.).max(100.)).show(ui,|ui|{
                    for (side,asset) in assets.iter().enumerate() {
                        ui.add_space(8.);
                        ui.label(egui::RichText::new(if side==0 {"SOURCE JOINT"}else{"TARGET JOINT"}).font(theme::font(10.,500)).color(if selection.awaiting==Some(side){theme::ORANGE}else{theme::SECONDARY}));
                        let selected=if side==0 {selection.source}else{selection.target};
                        let name=selected.filter(|&n|n<asset.nodes.len()).map(|n|asset.nodes[n].name.as_str());
                        let placeholder=if side==0 {"Select a joint from the source skeleton"}else{"Select a joint from the target skeleton"};
                        ui.horizontal(|ui|{
                            let response=ui.add_sized(egui::vec2(ui.available_width()-36.,48.),egui::Button::new(egui::RichText::new(name.unwrap_or(placeholder)).size(12.)).wrap_mode(if name.is_some(){egui::TextWrapMode::Truncate}else{egui::TextWrapMode::Wrap}).fill(theme::ELEVATED)).on_hover_text(name.unwrap_or(placeholder));
                            response.widget_info(||egui::WidgetInfo::labeled(egui::Role::Button,ui.is_enabled(),format!("{} joint: {}{}",if side==0{"Source"}else{"Target"},name.unwrap_or(placeholder),selected.map_or(String::new(),|node|format!(" · Node {node}")))));
                            if selection.awaiting==Some(side)||response.has_focus(){ui.painter().rect_stroke(response.rect,3.,egui::Stroke::new(1.,theme::ORANGE),egui::StrokeKind::Inside);}
                            if selection.focus && side==selection.awaiting.unwrap_or(0) {response.request_focus();selection.focus=false;}
                            if response.clicked(){selection.awaiting=Some(side);selection.picker=None;}
                            if response.has_focus() && !ctx.text_edit_focused() && ui.input(|i|i.key_pressed(egui::Key::Enter)) {confirm=true;}
                            if let Some(r)=&mut self.input_regions {r.mapping_controls.insert(if side==0 {"source-field"}else{"target-field"},response.rect);}
                            let search=widgets::icon(ui,Icon::Search,selection.picker==Some(side),if side==0 {"Search source joints by name or node ID"}else{"Search target joints by name or node ID"});
                            if search.clicked(){selection.picker=if selection.picker==Some(side){None}else{Some(side)};selection.awaiting=Some(side);}
                            if let Some(r)=&mut self.input_regions {r.mapping_controls.insert(if side==0 {"source-picker"}else{"target-picker"},search.rect);}
                        });
                        ui.label(egui::RichText::new(selected.map_or_else(||"Choose on canvas or search".into(),|node|format!("Node {node}"))).monospace().size(10.).color(theme::SECONDARY));
                    }
                    if let Some(side)=selection.picker {
                        ui.add_space(8.);
                        let focused=ctx.memory(|m|m.focused());
                        let response=widgets::search(ui,&mut selection.filters[side],"Name or node ID…");
                        if (response.has_focus()||focused==Some(response.id)) && ui.input(|i|i.key_pressed(egui::Key::Escape)) {cancel=true;}
                        if let Some(r)=&mut self.input_regions {r.mapping_controls.insert("picker-search",response.rect);}
                        let filter=selection.filters[side].to_lowercase();
                        let picker=egui::ScrollArea::vertical().id_salt(("joint-picker",side)).max_height(120.).show(ui,|ui|{
                            for joint in &rigs[side].joints {
                                let name=&assets[side].nodes[joint.node].name;
                                if !filter.is_empty()&&!name.to_lowercase().contains(&filter)&&!joint.node.to_string().contains(&filter){continue;}
                                let response=ui.selectable_label(false,format!("{name} · #{}",joint.node)).on_hover_text(name);
                                if let Some(r)=&mut self.input_regions {r.mapping_picker_options.insert((side,joint.node),response.rect);}
                                if response.clicked(){let mut op=Operation::Selecting(selection.clone());op.choose(Hit{side,node:joint.node});if let Operation::Selecting(next)=op {selection=next;}}
                            }
                        });
                        if let Some(r)=&mut self.input_regions {r.mapping_picker_clip=Some(picker.inner_rect);}
                    }
                    if let Some(old)=conflict {
                        ui.add_space(10.);ui.colored_label(theme::WARNING,"Target joint already mapped");
                        let target=selection.target.unwrap();let source=selection.source.unwrap();
                        ui.label(egui::RichText::new(format!("{} is currently mapped from {}.",assets[1].nodes[target].name,assets[0].nodes[old].name)).size(11.));
                        ui.label(egui::RichText::new(format!("Proposed replacement:\n{} → {}",assets[0].nodes[source].name,assets[1].nodes[target].name)).size(11.));
                        let response=ui.checkbox(&mut selection.replace,"Replace existing manual assignment").on_hover_text("The previous source becomes explicitly unmapped. Undo restores both overrides together.");
                        if let Some(r)=&mut self.input_regions {r.mapping_controls.insert("replace",response.rect);}
                    } else if selection.target.is_some_and(|target|self.retarget.as_ref().is_some_and(|p|p.rows.iter().any(|r|r.source!=selection.source.unwrap_or(usize::MAX)&&r.target==Some(target)))) {
                        ui.add_space(8.);ui.label(egui::RichText::new("Manual overrides take precedence when Apply & Preview compiles. Other automatic suggestions may change.").size(11.).color(theme::SECONDARY));
                    }
                    if let Some(Err(reason))=&valid {ui.colored_label(theme::WARNING,reason);}
                });
                if body.content_size.y>body.inner_rect.height()+1. {ui.label(egui::RichText::new("Scroll to review all options").size(10.).color(theme::WARNING));}
                ui.add_space(8.);ui.separator();
                ui.label(egui::RichText::new("Creates a pending edit. Apply & Preview updates animation.").size(10.).color(theme::SECONDARY));
                ui.horizontal(|ui|{
                    ui.spacing_mut().button_padding.x=8.;
                    let response=ui.button("Cancel").on_hover_text("Cancel without changing mapping · Escape");cancel|=response.clicked();
                    if let Some(r)=&mut self.input_regions {r.mapping_controls.insert("cancel",response.rect);}
                    let enabled=matches!(valid,Some(Ok(())))&&(conflict.is_none()||selection.replace);
                    let response=ui.add_enabled_ui(enabled,|ui|widgets::primary(ui,"Confirm mapping")).inner.on_hover_text("Create this manual override. Preview and saved profile change only when explicitly applied or saved.");
                    confirm|=response.clicked();
                    if let Some(r)=&mut self.input_regions {r.mapping_controls.insert("confirm",response.rect);r.mapping_confirm_enabled=enabled;}
                    confirm &= enabled;
                });
                if let Some(r)=&mut self.input_regions {r.utility_scrolls.insert("mapping-confirmation",(body.inner_rect,body.state.clamped_offset().y));}
            });
        if let (Some(panel), Some(r)) = (panel, &mut self.input_regions) {
            r.utility_windows
                .insert("mapping-confirmation", panel.response.rect);
        }
        if cancel {
            self.ui_state.correspondence.operation = Operation::Idle;
        } else if confirm {
            self.commit_joint(
                selection.source.unwrap(),
                selection.target,
                selection.replace,
            );
            self.ui_state.correspondence.operation = Operation::Idle;
        } else {
            self.ui_state.correspondence.operation = Operation::Selecting(selection);
        }
    }
}

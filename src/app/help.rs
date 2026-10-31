use super::*;
use crate::ui::icons::Icon;

/// Actual runtime icons, shared by the reference and its accessibility checks.
type IconEntry = (Icon, &'static str, &'static str);
type LegendGroup = (&'static str, &'static [IconEntry]);
pub(super) const LEGEND: &[LegendGroup] = &[
    (
        "RETARGET",
        &[
            (
                Icon::MapJoints,
                "Map Joints",
                "Create a source-to-target joint correspondence. Review the names, then confirm.",
            ),
            (
                Icon::Unmap,
                "Unmap",
                "Explicitly leave the selected source joint unmapped.",
            ),
            (
                Icon::RestoreAuto,
                "Restore Auto",
                "Remove the manual override and return to automatic mapping.",
            ),
            (
                Icon::Undo,
                "Undo",
                "Revert one mapping edit, including every override affected by a replacement.",
            ),
            (Icon::Redo, "Redo", "Reapply the last undone mapping edit."),
            (
                Icon::Check,
                "Apply & Preview",
                "Apply pending edits and open Animate. More → Apply only keeps Retarget open.",
            ),
            (
                Icon::Save,
                "Save Mapping",
                "Compile and persist the mapping profile for reuse.",
            ),
            (
                Icon::Link,
                "Linked View",
                "Synchronize skeleton orientation; each side keeps its own zoom and pan.",
            ),
            (
                Icon::Frame,
                "Frame Both",
                "Fit the two skeletons independently. Double-click one side to fit only that side.",
            ),
            (
                Icon::Plus,
                "Zoom in",
                "Enlarge both correspondence views. Scroll over one side for individual zoom.",
            ),
            (Icon::Minus, "Zoom out", "Reduce both correspondence views."),
        ],
    ),
    (
        "ANIMATE & LIBRARY",
        &[
            (Icon::Play, "Play", "Start playback or a motion preview."),
            (Icon::Pause, "Pause", "Pause at the current animation time."),
            (
                Icon::Start,
                "Jump to start",
                "Return to the beginning and clear completed-cycle placement.",
            ),
            (
                Icon::Loop,
                "Loop",
                "Toggle repeated playback. Placement is selected separately in the transport.",
            ),
            (
                Icon::Frame,
                "Frame Character",
                "Fit the current character pose or original-source preview. Also available with F.",
            ),
            (Icon::Mesh, "Mesh", "Show the rendered mesh."),
            (
                Icon::Skeleton,
                "Skeleton",
                "Show skeleton joints and bones.",
            ),
            (
                Icon::Ik,
                "Pelvis & foot IK",
                "Toggle contact-aware correction on a compiled retarget preview.",
            ),
            (
                Icon::Contact,
                "Foot anchors",
                "Show detected contact anchors.",
            ),
            (
                Icon::Root,
                "Root motion",
                "Inspect the compiled root-translation policy.",
            ),
            (
                Icon::Search,
                "Search / joint picker",
                "Filter library records or choose a correspondence joint by name or original node ID.",
            ),
        ],
    ),
    (
        "WORKSPACE",
        &[
            (
                Icon::Animate,
                "Animate",
                "Inspect animation, root placement and the contact timeline.",
            ),
            (
                Icon::Retarget,
                "Retarget",
                "Review and edit source-to-target correspondence.",
            ),
            (
                Icon::Library,
                "Library",
                "Browse imported motions and characters independently of similarity eligibility.",
            ),
            (
                Icon::Assets,
                "Assets",
                "Open the compact asset collection from the workspace toolbar.",
            ),
            (
                Icon::Inspector,
                "Detail",
                "Open the current selection's contextual detail drawer from the workspace toolbar.",
            ),
            (
                Icon::Settings,
                "Settings",
                "Configure persisted appearance, playback and workspace preferences.",
            ),
            (
                Icon::Labs,
                "Labs / Diagnostics",
                "Inspect runtime diagnostics and copy a report.",
            ),
            (Icon::Close, "Close", "Dismiss the current panel or drawer."),
        ],
    ),
];

pub(super) fn contents(ui: &mut egui::Ui) {
    ui.add_space(8.);
    widgets::section(ui, "GETTING STARTED");
    ui.label("Import a character and motion, preview it in Library, then choose the source and target in Retarget. Confirm your mapping edits and use Apply & Preview to inspect the result in Animate. Save Mapping keeps the profile for reuse.");
    for (group, rows) in [
        (
            "ANIMATE CONTROLS",
            vec![
                ("Play / pause", vec!["Space"]),
                ("Jump to start", vec!["Home"]),
                ("Seek", vec!["Click / drag timeline"]),
            ],
        ),
        (
            "RETARGET CONTROLS",
            vec![
                ("Map Joints", vec!["M"]),
                ("Drag to connect", vec!["Middle-drag source joint"]),
                ("Alternate connection gesture", vec!["Shift", "Left drag"]),
                ("Cancel proposal", vec!["Escape"]),
                ("Confirm focused panel", vec!["Enter"]),
                ("Navigate panel", vec!["Tab / Shift+Tab"]),
                ("Undo mapping edit", vec!["Ctrl / Cmd", "Z"]),
                ("Redo mapping edit", vec!["Ctrl / Cmd", "Y"]),
                ("Redo alternate", vec!["Ctrl / Cmd", "Shift", "Z"]),
                ("Navigate focused mapping list", vec!["Up / Down"]),
            ],
        ),
        (
            "LIBRARY CONTROLS",
            vec![
                ("Select motion", vec!["Click row"]),
                ("Preview representative pose", vec!["Click pose"]),
            ],
        ),
        (
            "CAMERA NAVIGATION",
            vec![
                ("Frame character", vec!["F"]),
                ("Orbit skeletons", vec!["Left-drag background"]),
                ("Pan", vec!["Right drag"]),
                ("Pan empty canvas", vec!["Middle drag"]),
                ("Zoom one view", vec!["Scroll"]),
                ("Fit one skeleton", vec!["Double-click"]),
            ],
        ),
        (
            "GENERAL SHORTCUTS",
            vec![
                ("Import asset", vec!["Ctrl / Cmd", "O"]),
                ("Dismiss compact drawer", vec!["Escape"]),
            ],
        ),
    ] {
        ui.add_space(8.);
        widgets::section(ui, group);
        if group == "RETARGET CONTROLS" {
            ui.label(egui::RichText::new("Middle-drag source → target and release, or click Map Joints and select each side separately. Both open the same confirmation. A confirmed edit stays pending until Apply & Preview. Search buttons offer precise joint selection.").size(12.).color(theme::SECONDARY));
            ui.add_space(8.);
        }
        egui::Grid::new(group)
            .num_columns(2)
            .min_col_width(205.)
            .max_col_width(205.)
            .spacing(egui::vec2(22., 8.))
            .show(ui, |ui| {
                for (action, keys) in rows {
                    super::labs::shortcut(ui, action, &keys);
                }
            });
    }
    ui.add_space(10.);
    ui.label(egui::RichText::new("Shortcuts pause while editing text or numbers and while a picker is open. Enter confirms only from the correspondence panel. Repeat at origin is the default playback placement.").size(12.).color(theme::SECONDARY));
    ui.add_space(16.);
    let heading = ui.label(egui::RichText::new("Interface & Icons").font(theme::font(20., 500)));
    if std::env::var_os("ARMATRA_CAPTURE_HELP_ICONS").is_some()
        && std::env::var_os("ARMATRA_CAPTURE").is_some()
    {
        heading
            .scroll_to_me_animation(Some(egui::Align::TOP), egui::style::ScrollAnimation::none());
    }
    ui.label(
        egui::RichText::new("The same vector artwork used throughout Armatra.")
            .size(12.)
            .color(theme::SECONDARY),
    );
    for (group, rows) in LEGEND {
        widgets::section(ui, group);
        for (icon, name, description) in *rows {
            ui.add_space(6.);
            ui.horizontal_top(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(22., 22.), egui::Sense::hover());
                widgets::paint_icon(ui, r, *icon, theme::ORANGE);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(*name).font(theme::font(12., 500)));
                    ui.label(
                        egui::RichText::new(*description)
                            .size(12.)
                            .color(theme::SECONDARY),
                    );
                });
            });
        }
        ui.add_space(10.);
        ui.separator();
    }
}

use egui::{Color32, CornerRadius, Stroke};

pub const BACKGROUND: Color32 = Color32::from_rgb(16, 18, 20);
pub const STAGE_UI: Color32 = Color32::from_rgb(19, 23, 25);
pub const PANEL: Color32 = Color32::from_rgb(25, 28, 31);
pub const ELEVATED: Color32 = Color32::from_rgb(36, 40, 44);
pub const HOVER: Color32 = Color32::from_rgb(44, 49, 53);
pub const ORANGE: Color32 = Color32::from_rgb(245, 150, 81);
pub const ORANGE_HOVER: Color32 = Color32::from_rgb(255, 173, 112);
pub const ORANGE_PRESSED: Color32 = Color32::from_rgb(230, 106, 33);
pub const SELECTED: Color32 = Color32::from_rgb(57, 39, 29);
pub const TEXT: Color32 = Color32::from_rgb(238, 234, 227);
pub const SECONDARY: Color32 = Color32::from_rgb(178, 184, 187);
pub const MUTED: Color32 = Color32::from_rgb(130, 140, 145);
pub const BORDER: Color32 = Color32::from_rgb(50, 55, 59);
pub const WARNING: Color32 = Color32::from_rgb(212, 181, 117);
pub const ERROR: Color32 = Color32::from_rgb(237, 119, 119);
pub const SUCCESS: Color32 = Color32::from_rgb(156, 184, 167);
pub const RADIUS: u8 = 3;
pub const SPACE: f32 = 8.;

// Linear RGB values for wgpu's sRGB render target; kept beside the UI palette.
pub const STAGE: [f64; 3] = [0.00518, 0.00605, 0.007];
pub const GRID: [f32; 3] = [0.016, 0.021, 0.024];
pub const GRID_MAJOR: [f32; 3] = [0.030, 0.037, 0.041];
pub const BONE: [f32; 3] = [0.86, 0.49, 0.23];

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut fonts = egui::FontDefinitions::default();
    for (weight, bytes) in [
        (
            400,
            include_bytes!("../../assets/fonts/ArmatraUI-400.ttf").as_slice(),
        ),
        (
            500,
            include_bytes!("../../assets/fonts/ArmatraUI-500.ttf").as_slice(),
        ),
        (
            600,
            include_bytes!("../../assets/fonts/ArmatraUI-600.ttf").as_slice(),
        ),
    ] {
        let name = format!("ArmatraUI{weight}");
        fonts
            .font_data
            .insert(name.clone(), egui::FontData::from_static(bytes).into());
        let mut fallbacks = fonts.families[&egui::FontFamily::Proportional].clone();
        fallbacks.insert(0, name.clone());
        fonts
            .families
            .insert(egui::FontFamily::Name(name.into()), fallbacks);
    }
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "ArmatraUI400".into());
    ctx.set_fonts(fonts);
    let mut style = egui::Style {
        text_styles: [
            (egui::TextStyle::Heading, font(20., 600)),
            (egui::TextStyle::Body, font(13., 400)),
            (egui::TextStyle::Button, font(13., 500)),
            (egui::TextStyle::Small, font(11., 400)),
            (egui::TextStyle::Monospace, egui::FontId::monospace(13.)),
        ]
        .into(),
        ..Default::default()
    };
    style.spacing.item_spacing = egui::vec2(SPACE, SPACE);
    style.spacing.button_padding = egui::vec2(12., 7.);
    style.spacing.interact_size = egui::vec2(32., 30.);
    style.spacing.combo_width = 160.;
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.weak_text_color = Some(SECONDARY);
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.window_highlight_topmost = false;
    v.extreme_bg_color = BACKGROUND;
    v.faint_bg_color = ELEVATED;
    v.window_stroke = Stroke::new(1., BORDER);
    v.window_corner_radius = CornerRadius::same(8);
    v.selection.bg_fill = SELECTED;
    v.selection.stroke = Stroke::new(1., ORANGE);
    v.hyperlink_color = ORANGE;
    v.warn_fg_color = WARNING;
    v.error_fg_color = ERROR;
    for (widget, fill, stroke) in [
        (&mut v.widgets.noninteractive, PANEL, BORDER),
        (&mut v.widgets.inactive, ELEVATED, BORDER),
        (&mut v.widgets.hovered, HOVER, ORANGE_HOVER),
        (&mut v.widgets.active, SELECTED, ORANGE),
        (&mut v.widgets.open, SELECTED, ORANGE),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = Stroke::NONE;
        widget.fg_stroke = Stroke::new(1., stroke);
        widget.corner_radius = CornerRadius::same(RADIUS);
        widget.expansion = 0.;
    }
    // egui derives strong text and checkbox marks from these foreground strokes.
    v.widgets.active.fg_stroke.color = TEXT;
    v.widgets.inactive.fg_stroke.color = SECONDARY;
    v.widgets.hovered.fg_stroke.color = TEXT;
    v.widgets.open.fg_stroke.color = TEXT;
    ctx.set_style_of(egui::Theme::Dark, style);
}

pub fn font(size: f32, weight: u16) -> egui::FontId {
    egui::FontId::new(
        size,
        egui::FontFamily::Name(format!("ArmatraUI{weight}").into()),
    )
}

pub fn panel() -> egui::Frame {
    egui::Frame::NONE.fill(PANEL).inner_margin(14.)
}

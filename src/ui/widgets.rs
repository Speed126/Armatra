use super::theme as t;
use egui::{Color32, Response, RichText, Sense, Ui};

pub fn primary(ui: &mut Ui, label: &str) -> Response {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.bg_fill = t::ORANGE;
        ui.visuals_mut().widgets.inactive.weak_bg_fill = t::ORANGE;
        ui.visuals_mut().widgets.hovered.bg_fill = t::ORANGE_HOVER;
        ui.visuals_mut().widgets.active.bg_fill = t::ORANGE_PRESSED;
        ui.add(egui::Button::new(
            RichText::new(label).strong().color(t::BACKGROUND),
        ))
    })
    .inner
}

pub fn primary_icon(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().button_padding.x = 30.;
            primary(ui, label)
        })
        .inner;
    super::icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(
            egui::pos2(response.rect.left() + 16., response.rect.center().y),
            egui::vec2(18., 18.),
        ),
        icon,
        t::BACKGROUND,
    );
    response
}

/// One visual control with independently focusable/activated hit regions.
pub fn split_primary(ui: &mut Ui, icon: Icon, label: &str) -> (Response, Response) {
    let galley = ui
        .painter()
        .layout_no_wrap(label.into(), t::font(12., 500), t::BACKGROUND);
    let width = galley.size().x + 54.;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width + 32., 32.), Sense::hover());
    let main_rect = egui::Rect::from_min_max(rect.min, rect.right_bottom() - egui::vec2(32., 0.));
    let arrow_rect = egui::Rect::from_min_max(main_rect.right_top(), rect.max);
    let main = ui.interact(main_rect, ui.scope_id().with("split-apply"), Sense::click());
    let arrow = ui.interact(
        arrow_rect,
        ui.scope_id().with("split-options"),
        Sense::click(),
    );
    let fill = |response: &Response| {
        if !ui.is_enabled() {
            t::ELEVATED
        } else if response.is_pointer_button_down_on() {
            t::ORANGE_PRESSED
        } else if response.hovered() {
            t::ORANGE_HOVER
        } else {
            t::ORANGE
        }
    };
    let corners = egui::CornerRadius {
        nw: 3,
        sw: 3,
        ne: 0,
        se: 0,
    };
    ui.painter().rect_filled(main_rect, corners, fill(&main));
    ui.painter().rect_filled(
        arrow_rect,
        egui::CornerRadius {
            nw: 0,
            sw: 0,
            ne: 3,
            se: 3,
        },
        fill(&arrow),
    );
    ui.painter().vline(
        main_rect.right(),
        rect.top() + 6. ..=rect.bottom() - 6.,
        egui::Stroke::new(1., t::ORANGE_PRESSED),
    );
    let color = if ui.is_enabled() {
        t::BACKGROUND
    } else {
        t::MUTED
    };
    paint_icon(
        ui,
        egui::Rect::from_center_size(
            main_rect.left_center() + egui::vec2(16., 0.),
            egui::vec2(18., 18.),
        ),
        icon,
        color,
    );
    paint_icon(ui, arrow_rect.shrink(7.), Icon::ChevronDown, color);
    ui.painter().galley(
        main_rect.left_center() + egui::vec2(32., -galley.size().y / 2.),
        galley,
        color,
    );
    for response in [&main, &arrow] {
        if response.has_focus() {
            ui.painter().rect_stroke(
                response.rect.shrink(2.),
                2.,
                egui::Stroke::new(1.5, t::TEXT),
                egui::StrokeKind::Inside,
            );
        }
    }
    main.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    arrow.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), "More apply options")
    });
    (
        main.on_hover_cursor(egui::CursorIcon::PointingHand),
        arrow
            .on_hover_text("More apply options")
            .on_hover_cursor(egui::CursorIcon::PointingHand),
    )
}

pub fn secondary(ui: &mut Ui, label: &str) -> Response {
    ui.button(label)
}

pub fn nav(ui: &mut Ui, label: &str, active: bool) -> Response {
    let response = ui.add(
        egui::Button::new(RichText::new(label).color(if active { t::TEXT } else { t::SECONDARY }))
            .fill(if active {
                t::ELEVATED
            } else {
                Color32::TRANSPARENT
            }),
    );
    if active {
        let rect = response.rect;
        ui.painter().hline(
            rect.left() + 12. ..=rect.right() - 12.,
            rect.bottom() + 4.,
            egui::Stroke::new(2., t::ORANGE),
        );
    }
    response
}

pub fn section(ui: &mut Ui, label: &str) {
    ui.add_space(10.);
    ui.label(RichText::new(label).size(11.).strong().color(t::SECONDARY));
    ui.add_space(2.);
}

pub fn status(ui: &mut Ui, label: &str, color: Color32) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(6., 6.), Sense::hover());
        ui.painter().circle_filled(r.center(), 3., color);
        ui.label(RichText::new(label).color(color));
    });
}

pub fn search(ui: &mut Ui, text: &mut String, hint: &str) -> Response {
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .background_color(t::ELEVATED)
            .hint_text(hint)
            .margin(egui::vec2(10., 8.))
            .desired_width(f32::INFINITY),
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            t::RADIUS,
            egui::Stroke::new(1., t::ORANGE),
            egui::StrokeKind::Inside,
        );
    }
    response
}

pub fn empty(ui: &mut Ui, title: &str, detail: &str) {
    ui.add_space(20.);
    ui.label(RichText::new(title).size(18.).strong());
    ui.add_space(4.);
    ui.label(RichText::new(detail).color(t::SECONDARY));
    ui.add_space(12.);
}

pub fn property(ui: &mut Ui, name: &str, value: impl Into<String>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(name).color(t::SECONDARY));
        ui.label(value.into());
    });
}

pub use super::icons::Icon;

pub fn toggle(ui: &mut Ui, icon: Icon, label: &str, value: &mut bool) -> Response {
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.), Sense::click());
    if response.clicked() && ui.ctx().current_pass_index() == 0 {
        *value = !*value;
        response.mark_changed();
    }
    let p = ui.painter_at(rect);
    if response.hovered() {
        p.rect_filled(rect, 3., t::ELEVATED);
    }
    if response.has_focus() {
        p.rect_stroke(
            rect,
            3.,
            egui::Stroke::new(1., t::TEXT),
            egui::StrokeKind::Inside,
        );
    }
    super::icons::paint(
        &p,
        egui::Rect::from_center_size(
            rect.left_center() + egui::vec2(12., 0.),
            egui::vec2(20., 20.),
        ),
        icon,
        t::SECONDARY,
    );
    p.text(
        rect.left_center() + egui::vec2(32., 0.),
        egui::Align2::LEFT_CENTER,
        label,
        t::font(12., 400),
        t::TEXT,
    );
    let switch = egui::Rect::from_center_size(
        rect.right_center() - egui::vec2(16., 0.),
        egui::vec2(28., 14.),
    );
    p.rect_filled(switch, 7., if *value { t::SUCCESS } else { t::BORDER });
    p.circle_filled(
        egui::pos2(
            if *value {
                switch.right() - 7.
            } else {
                switch.left() + 7.
            },
            switch.center().y,
        ),
        5.,
        if *value { t::TEXT } else { t::SECONDARY },
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), *value, label)
    });
    response
}

pub fn icon(ui: &mut Ui, icon: Icon, active: bool, tooltip: &str) -> Response {
    let response = glyph_button(ui, icon, "", active);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tooltip));
    response.on_hover_text(tooltip)
}

pub fn glyph_button(ui: &mut Ui, icon: Icon, label: &str, active: bool) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), t::font(12., 500), t::TEXT);
    let width = if label.is_empty() {
        32.
    } else {
        galley.size().x + 44.
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.), Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        t::SELECTED
    } else if response.hovered() {
        t::HOVER
    } else if active {
        t::ELEVATED
    } else {
        Color32::TRANSPARENT
    };
    let p = ui.painter();
    p.rect_filled(rect, t::RADIUS, fill);
    if active {
        p.line_segment(
            [
                rect.left_bottom() + egui::vec2(3., 0.),
                rect.right_bottom() - egui::vec2(3., 0.),
            ],
            egui::Stroke::new(2., t::ORANGE),
        );
    }
    if response.has_focus() {
        p.rect_stroke(
            rect,
            3.,
            egui::Stroke::new(1.5, t::TEXT),
            egui::StrokeKind::Inside,
        );
    }
    let color = if !ui.is_enabled() {
        t::MUTED
    } else if active {
        t::ORANGE
    } else {
        t::SECONDARY
    };
    super::icons::paint(
        p,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 16., rect.center().y),
            egui::vec2(20., 20.),
        ),
        icon,
        color,
    );
    if !label.is_empty() {
        p.galley(
            egui::pos2(rect.left() + 32., rect.center().y - galley.size().y * 0.5),
            galley,
            t::TEXT,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn paint_icon(ui: &Ui, r: egui::Rect, icon: Icon, color: Color32) {
    super::icons::paint(ui.painter(), r, icon, color);
}

pub fn transport(ui: &mut Ui, icon: Icon, primary: bool, active: bool, label: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(32., 32.), Sense::click());
    let enabled = ui.is_enabled();
    let fill = if primary && enabled {
        t::ORANGE
    } else if response.hovered() {
        t::HOVER
    } else if active {
        t::ELEVATED
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 3., fill);
    if active && !primary {
        ui.painter().hline(
            rect.left() + 4. ..=rect.right() - 4.,
            rect.bottom(),
            egui::Stroke::new(2., t::ORANGE),
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            3.,
            egui::Stroke::new(1.5, t::TEXT),
            egui::StrokeKind::Inside,
        );
    }
    super::icons::paint(
        ui.painter(),
        rect.shrink(6.),
        icon,
        if !enabled {
            t::MUTED
        } else if primary {
            t::BACKGROUND
        } else {
            t::TEXT
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    response.on_hover_text(label)
}

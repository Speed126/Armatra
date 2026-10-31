//! One bounded motion clip and real support intervals. This widget emits seeks;
//! it never owns playback, edits contacts, or invents animation tracks.
use super::theme as t;
use crate::{
    contacts::Interval,
    rig::{Rig, Side},
    scene::Asset,
};
use egui::{Rect, pos2, vec2};

pub fn time_at(x: f32, track: Rect, duration: f32) -> f32 {
    ((x - track.left()) / track.width().max(1.)).clamp(0., 1.) * duration.max(0.)
}
pub fn x_at(time: f32, track: Rect, duration: f32) -> f32 {
    track.left() + track.width() * (time / duration.max(0.0001)).clamp(0., 1.)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneKind {
    Left,
    Right,
    Combined,
}
pub struct Lane {
    pub label: String,
    pub kind: LaneKind,
    pub intervals: Vec<Interval>,
}
/// Side comes from analyzed source/target anatomy, never a numeric node convention.
/// Conflicting or unknown identities fall back to an explicitly combined lane.
pub fn contact_lanes(intervals: &[Interval], rigs: [&Rig; 2], assets: [&Asset; 2]) -> Vec<Lane> {
    let mut result = vec![
        Lane {
            label: "L FOOT".into(),
            kind: LaneKind::Left,
            intervals: vec![],
        },
        Lane {
            label: "R FOOT".into(),
            kind: LaneKind::Right,
            intervals: vec![],
        },
    ];
    for interval in intervals {
        let source = rigs[0]
            .joints
            .iter()
            .find(|j| j.node == interval.source)
            .map(|j| j.side);
        let target = rigs[1]
            .joints
            .iter()
            .find(|j| j.node == interval.target)
            .map(|j| j.side);
        let side = match (source, target) {
            (Some(Side::Left), Some(Side::Left)) => Some(0),
            (Some(Side::Right), Some(Side::Right)) => Some(1),
            (Some(Side::Left), None | Some(Side::Unknown))
            | (None | Some(Side::Unknown), Some(Side::Left)) => Some(0),
            (Some(Side::Right), None | Some(Side::Unknown))
            | (None | Some(Side::Unknown), Some(Side::Right)) => Some(1),
            _ => None,
        };
        if let Some(side) = side {
            result[side].intervals.push(interval.clone());
        } else {
            let label = if intervals.len() == 1 {
                assets[1]
                    .nodes
                    .get(interval.target)
                    .map_or("CONTACTS".to_owned(), |n| n.name.clone())
            } else {
                "CONTACTS".into()
            };
            return vec![Lane {
                label,
                kind: LaneKind::Combined,
                intervals: intervals.to_vec(),
            }];
        }
    }
    result
}
pub struct Model<'a> {
    pub name: &'a str,
    pub duration: f32,
    pub time: f32,
    pub lanes: &'a [Lane],
}
pub struct ContactBar {
    pub lane: LaneKind,
    pub source: usize,
    pub target: usize,
    pub start: f32,
    pub end: f32,
    pub rect: Rect,
}
pub struct Output {
    pub seek: Option<f32>,
    pub hover: Option<f32>,
    pub track: Rect,
    pub clip: Rect,
    pub bars: Vec<ContactBar>,
}
pub fn show(ui: &mut egui::Ui, model: &Model<'_>) -> Output {
    let height = ui.available_height().max(66.);
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), egui::Sense::hover());
    let compact = height < 100.;
    let gutter = if rect.width() < 900. { 72. } else { 88. };
    let track = Rect::from_min_max(
        pos2(rect.left() + gutter, rect.top()),
        pos2(rect.right() - 8., rect.bottom()),
    );
    let response = ui.interact(
        track,
        ui.scope_id().with("motion-time"),
        egui::Sense::click_and_drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Slider,
            ui.is_enabled(),
            format!(
                "{} time {:.3} of {:.3} seconds",
                model.name, model.time, model.duration
            ),
        )
    });
    let hover = response
        .hover_pos()
        .map(|p| time_at(p.x, track, model.duration));
    let seek = if model.duration > 0.
        && ui.ctx().current_pass_index() == 0
        && (response.clicked_by(egui::PointerButton::Primary)
            || response.dragged_by(egui::PointerButton::Primary))
    {
        response
            .interact_pointer_pos()
            .map(|p| time_at(p.x, track, model.duration))
    } else {
        None
    };
    let mut output = Output {
        seek,
        hover,
        track,
        clip: Rect::NOTHING,
        bars: vec![],
    };
    let p = ui.painter_at(rect);
    let ruler = if compact { 16. } else { 24. };
    let clip_height = if compact { 18. } else { 26. };
    let clip = Rect::from_min_max(
        pos2(track.left(), rect.top() + ruler),
        pos2(track.right(), rect.top() + ruler + clip_height),
    );
    output.clip = clip;
    p.hline(rect.x_range(), rect.top(), egui::Stroke::new(1., t::BORDER));
    if model.duration <= 0. {
        p.text(
            clip.left_center(),
            egui::Align2::LEFT_CENTER,
            "Choose a motion",
            t::font(12., 400),
            t::SECONDARY,
        );
        return output;
    }
    let desired = model.duration / (track.width() / 80.).max(1.);
    let power = 10f32.powf(desired.max(0.0001).log10().floor());
    let step = [1., 2., 5., 10.]
        .into_iter()
        .find(|n| *n * power >= desired)
        .unwrap_or(10.)
        * power;
    let tick = |time: f32, major: bool| {
        let x = x_at(time, track, model.duration);
        let bottom = clip.top() - 3.;
        p.vline(
            x,
            bottom - if major { 7. } else { 3. }..=bottom,
            egui::Stroke::new(1., if major { t::MUTED } else { t::BORDER }),
        );
        if major {
            let align = if time == 0. {
                egui::Align2::LEFT_TOP
            } else if time == model.duration {
                egui::Align2::RIGHT_TOP
            } else {
                egui::Align2::CENTER_TOP
            };
            p.text(
                pos2(x, rect.top() + 1.),
                align,
                format!("{time:.2}"),
                egui::FontId::monospace(11.),
                t::SECONDARY,
            );
        }
    };
    tick(0., true);
    tick(model.duration, true);
    for i in 1..=((model.duration / step * 5.).ceil() as usize).min(1000) {
        let time = i as f32 * step / 5.;
        if time >= model.duration {
            break;
        }
        let major = i % 5 == 0;
        if major && (track.right() - x_at(time, track, model.duration)) < 48. {
            continue;
        }
        tick(time, major);
    }
    p.text(
        pos2(rect.left() + 4., clip.center().y),
        egui::Align2::LEFT_CENTER,
        "MOTION",
        t::font(11., 500),
        t::SECONDARY,
    );
    p.rect_filled(clip, 2., t::ELEVATED);
    p.vline(
        clip.left(),
        clip.y_range(),
        egui::Stroke::new(3., t::ORANGE),
    );
    let clip_painter = ui.painter_at(clip.shrink2(vec2(8., 0.)));
    let mut title =
        egui::text::LayoutJob::simple_singleline(model.name.to_owned(), t::font(12., 400), t::TEXT);
    title.wrap.max_width = (clip.width() - if clip.width() > 360. { 110. } else { 20. }).max(1.);
    title.wrap.max_rows = 1;
    title.wrap.overflow_character = Some('…');
    let galley = ui.fonts_mut(|f| f.layout_job(title));
    clip_painter.galley(
        pos2(clip.left() + 10., clip.center().y - galley.size().y * 0.5),
        galley,
        t::TEXT,
    );
    if clip.width() > 360. {
        clip_painter.text(
            clip.right_center() - vec2(10., 0.),
            egui::Align2::RIGHT_CENTER,
            format!("{:.2} s", model.duration),
            egui::FontId::monospace(11.),
            t::SECONDARY,
        );
    }
    let lane_height = if compact { 12. } else { 14. };
    let gap = if compact { 4. } else { 8. };
    let lane_gap = if compact { 4. } else { 6. };
    if model.lanes.is_empty() {
        p.text(
            pos2(rect.left() + 4., clip.bottom() + gap + lane_height * 0.5),
            egui::Align2::LEFT_CENTER,
            "CONTACTS",
            t::font(11., 400),
            t::MUTED,
        );
        p.text(
            pos2(track.left(), clip.bottom() + gap + lane_height * 0.5),
            egui::Align2::LEFT_CENTER,
            "Not available for this motion / target",
            t::font(11., 400),
            t::SECONDARY,
        );
    }
    for (index, lane) in model.lanes.iter().enumerate() {
        let y = clip.bottom() + gap + index as f32 * (lane_height + lane_gap);
        let lane_rect =
            Rect::from_min_size(pos2(track.left(), y), vec2(track.width(), lane_height));
        let label_painter = ui.painter_at(Rect::from_min_max(
            pos2(rect.left(), y - 2.),
            pos2(track.left() - 6., y + lane_height + 2.),
        ));
        label_painter.text(
            pos2(rect.left() + 4., lane_rect.center().y),
            egui::Align2::LEFT_CENTER,
            &lane.label,
            egui::FontId::monospace(11.),
            t::SECONDARY,
        );
        p.rect_filled(lane_rect, 1., t::STAGE_UI);
        for interval in &lane.intervals {
            if interval.end < interval.start {
                continue;
            }
            let bar = Rect::from_min_max(
                pos2(x_at(interval.start, track, model.duration), y),
                pos2(x_at(interval.end, track, model.duration), y + lane_height),
            );
            p.rect_filled(bar, 1., t::SUCCESS.gamma_multiply(0.28));
            if lane.kind == LaneKind::Right {
                let hatch = ui.painter_at(bar);
                let mut x = bar.left() - lane_height;
                while x < bar.right() {
                    hatch.line_segment(
                        [pos2(x, bar.bottom()), pos2(x + lane_height, bar.top())],
                        egui::Stroke::new(0.8, t::SUCCESS.gamma_multiply(0.55)),
                    );
                    x += 6.;
                }
            } else {
                p.hline(bar.x_range(), bar.top(), egui::Stroke::new(2., t::SUCCESS));
            }
            if !compact && bar.width() > 100. {
                ui.painter_at(bar).text(
                    bar.left_center() + vec2(6., 0.),
                    egui::Align2::LEFT_CENTER,
                    format!("{:.3}–{:.3}", interval.start, interval.end),
                    egui::FontId::monospace(11.),
                    t::SUCCESS,
                );
            }
            if response.hover_pos().is_some_and(|v| bar.contains(v)) {
                response.clone().on_hover_text(format!(
                    "{} · source {} → target {}\n{:.6}–{:.6} s · detected support interval",
                    lane.label, interval.source, interval.target, interval.start, interval.end
                ));
            }
            output.bars.push(ContactBar {
                lane: lane.kind,
                source: interval.source,
                target: interval.target,
                start: interval.start,
                end: interval.end,
                rect: bar,
            });
        }
    }
    let current = x_at(seek.unwrap_or(model.time), track, model.duration);
    p.vline(
        current,
        clip.top() - 6. ..=rect.bottom() - 2.,
        egui::Stroke::new(1.5, t::ORANGE),
    );
    p.add(egui::Shape::convex_polygon(
        vec![
            pos2(current - 5., clip.top() - 8.),
            pos2(current + 5., clip.top() - 8.),
            pos2(current, clip.top() - 1.),
        ],
        t::ORANGE,
        egui::Stroke::NONE,
    ));
    if let Some(time) = hover {
        let x = x_at(time, track, model.duration);
        let mut y = clip.top();
        while y < rect.bottom() - 2. {
            p.vline(
                x,
                y..=(y + 3.).min(rect.bottom() - 2.),
                egui::Stroke::new(1., t::SECONDARY),
            );
            y += 7.;
        }
        let box_width = 80f32.min(track.width().max(1.));
        let left = (x - box_width * 0.5).clamp(track.left(), track.right() - box_width);
        let badge = Rect::from_min_size(pos2(left, rect.top()), vec2(box_width, 18.));
        p.rect_filled(badge, 2., t::HOVER);
        p.text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            format!("{time:.3} s"),
            egui::FontId::monospace(11.),
            t::TEXT,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    output
}
/// Library transport uses a shallow version of the same single-clip seek field.
/// It does not manufacture contact lanes when discovery is the active task.
pub fn mini(ui: &mut egui::Ui, model: &Model<'_>) -> Output {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ui.available_height().max(24.)),
        egui::Sense::hover(),
    );
    let track = rect.shrink2(egui::vec2(8., 0.));
    let clip = Rect::from_min_max(
        pos2(track.left(), track.top() + 2.),
        pos2(track.right(), track.top() + 14.),
    );
    let response = ui.interact(
        track,
        ui.scope_id().with("library-motion-time"),
        egui::Sense::click_and_drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Slider,
            ui.is_enabled(),
            format!("{} time {:.3} seconds", model.name, model.time),
        )
    });
    let seek = if model.duration > 0.
        && ui.ctx().current_pass_index() == 0
        && (response.clicked() || response.dragged_by(egui::PointerButton::Primary))
    {
        response
            .interact_pointer_pos()
            .map(|p| time_at(p.x, track, model.duration))
    } else {
        None
    };
    let hover = response
        .hover_pos()
        .map(|p| time_at(p.x, track, model.duration));
    let p = ui.painter_at(rect);
    p.rect_filled(clip, 2., t::ELEVATED);
    let x = x_at(seek.unwrap_or(model.time), track, model.duration);
    p.vline(x, clip.y_range(), egui::Stroke::new(2., t::ORANGE));
    p.text(
        pos2(track.left(), clip.bottom() + 1.),
        egui::Align2::LEFT_TOP,
        "0.000",
        egui::FontId::monospace(11.),
        t::MUTED,
    );
    p.text(
        pos2(track.right(), clip.bottom() + 1.),
        egui::Align2::RIGHT_TOP,
        format!("{:.3} s", model.duration),
        egui::FontId::monospace(11.),
        t::MUTED,
    );
    if let Some(time) = hover {
        p.vline(
            x_at(time, track, model.duration),
            track.y_range(),
            egui::Stroke::new(1., t::SECONDARY),
        );
        response.on_hover_text(format!("{} · {time:.3} s", model.name));
    }
    Output {
        seek,
        hover,
        track,
        clip,
        bars: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seeking_uses_track_bounds_and_preserves_exact_endpoints() {
        let r = Rect::from_min_size(pos2(10., 0.), vec2(200., 50.));
        assert_eq!(time_at(-20., r, 4.), 0.);
        assert_eq!(time_at(10., r, 4.), 0.);
        assert_eq!(time_at(110., r, 4.), 2.);
        assert_eq!(time_at(210., r, 4.), 4.);
        assert_eq!(time_at(900., r, 4.), 4.);
        assert_eq!(time_at(110., r, 0.), 0.);
    }
    #[test]
    fn contact_side_uses_real_rig_anatomy_and_unknowns_fall_back() {
        let (builder, _) = crate::rig_corpus::humanoid(Default::default(), true);
        let asset = crate::import::from_bytes(&builder.finish(), &crate::import::Limits::default())
            .unwrap();
        let rig = Rig::analyze(&asset);
        let feet: Vec<_> = rig
            .joints
            .iter()
            .filter(|j| j.anatomy == crate::rig::Anatomy::Foot)
            .collect();
        let intervals: Vec<_> = feet
            .iter()
            .map(|j| Interval {
                source: j.node,
                target: j.node,
                start: 0.2,
                end: 0.7,
                source_point: [0.; 3],
                anchor: [0.; 3],
            })
            .collect();
        let lanes = contact_lanes(&intervals, [&rig, &rig], [&asset, &asset]);
        assert_eq!(lanes.len(), 2);
        assert!(!lanes[0].intervals.is_empty());
        assert!(!lanes[1].intervals.is_empty());
        let mut unknown = rig.clone();
        for j in &mut unknown.joints {
            j.side = Side::Unknown;
        }
        let combined = contact_lanes(&intervals, [&unknown, &unknown], [&asset, &asset]);
        assert_eq!(combined.len(), 1);
        assert_eq!(combined[0].kind, LaneKind::Combined);
        assert_eq!(combined[0].intervals, intervals);
    }
}

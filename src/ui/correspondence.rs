//! Rest-skeleton correspondence. Scene-unit recovery, projection, hit testing and
//! annotation geometry are shared by native painting and interaction checks.
use super::theme as t;
use crate::{
    rig::{Anatomy, Rig},
    scene::Asset,
};
use egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};
use glam::{Quat, Vec3};
mod operation;
pub use operation::{Operation, Selection};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub pan: Vec2,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: 0.,
            pitch: 0.,
            zoom: 1.,
            pan: Vec2::ZERO,
        }
    }
}
#[derive(Default)]
pub struct State {
    pub cameras: [Camera; 2],
    pub unlinked: bool,
    pub true_scale: bool,
    pub orphan_target: Option<usize>,
    pub candidates: Vec<usize>,
    pub operation: Operation,
    pub navigation: Option<(usize, bool)>,
}
impl State {
    pub fn frame(&mut self) {
        self.cameras = [Camera::default(); 2];
    }
    pub fn orbit(&mut self, side: usize, delta: Vec2) {
        self.cameras[side].yaw += delta.x * 0.008;
        self.cameras[side].pitch = (self.cameras[side].pitch + delta.y * 0.008).clamp(-1.3, 1.3);
        if !self.unlinked {
            self.link_orientation(side);
        }
    }
    pub fn link_orientation(&mut self, side: usize) {
        self.cameras[1 - side].yaw = self.cameras[side].yaw;
        self.cameras[1 - side].pitch = self.cameras[side].pitch;
    }
    pub fn fit_side(&mut self, side: usize) {
        self.cameras[side].zoom = 1.;
        self.cameras[side].pan = Vec2::ZERO;
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Automatic,
    Manual,
    Ambiguous,
    Unmapped,
}
impl Status {
    pub fn color(self) -> Color32 {
        match self {
            Self::Automatic => t::SUCCESS,
            Self::Manual => t::ORANGE,
            Self::Ambiguous => t::WARNING,
            Self::Unmapped => t::MUTED,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Manual => "Manual",
            Self::Ambiguous => "Ambiguous",
            Self::Unmapped => "Unmapped",
        }
    }
}
#[derive(Clone, Copy)]
pub struct Link {
    pub source: usize,
    pub target: Option<usize>,
    pub status: Status,
}
pub struct Model<'a> {
    pub rigs: [&'a Rig; 2],
    pub assets: [&'a Asset; 2],
    pub links: &'a [Link],
    pub selected: Option<usize>,
}
#[derive(Clone, Copy, Debug)]
pub struct Joint {
    pub node: usize,
    pub parent: Option<usize>,
    pub point: Pos2,
    pub depth: f32,
    pub helper: bool,
}
pub struct Geometry {
    pub rect: Rect,
    pub joints: [Vec<Joint>; 2],
    /// Source display scale. Framing never changes asset or retarget units.
    pub scale: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub side: usize,
    pub node: usize,
}

/// Use normalized coordinates for numerically stable editing at any scene size.
fn positions(rig: &Rig, camera: Camera) -> Vec<Vec3> {
    let rotate = Quat::from_rotation_x(camera.pitch) * Quat::from_rotation_y(camera.yaw);
    rig.joints.iter().map(|j| rotate * j.position).collect()
}
pub fn project(rect: Rect, rigs: [&Rig; 2], state: &State) -> Geometry {
    let values = [
        positions(rigs[0], state.cameras[0]),
        positions(rigs[1], state.cameras[1]),
    ];
    let bounds: Vec<_> = values
        .iter()
        .map(|v| {
            if v.is_empty() {
                return (Vec3::ZERO, Vec3::ONE);
            }
            (
                v.iter()
                    .copied()
                    .fold(Vec3::splat(f32::INFINITY), Vec3::min),
                v.iter()
                    .copied()
                    .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max),
            )
        })
        .collect();
    let padding = if rect.height() < 400. { 52. } else { 100. };
    let fits: [f32; 2] = std::array::from_fn(|side| {
        let extent = bounds[side].1 - bounds[side].0;
        ((rect.height() - padding).max(1.) / extent.y.max(1e-6))
            .min((rect.width() * 0.5 - 80.).max(1.) / extent.x.max(1e-6))
            .min(1e6)
    });
    let scene_scale = (fits[0] / rigs[0].height.max(f32::MIN_POSITIVE))
        .min(fits[1] / rigs[1].height.max(f32::MIN_POSITIVE));
    let scales: [f32; 2] = std::array::from_fn(|side| {
        if state.true_scale {
            scene_scale * rigs[side].height
        } else {
            fits[side]
        }
    });
    let joints = std::array::from_fn(|side| {
        let (min, max) = bounds[side];
        let camera = state.cameras[side];
        let anchor = pos2(
            rect.left() + rect.width() * (if side == 0 { 0.255 } else { 0.755 }),
            rect.bottom() - 12.,
        );
        values[side]
            .iter()
            .zip(&rigs[side].joints)
            .map(|(p, d)| Joint {
                node: d.node,
                parent: d.parent,
                depth: p.z,
                point: anchor
                    + vec2(p.x - (min.x + max.x) * 0.5, -(p.y - min.y))
                        * scales[side]
                        * camera.zoom
                    + camera.pan,
                helper: d.anatomy == Anatomy::Helper,
            })
            .collect()
    });
    Geometry {
        rect,
        joints,
        scale: scales[0],
    }
}
pub fn hit(geometry: &Geometry, pointer: Pos2) -> Option<Hit> {
    if !geometry.rect.contains(pointer) {
        return None;
    }
    let mut candidates: Vec<_> = geometry
        .joints
        .iter()
        .enumerate()
        .filter(|(side, _)| *side == usize::from(pointer.x >= geometry.rect.center().x))
        .flat_map(|(side, v)| v.iter().map(move |j| (side, j)))
        .filter_map(|(side, j)| {
            let distance = j.point.distance_sq(pointer);
            (distance <= 14. * 14.).then_some((distance, j.depth, side, j.node))
        })
        .collect();
    candidates.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.1.total_cmp(&b.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.cmp(&b.3))
    });
    if let Some((_, _, side, node)) = candidates.first() {
        return Some(Hit {
            side: *side,
            node: *node,
        });
    }
    // Outgoing segments select the parent joint, matching the emphasized bone.
    geometry
        .joints
        .iter()
        .enumerate()
        .filter(|(side, _)| *side == usize::from(pointer.x >= geometry.rect.center().x))
        .flat_map(|(side, v)| {
            v.iter().filter_map(move |j| {
                let parent = v.iter().find(|p| Some(p.node) == j.parent)?;
                let line = j.point - parent.point;
                let fraction = ((pointer - parent.point).dot(line) / line.length_sq().max(0.0001))
                    .clamp(0., 1.);
                let distance = (parent.point + line * fraction).distance(pointer);
                (distance < 6.).then_some((
                    distance,
                    Hit {
                        side,
                        node: parent.node,
                    },
                ))
            })
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|v| v.1)
}
pub fn interact(
    ui: &mut egui::Ui,
    rect: Rect,
    state: &mut State,
    rigs: [&Rig; 2],
) -> (Geometry, Option<Hit>) {
    let response = ui.interact(
        rect,
        ui.scope_id().with("correspondence-canvas"),
        egui::Sense::click_and_drag(),
    );
    response.widget_info(||egui::WidgetInfo::labeled(egui::Role::Button,ui.is_enabled(),"Source and target skeletons; select joints, drag to orbit, right drag to pan, scroll to zoom"));
    let before = project(rect, rigs, state);
    if ui.ctx().current_pass_index() == 0 {
        if ui.input(|i| {
            i.events
                .iter()
                .any(|event| matches!(event, egui::Event::WindowFocused(false)))
        }) {
            state.operation = Operation::Idle;
            state.navigation = None;
        }
        let (pointer, middle, right, left, shift) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.button_pressed(egui::PointerButton::Middle),
                i.pointer.button_pressed(egui::PointerButton::Secondary),
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.modifiers.shift,
            )
        });
        if response.contains_pointer()
            && let Some(pointer) = pointer
            && (middle || right || left)
        {
            let side = usize::from(pointer.x >= rect.center().x);
            let joint = hit(&before, pointer);
            if !right && (middle || (left && shift)) && joint.is_some_and(|j| j.side == 0) {
                state.operation = Operation::Drag {
                    source: joint.unwrap().node,
                    start: pointer,
                    pointer,
                    button: if middle {
                        egui::PointerButton::Middle
                    } else {
                        egui::PointerButton::Primary
                    },
                    active: false,
                    target: None,
                };
                state.navigation = None;
            } else {
                state.navigation = if right || middle {
                    Some((side, true))
                } else if joint.is_none() {
                    Some((side, false))
                } else {
                    None
                };
            }
        }
        if let Operation::Drag {
            source,
            start,
            pointer: at,
            button,
            active,
            target,
        } = &mut state.operation
        {
            if let Some(p) = pointer {
                *at = p;
                *active |= p.distance(*start) > 5.;
                *target = hit(&before, p).filter(|h| h.side == 1).map(|h| h.node);
            }
            if ui.input(|i| i.pointer.button_released(*button)) {
                let pair = if *active {
                    target.map(|t| (*source, t))
                } else {
                    None
                };
                state.operation = Operation::Idle;
                if let Some((s, t)) = pair {
                    state.operation.begin(Some(s), Some(t));
                }
            }
        } else if response.dragged()
            && let Some((side, pan)) = state.navigation
        {
            let delta = ui.input(|i| i.pointer.delta());
            if pan {
                state.cameras[side].pan += delta;
            } else {
                state.orbit(side, delta);
            }
        }
        if !ui.input(|i| i.pointer.any_down()) {
            state.navigation = None;
        }
    }
    if let Some(pointer) = response
        .interact_pointer_pos()
        .or_else(|| response.hover_pos())
    {
        let side = usize::from(pointer.x >= rect.center().x);
        if response.hovered() && hit(&before, pointer).is_some_and(|h| h.side == 0) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let wheel = if response.hovered() && ui.ctx().current_pass_index() == 0 {
            ui.input(|i| i.smooth_scroll_delta.y)
        } else {
            0.
        };
        if wheel != 0. {
            let old = state.cameras[side].zoom;
            let zoom = (old * (wheel * 0.003).exp()).clamp(0.4, 4.);
            let origin = pos2(
                rect.left() + rect.width() * (if side == 0 { 0.255 } else { 0.755 }),
                rect.bottom() - 12.,
            );
            state.cameras[side].pan =
                (state.cameras[side].pan + (origin - pointer)) * (zoom / old) + (pointer - origin);
            state.cameras[side].zoom = zoom;
        }
        if response.double_clicked() {
            state.fit_side(side);
        }
    }
    let geometry = project(rect, rigs, state);
    let clicked =
        if response.clicked() && !response.double_clicked() && ui.ctx().current_pass_index() == 0 {
            response.request_focus();
            response
                .interact_pointer_pos()
                .and_then(|p| hit(&geometry, p))
        } else {
            None
        };
    (geometry, clicked)
}

pub fn paint_operation(ui: &egui::Ui, geometry: &Geometry, operation: &Operation) {
    let point = |side: usize, node: usize| {
        geometry.joints[side]
            .iter()
            .find(|j| j.node == node)
            .map(|j| j.point)
    };
    let endpoints = match operation {
        Operation::Drag {
            source,
            pointer,
            active: true,
            target,
            ..
        } => {
            ui.ctx().set_cursor_icon(if target.is_some() {
                egui::CursorIcon::Crosshair
            } else {
                egui::CursorIcon::NotAllowed
            });
            point(0, *source).map(|from| {
                (
                    from,
                    target.and_then(|n| point(1, n)).unwrap_or(*pointer),
                    false,
                    target.is_some(),
                )
            })
        }
        Operation::Selecting(selection) => selection
            .source
            .and_then(|s| point(0, s))
            .zip(selection.target.and_then(|n| point(1, n)))
            .map(|(a, b)| (a, b, true, true)),
        _ => None,
    };
    let p = ui.painter_at(geometry.rect);
    if let Some((a, b, dashed, eligible)) = endpoints {
        let bend = (b.x - a.x).abs() * 0.4;
        let controls = [
            a,
            pos2(a.x + bend, (a.y - 45.).max(geometry.rect.top() + 46.)),
            pos2(b.x - bend, (b.y - 45.).max(geometry.rect.top() + 46.)),
            b,
        ];
        if dashed {
            let at = |t: f32| {
                controls[0].to_vec2() * (1. - t).powi(3)
                    + controls[1].to_vec2() * 3. * (1. - t).powi(2) * t
                    + controls[2].to_vec2() * 3. * (1. - t) * t * t
                    + controls[3].to_vec2() * t.powi(3)
            };
            for i in (0..48).step_by(3) {
                p.line_segment(
                    [
                        at(i as f32 / 48.).to_pos2(),
                        at((i + 2) as f32 / 48.).to_pos2(),
                    ],
                    egui::Stroke::new(1.5, t::ORANGE),
                );
            }
        } else {
            p.add(egui::epaint::CubicBezierShape::from_points_stroke(
                controls,
                false,
                Color32::TRANSPARENT,
                egui::Stroke::new(2., t::ORANGE),
            ));
        }
        p.circle_stroke(a, 10., egui::Stroke::new(1.5, t::ORANGE));
        if eligible {
            p.circle_stroke(b, 10., egui::Stroke::new(1.5, t::ORANGE));
        } else {
            p.circle_filled(b, 2., t::ORANGE);
        }
    } else if let Operation::Selecting(selection) = operation {
        for (side, node) in [selection.source, selection.target].into_iter().enumerate() {
            if let Some(at) = node.and_then(|n| point(side, n)) {
                p.circle_stroke(at, 10., egui::Stroke::new(1.5, t::ORANGE));
            }
        }
    } else if let Some(at) = ui.input(|i| i.pointer.hover_pos())
        && let Some(h) = hit(geometry, at).filter(|h| h.side == 0)
        && let Some(at) = point(0, h.node)
    {
        p.circle_stroke(
            at,
            8.,
            egui::Stroke::new(1., t::ORANGE.gamma_multiply(0.65)),
        );
    }
}

pub fn marker(p: &egui::Painter, at: Pos2, status: Status, radius: f32) {
    let color = status.color();
    match status {
        Status::Automatic => {
            p.circle_filled(at, radius, color);
        }
        Status::Manual => {
            p.rect_filled(
                Rect::from_center_size(at, Vec2::splat(radius * 2.)),
                0.,
                color,
            );
        }
        Status::Ambiguous => {
            p.add(egui::Shape::closed_line(
                vec![
                    at + vec2(0., -radius - 1.),
                    at + vec2(radius + 1., 0.),
                    at + vec2(0., radius + 1.),
                    at + vec2(-radius - 1., 0.),
                ],
                egui::Stroke::new(1., color),
            ));
        }
        Status::Unmapped => {
            p.circle_stroke(at, radius, egui::Stroke::new(1., color));
        }
    }
}
pub struct PaintEvidence {
    pub labels: Vec<Rect>,
    pub selected_points: Vec<Pos2>,
    pub selected_bones: Vec<[Pos2; 2]>,
}
pub fn paint(
    ui: &mut egui::Ui,
    geometry: &Geometry,
    model: &Model<'_>,
    state: &State,
) -> PaintEvidence {
    let rect = geometry.rect;
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0., t::STAGE_UI);
    p.text(
        rect.right_top() + vec2(-16., 22.),
        egui::Align2::RIGHT_TOP,
        if rect.width() < 700. {
            format!(
                "Target: {:.4}×",
                model.rigs[1].height / model.rigs[0].height
            )
        } else {
            format!(
                "Target: {:.4}× source height · {}",
                model.rigs[1].height / model.rigs[0].height,
                if state.true_scale {
                    "True scale"
                } else {
                    "Fitted views"
                }
            )
        },
        t::font(11., 400),
        t::SECONDARY,
    );
    let selected_target = if let Some(s) = model.selected {
        model
            .links
            .iter()
            .find(|l| l.source == s)
            .and_then(|l| l.target)
    } else if matches!(state.operation, Operation::Idle) {
        state.orphan_target
    } else {
        None
    };
    let selected = [model.selected, selected_target];
    let mut occupied = Vec::new();
    let mut evidence = PaintEvidence {
        labels: vec![],
        selected_points: vec![],
        selected_bones: vec![],
    };
    for (side, joints) in geometry.joints.iter().enumerate() {
        let side_rect = Rect::from_min_max(
            pos2(rect.left() + side as f32 * rect.width() * 0.5, rect.top()),
            pos2(
                rect.left() + (side + 1) as f32 * rect.width() * 0.5,
                rect.bottom(),
            ),
        );
        let p = p.with_clip_rect(side_rect);
        p.text(
            pos2(
                rect.left() + rect.width() * side as f32 * 0.5 + 28.,
                rect.top() + 22.,
            ),
            egui::Align2::LEFT_TOP,
            if side == 0 {
                "01 / SOURCE"
            } else {
                "02 / TARGET"
            },
            t::font(11., 400),
            t::SECONDARY,
        );
        if joints.len() <= 1 {
            p.text(
                pos2(
                    rect.left() + rect.width() * side as f32 * 0.5 + 28.,
                    rect.top() + 48.,
                ),
                egui::Align2::LEFT_TOP,
                "Rig analysis incomplete: ≤1 joint. Inspect skin in Advanced.",
                t::font(11., 400),
                t::WARNING,
            );
        } else if model.rigs[side]
            .joints
            .iter()
            .filter(|j| !matches!(j.anatomy, Anatomy::Unknown | Anatomy::Helper))
            .count()
            <= 1
        {
            p.text(
                pos2(side_rect.left() + 28., rect.top() + 48.),
                egui::Align2::LEFT_TOP,
                "Rig roles incomplete · Inspect skin / anatomy in Advanced",
                t::font(11., 400),
                t::WARNING,
            );
        }
        for j in joints {
            if let Some(parent) = joints.iter().find(|v| Some(v.node) == j.parent) {
                let delta = j.point - parent.point;
                if delta.length() > 0.5 {
                    let normal = vec2(-delta.y, delta.x).normalized();
                    let active = selected[side] == Some(parent.node);
                    let width = if active { 6. } else { 2.2 };
                    let vertices = vec![
                        parent.point,
                        parent.point + delta * 0.22 + normal * width,
                        j.point,
                        parent.point + delta * 0.22 - normal * width,
                    ];
                    p.add(egui::Shape::convex_polygon(
                        vertices.clone(),
                        if active { t::ORANGE } else { t::ELEVATED },
                        egui::Stroke::NONE,
                    ));
                    // Separate edges avoid acute polygon miter spikes at tips.
                    if !active {
                        for i in 0..4 {
                            p.line_segment(
                                [vertices[i], vertices[(i + 1) % 4]],
                                egui::Stroke::new(0.7, t::MUTED),
                            );
                        }
                    }
                    occupied.push(Rect::from_two_pos(parent.point, j.point).expand(5.));
                    if active {
                        evidence.selected_bones.push([parent.point, j.point]);
                    }
                }
            }
            if j.helper {
                p.line_segment(
                    [j.point - vec2(3., 0.), j.point + vec2(3., 0.)],
                    egui::Stroke::new(1., t::MUTED),
                );
                p.line_segment(
                    [j.point - vec2(0., 3.), j.point + vec2(0., 3.)],
                    egui::Stroke::new(1., t::MUTED),
                );
            } else {
                let link = model.links.iter().find(|l| {
                    if side == 0 {
                        l.source == j.node
                    } else {
                        l.target == Some(j.node)
                    }
                });
                let status = link.map_or(Status::Unmapped, |l| l.status);
                // Automatic structure stays neutral; category is explicit on selection/list.
                if status == Status::Automatic {
                    p.circle_filled(j.point, 2.5, t::SECONDARY);
                } else {
                    marker(&p, j.point, status, 3.);
                }
            }
        }
    }
    let endpoints: Vec<_> = (0..2)
        .filter_map(|side| {
            selected[side].and_then(|n| {
                geometry.joints[side]
                    .iter()
                    .find(|j| j.node == n)
                    .map(|j| (side, j))
            })
        })
        .collect();
    let mut bridge_segments = vec![];
    if endpoints.len() == 2 {
        let a = endpoints[0].1.point;
        let b = endpoints[1].1.point;
        let crown =
            (a.y.min(b.y) - 125.).max(rect.top() + if rect.height() < 400. { 30. } else { 55. });
        let controls = [a, pos2(a.x + 80., crown), pos2(b.x - 80., crown), b];
        p.add(egui::epaint::CubicBezierShape::from_points_stroke(
            controls,
            false,
            Color32::TRANSPARENT,
            egui::Stroke::new(1.5, t::ORANGE),
        ));
        let mut prev = a;
        for i in 1..=32 {
            let v = i as f32 / 32.;
            let u = 1. - v;
            let next = pos2(
                u * u * u * a.x
                    + 3. * u * u * v * controls[1].x
                    + 3. * u * v * v * controls[2].x
                    + v * v * v * b.x,
                u * u * u * a.y
                    + 3. * u * u * v * controls[1].y
                    + 3. * u * v * v * controls[2].y
                    + v * v * v * b.y,
            );
            bridge_segments.push(Rect::from_two_pos(prev, next).expand(3.));
            prev = next;
        }
        let badge = Rect::from_center_size(pos2(rect.center().x, crown + 16.), vec2(154., 28.));
        p.rect_filled(badge, 3., t::SELECTED);
        p.text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            format!(
                "PAIR {:02} → {:02}",
                endpoints[0].1.node, endpoints[1].1.node
            ),
            egui::FontId::monospace(11.),
            t::ORANGE,
        );
        occupied.push(badge);
    }
    occupied.extend(bridge_segments);
    for (side, j) in endpoints {
        p.circle_filled(j.point, 11., t::STAGE_UI);
        p.circle_stroke(j.point, 11., egui::Stroke::new(2., t::ORANGE));
        p.circle_filled(j.point, 3.5, t::TEXT);
        for (dx, dy) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)] {
            let corner = j.point + vec2(14. * dx, 14. * dy);
            p.line_segment(
                [corner, corner - vec2(6. * dx, 0.)],
                egui::Stroke::new(0.7, t::ORANGE),
            );
            p.line_segment(
                [corner, corner - vec2(0., 6. * dy)],
                egui::Stroke::new(0.7, t::ORANGE),
            );
        }
        evidence.selected_points.push(j.point);
        let name = &model.assets[side].nodes[j.node].name;
        let short_caption = rect.height() < 180.;
        let caption_font = if short_caption { 11. } else { 15. };
        let mut display = name.chars().take(80).collect::<String>();
        let mut galley = p.layout_no_wrap(display.clone(), t::font(caption_font, 500), t::TEXT);
        if galley.size().x > 160. || display != *name {
            while !display.is_empty()
                && p.layout_no_wrap(format!("{display}…"), t::font(caption_font, 500), t::TEXT)
                    .size()
                    .x
                    > 160.
            {
                display.pop();
            }
            galley = p.layout_no_wrap(format!("{display}…"), t::font(caption_font, 500), t::TEXT);
        }
        let label_width = if rect.width() < 850. {
            (galley.size().x + 64.).clamp(132., 184.)
        } else {
            184.
        };
        let size = vec2(
            label_width,
            if short_caption {
                26.
            } else {
                (galley.size().y + 29.).max(49.)
            },
        );
        let candidates = [
            Rect::from_min_size(j.point + vec2(40., 42.), size),
            Rect::from_min_size(j.point + vec2(40., -size.y - 28.), size),
            Rect::from_min_size(j.point - vec2(size.x + 40., -42.), size),
            Rect::from_min_size(j.point - vec2(size.x + 40., size.y + 28.), size),
        ];
        let label = candidates
            .into_iter()
            .map(|r| {
                Rect::from_min_size(
                    pos2(
                        r.left()
                            .clamp(rect.left() + 12., rect.right() - size.x - 12.),
                        r.top().clamp(rect.top() + 40., rect.bottom() - size.y - 4.),
                    ),
                    size,
                )
            })
            .find(|r| {
                rect.shrink(8.).contains_rect(*r)
                    && !occupied
                        .iter()
                        .chain(evidence.labels.iter())
                        .any(|o| r.intersects(*o))
                    && !evidence
                        .selected_points
                        .iter()
                        .any(|v| r.expand(16.).contains(*v))
            })
            .unwrap_or_else(|| {
                Rect::from_min_size(
                    pos2(
                        rect.left() + side as f32 * rect.width() * 0.5 + 20.,
                        rect.bottom() - size.y - 3.,
                    ),
                    size,
                )
            });
        let anchor = pos2(label.left() + 14., label.top());
        p.add(egui::Shape::line(
            vec![
                j.point + vec2(0., 14.),
                pos2(j.point.x, label.top() - 10.),
                anchor,
            ],
            egui::Stroke::new(1., t::ORANGE),
        ));
        p.rect_filled(label, 3., t::ELEVATED);
        p.line_segment(
            [label.left_top(), label.left_bottom()],
            egui::Stroke::new(3., t::ORANGE),
        );
        if !short_caption {
            p.text(
                label.left_top() + vec2(12., 7.),
                egui::Align2::LEFT_TOP,
                format!(
                    "{} {:02}",
                    if side == 0 { "SOURCE" } else { "TARGET" },
                    j.node
                ),
                egui::FontId::monospace(11.),
                t::SECONDARY,
            );
        }
        p.galley(
            label.left_top() + vec2(12., if short_caption { 6. } else { 24. }),
            galley,
            t::TEXT,
        );
        evidence.labels.push(label);
        ui.interact(
            label,
            ui.scope_id().with(("joint-caption", side)),
            egui::Sense::hover(),
        )
        .on_hover_text(name);
    }
    if let Some(pointer) = ui.input(|i| i.pointer.hover_pos())
        && let Some(hover) = hit(geometry, pointer)
        && selected[hover.side] != Some(hover.node)
    {
        let name = &model.assets[hover.side].nodes[hover.node].name;
        let at = pointer + vec2(14., 18.);
        let galley = p.layout_no_wrap(
            format!("{name}  ·  {}", hover.node),
            t::font(12., 400),
            t::TEXT,
        );
        let hover_rect = Rect::from_min_size(at, galley.size() + vec2(16., 10.));
        if rect.contains_rect(hover_rect)
            && !evidence.labels.iter().any(|r| r.intersects(hover_rect))
        {
            p.rect_filled(hover_rect, 3., t::PANEL);
            p.galley(at + vec2(8., 5.), galley, t::TEXT);
        }
    }
    evidence
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_duplicate_names_keep_node_identity_and_selected_labels_clear() {
        let (mut builder, _) = crate::rig_corpus::humanoid(Default::default(), true);
        for node in [7, 8] {
            builder.json["nodes"][node]["name"] =
                serde_json::json!("Long duplicate upper limb label / articulation / left side");
        }
        let asset = crate::import::from_bytes(&builder.finish(), &crate::import::Limits::default())
            .unwrap();
        let rigs = [Rig::analyze(&asset), Rig::analyze(&asset)];
        let links: Vec<_> = rigs[0]
            .joints
            .iter()
            .map(|j| Link {
                source: j.node,
                target: Some(j.node),
                status: Status::Manual,
            })
            .collect();
        let ctx = egui::Context::default();
        super::super::theme::apply(&ctx);
        for width in [720., 1024.] {
            let rect = Rect::from_min_size(pos2(20., 20.), vec2(width, 330.));
            let geometry = project(rect, [&rigs[0], &rigs[1]], &State::default());
            let expected = geometry.joints[0]
                .iter()
                .find(|j| j.node == 7)
                .unwrap()
                .point;
            let mut evidence = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width + 40., 400.))),
                    ..Default::default()
                },
                |ui| {
                    evidence = Some(paint(
                        ui,
                        &geometry,
                        &Model {
                            rigs: [&rigs[0], &rigs[1]],
                            assets: [&asset, &asset],
                            links: &links,
                            selected: Some(7),
                        },
                        &State::default(),
                    ));
                },
            );
            output.textures_delta.clear();
            let evidence = evidence.unwrap();
            assert_eq!(
                evidence.selected_points[0], expected,
                "Selection follows node identity, not duplicate name"
            );
            assert_eq!(evidence.labels.len(), 2);
            for label in evidence.labels {
                assert!(rect.contains_rect(label));
                assert!(
                    evidence
                        .selected_points
                        .iter()
                        .all(|p| !label.expand(14.).contains(*p))
                );
                assert!(
                    evidence
                        .selected_bones
                        .iter()
                        .all(|b| !label.intersects(Rect::from_two_pos(b[0], b[1]).expand(3.)))
                );
            }
        }
    }
    #[test]
    fn overlapping_hits_use_distance_depth_then_stable_node_and_bones_select_parent() {
        let joint = |node, point, depth, parent| Joint {
            node,
            point,
            depth,
            parent,
            helper: false,
        };
        let mut g = Geometry {
            rect: Rect::from_min_size(Pos2::ZERO, vec2(400., 300.)),
            scale: 1.,
            joints: [
                vec![
                    joint(7, pos2(40., 40.), 0., None),
                    joint(4, pos2(40., 40.), 0., None),
                ],
                vec![],
            ],
        };
        assert_eq!(hit(&g, pos2(40., 40.)), Some(Hit { side: 0, node: 4 }));
        g.joints[0][0].depth = -1.;
        assert_eq!(hit(&g, pos2(40., 40.)), Some(Hit { side: 0, node: 7 }));
        g.joints[0] = vec![
            joint(4, pos2(20., 50.), 0., None),
            joint(7, pos2(180., 50.), 0., Some(4)),
        ];
        assert_eq!(hit(&g, pos2(100., 50.)), Some(Hit { side: 0, node: 4 }));
    }
    use crate::{
        import,
        rig_corpus::{Variant, humanoid},
    };
    #[test]
    fn true_scene_scale_restores_descriptor_height_and_preserves_ratio() {
        let (a, _) = humanoid(Variant::default(), true);
        let (b, _) = humanoid(
            Variant {
                proportions: true,
                ..Default::default()
            },
            false,
        );
        let a = import::from_bytes(&a.finish(), &import::Limits::default()).unwrap();
        let b = import::from_bytes(&b.finish(), &import::Limits::default()).unwrap();
        let mut rigs = [Rig::analyze(&a), Rig::analyze(&b)];
        // Controlled scene-unit change, normalized descriptors intentionally unchanged.
        rigs[1] = rigs[0].clone();
        rigs[1].height *= 1.5;
        let g = project(
            Rect::from_min_size(Pos2::ZERO, vec2(1064., 606.)),
            [&rigs[0], &rigs[1]],
            &State {
                true_scale: true,
                ..Default::default()
            },
        );
        let span = |v: &[Joint]| {
            v.iter()
                .map(|j| j.point.y)
                .fold(f32::NEG_INFINITY, f32::max)
                - v.iter().map(|j| j.point.y).fold(f32::INFINITY, f32::min)
        };
        assert!((span(&g.joints[1]) / span(&g.joints[0]) - 1.5).abs() < 1e-5);
        let floor = |v: &[Joint]| {
            v.iter()
                .map(|j| j.point.y)
                .fold(f32::NEG_INFINITY, f32::max)
        };
        assert_eq!(floor(&g.joints[0]), floor(&g.joints[1]));
        let mut rotated = State {
            unlinked: true,
            true_scale: true,
            ..Default::default()
        };
        rotated.orbit(1, vec2(30., 15.));
        let other = project(g.rect, [&rigs[0], &rigs[1]], &rotated);
        assert!(other.scale.is_finite());
        let joint = g.joints[0][7];
        assert_eq!(
            hit(&g, joint.point),
            Some(Hit {
                side: 0,
                node: joint.node
            })
        );
    }
    #[test]
    fn linked_orbit_from_either_side_is_numerically_equal() {
        let mut state = State::default();
        state.orbit(1, vec2(31., -12.));
        assert_eq!(state.cameras[0], state.cameras[1]);
        state.orbit(0, vec2(-7., 19.));
        assert_eq!(state.cameras[0], state.cameras[1]);
        state.unlinked = true;
        state.orbit(1, vec2(5., 0.));
        assert_ne!(state.cameras[0], state.cameras[1]);
    }
    #[test]
    fn independent_fit_extreme_ratios_keeps_original_joint_ids_clickable() {
        let (builder, _) = humanoid(Variant::default(), true);
        let asset = import::from_bytes(&builder.finish(), &import::Limits::default()).unwrap();
        let original = Rig::analyze(&asset);
        for ratio in [1., 0.01, 100., 0.001] {
            let mut target = original.clone();
            target.height *= ratio;
            let rect = Rect::from_min_size(pos2(20., 30.), vec2(900., 500.));
            let state = State::default();
            let g = project(rect, [&original, &target], &state);
            for side in 0..2 {
                let region = Rect::from_min_max(
                    pos2(rect.left() + side as f32 * rect.width() * 0.5, rect.top()),
                    pos2(
                        rect.left() + (side + 1) as f32 * rect.width() * 0.5,
                        rect.bottom(),
                    ),
                );
                assert!(g.joints[side].iter().all(|j| region.contains(j.point)));
                let extent = g.joints[side].iter().fold(Rect::NOTHING, |r, j| {
                    r.union(Rect::from_center_size(j.point, Vec2::ZERO))
                });
                assert!(extent.height() > 300.);
                for node in [5, 7, 9, 13, 18, 22] {
                    if let Some(joint) = g.joints[side].iter().find(|j| j.node == node) {
                        assert_eq!(hit(&g, joint.point), Some(Hit { side, node }));
                    }
                }
            }
            let mut state = State::default();
            state.cameras[0].zoom = 1.6;
            state.cameras[1].zoom = 0.8;
            state.cameras[0].pan = vec2(12., 3.);
            state.cameras[1].pan = vec2(-8., 10.);
            let before = state.cameras;
            state.orbit(1, vec2(20., 8.));
            for (side, camera) in before.iter().enumerate() {
                assert_eq!(state.cameras[side].zoom, camera.zoom);
                assert_eq!(state.cameras[side].pan, camera.pan);
            }
            state.fit_side(1);
            assert_eq!(state.cameras[0].pan, before[0].pan);
            assert_eq!(state.cameras[1].zoom, 1.);
        }
    }
    #[test]
    fn framing_modes_leave_retarget_samples_unchanged_and_handle_degenerate_rigs() {
        let (builder, _) = humanoid(Variant::default(), true);
        let asset = import::from_bytes(&builder.finish(), &import::Limits::default()).unwrap();
        let plan = crate::retarget::compile(&asset, &asset, &Default::default(), &[]).unwrap();
        let before = crate::retarget::sample(&asset, &asset, &asset.clips[0], &plan, 0.5);
        let mut rig = Rig::analyze(&asset);
        for true_scale in [false, true] {
            let state = State {
                true_scale,
                ..Default::default()
            };
            for count in [rig.joints.len(), 1, 0] {
                rig.joints.truncate(count);
                let g = project(
                    Rect::from_min_size(Pos2::ZERO, vec2(800., 400.)),
                    [&rig, &rig],
                    &state,
                );
                assert!(g.joints.iter().flatten().all(|j| j.point.is_finite()));
                assert!(g.scale.is_finite());
            }
        }
        let after = crate::retarget::sample(&asset, &asset, &asset.clips[0], &plan, 0.5);
        assert!(
            before
                .iter()
                .zip(&after)
                .all(|(a, b)| a.matrix() == b.matrix())
        );
    }
}

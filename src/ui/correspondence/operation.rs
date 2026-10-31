//! One transient selection flow for pointer dragging and explicit joint picking.
use super::Hit;
use egui::{PointerButton, Pos2};

#[derive(Clone, Debug, Default)]
pub enum Operation {
    #[default]
    Idle,
    Drag {
        source: usize,
        start: Pos2,
        pointer: Pos2,
        button: PointerButton,
        active: bool,
        target: Option<usize>,
    },
    Selecting(Selection),
}
#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub source: Option<usize>,
    pub target: Option<usize>,
    pub awaiting: Option<usize>,
    pub picker: Option<usize>,
    pub filters: [String; 2],
    pub replace: bool,
    pub focus: bool,
}
impl Operation {
    pub fn begin(&mut self, source: Option<usize>, target: Option<usize>) {
        *self = Self::Selecting(Selection {
            source,
            target,
            awaiting: if source.is_none() {
                Some(0)
            } else if target.is_none() {
                Some(1)
            } else {
                None
            },
            focus: true,
            ..Default::default()
        });
    }
    pub fn choose(&mut self, hit: Hit) {
        if let Self::Selecting(selection) = self {
            if hit.side == 0 {
                selection.source = Some(hit.node);
            } else {
                selection.target = Some(hit.node);
            }
            selection.awaiting = if selection.source.is_none() {
                Some(0)
            } else if selection.target.is_none() {
                Some(1)
            } else {
                None
            };
            selection.picker = None;
            selection.replace = false;
            selection.focus = true;
        }
    }
}

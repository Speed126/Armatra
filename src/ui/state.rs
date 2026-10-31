use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Workspace {
    #[default]
    Animate,
    Retarget,
    Library,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Collection {
    #[default]
    All,
    Models,
    Motions,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub assets_width: f32,
    pub inspector_width: f32,
    pub timeline_height: f32,
    pub mesh: bool,
    pub skeleton: bool,
    pub grid: bool,
    pub axes: bool,
    pub scale: f32,
    pub continuous: bool,
    pub playback_speed: f32,
    pub looping: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            assets_width: 220.,
            inspector_width: 264.,
            timeline_height: 178.,
            mesh: true,
            skeleton: false,
            grid: true,
            axes: true,
            scale: 1.,
            continuous: false,
            playback_speed: 1.,
            looping: true,
        }
    }
}

impl Preferences {
    pub fn load(root: &Path) -> Self {
        std::fs::read(root.join("ui-preferences.json"))
            .ok()
            .filter(|bytes| bytes.len() <= 4096)
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|p| {
                [
                    p.assets_width,
                    p.inspector_width,
                    p.timeline_height,
                    p.scale,
                    p.playback_speed,
                ]
                .iter()
                .all(|v| v.is_finite())
            })
            .map(|mut p| {
                p.assets_width = p.assets_width.clamp(220., 400.);
                p.inspector_width = p.inspector_width.clamp(240., 400.);
                p.timeline_height = p.timeline_height.clamp(130., 240.);
                p.scale = p.scale.clamp(0.85, 1.5);
                p.playback_speed = p.playback_speed.clamp(-3., 3.);
                p
            })
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> anyhow::Result<()> {
        crate::library::atomic_publish(
            &root.join("ui-preferences.json"),
            &serde_json::to_vec(self)?,
        )
    }
}

pub struct UiState {
    pub workspace: Workspace,
    pub collection: Collection,
    pub labs: bool,
    pub help: bool,
    pub settings: bool,
    pub preferences: Preferences,
    pub correspondence: super::correspondence::State,
    pub mapping_advanced: bool,
    pub retarget_assets: bool,
    pub drawer: super::drawers::State,
    pub mapping_revealed: Option<(usize, String)>,
    pub mapping_reveal_frame: Option<u64>,
    pub mapping_reveal_height: f32,
    pub mapping_detail_height: f32,
}

impl UiState {
    pub fn new(preferences: Preferences) -> Self {
        Self {
            workspace: Workspace::Animate,
            collection: Collection::All,
            labs: false,
            help: false,
            settings: false,
            preferences,
            correspondence: super::correspondence::State::default(),
            mapping_advanced: false,
            retarget_assets: false,
            drawer: super::drawers::State::default(),
            mapping_revealed: None,
            mapping_reveal_frame: None,
            mapping_reveal_height: 0.,
            mapping_detail_height: 0.,
        }
    }
}

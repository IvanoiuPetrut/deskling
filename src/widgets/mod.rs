//! The widget types. Each one lives in its own file and implements [`WidgetType`]: what it's
//! called and what it shows. Adding a new widget = a new file + a `WidgetKind` variant.

mod battery;
mod clock;
mod cpu;
mod disk;
mod network;
mod now_playing;
mod ram;

pub use clock::utc_name;

use std::sync::Arc;

use gpui_kit::RenderImage;

use crate::dashboard::{WidgetConfig, WidgetKind};
use crate::stats::SystemStats;

/// What a widget shows, top to bottom: a small label, the big value, then a bar or a graph.
/// A widget with `rows` (one per battery, say) shows those under the label instead, and one
/// with a `player` (Now Playing) puts its cover art beside the text and buttons under it.
pub struct Content {
    pub label: String,
    pub value: String,
    /// 0.0–1.0, drawn as a thin bar under the value.
    pub bar: Option<f32>,
    /// Recent values for the history graph (drawn instead of the bar when the graph is on).
    pub graph: Option<Graph>,
    pub rows: Vec<Row>,
    pub player: Option<Player>,
}

impl Content {
    /// Whether `other` would draw the same (bars within a hair), so there's nothing to animate.
    pub fn looks_like(&self, other: &Content) -> bool {
        let close = |a: f32, b: f32| (a - b).abs() < 0.002;
        let same_bar = match (self.bar, other.bar) {
            (Some(a), Some(b)) => close(a, b),
            (a, b) => a.is_none() && b.is_none(),
        };
        let same_rows = self.rows.len() == other.rows.len()
            && self.rows.iter().zip(&other.rows).all(|(a, b)| a.value == b.value && close(a.fill, b.fill));
        self.label == other.label && self.value == other.value && same_bar && same_rows
    }
}

/// One line of a list widget: a name on the left, a value on the right, a bar below.
pub struct Row {
    pub name: String,
    pub value: String,
    /// 0.0–1.0, the bar's fill; also what the threshold colors look at.
    pub fill: f32,
    /// Drawn faded, e.g. a device that's switched off (its value is old).
    pub dimmed: bool,
}

/// The Now Playing widget's extras. Its label is the artist, the value the title and the bar
/// how far into the track it is.
pub struct Player {
    pub art: Option<Arc<RenderImage>>,
    /// Leave room for the cover even while there's none (yet), so the text doesn't jump.
    pub show_art: bool,
    pub show_controls: bool,
    pub playing: bool,
    pub can_play_pause: bool,
    pub can_previous: bool,
    pub can_next: bool,
}

/// Values oldest first; `max` is the value that reaches the top of the graph.
pub struct Graph {
    pub points: Vec<f32>,
    pub max: f32,
}

pub trait WidgetType {
    /// Shown in the dashboard's list and "Add widget" buttons.
    fn name(&self) -> &'static str;

    /// What to draw right now.
    fn content(&self, config: &WidgetConfig, stats: &SystemStats) -> Content;

    /// Whether the "Show history graph" option makes sense for this widget.
    fn has_graph(&self) -> bool {
        false
    }

    /// The threshold colors warn when the value gets low (batteries) instead of high (CPU).
    fn warns_when_low(&self) -> bool {
        false
    }
}

/// The implementation for a kind. `&'static dyn WidgetType` = "some type that implements
/// WidgetType", picked at runtime; the structs have no fields, so one shared instance is enough.
pub fn widget_type(kind: WidgetKind) -> &'static dyn WidgetType {
    match kind {
        WidgetKind::Clock => &clock::Clock,
        WidgetKind::Cpu => &cpu::Cpu,
        WidgetKind::Ram => &ram::Ram,
        WidgetKind::Disk => &disk::Disk,
        WidgetKind::Network => &network::Network,
        WidgetKind::Battery => &battery::Batteries,
        WidgetKind::NowPlaying => &now_playing::NowPlaying,
    }
}

/// 1 GB as Windows Explorer counts it (1024 x 1024 x 1024 bytes).
pub fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1024. * 1024. * 1024.))
}

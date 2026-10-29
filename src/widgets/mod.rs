//! The widget types. Each one lives in its own file and implements [`WidgetType`]: what it's
//! called and what it shows. Adding a new widget = a new file + a `WidgetKind` variant.

mod clock;
mod cpu;
mod disk;
mod network;
mod ram;

use crate::dashboard::{WidgetConfig, WidgetKind};
use crate::stats::SystemStats;

/// What a widget shows, top to bottom: a small label, the big value, then a bar or a graph.
pub struct Content {
    pub label: String,
    pub value: String,
    /// 0.0–1.0, drawn as a thin bar under the value.
    pub bar: Option<f32>,
    /// Recent values for the history graph (drawn instead of the bar when the graph is on).
    pub graph: Option<Graph>,
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
    }
}

/// 1 GB as Windows Explorer counts it (1024 x 1024 x 1024 bytes).
pub fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1024. * 1024. * 1024.))
}

use super::{Content, Graph, WidgetType, gigabytes};
use crate::dashboard::WidgetConfig;
use crate::stats::SystemStats;

pub struct Ram;

impl WidgetType for Ram {
    fn name(&self) -> &'static str {
        "RAM"
    }

    fn content(&self, _: &WidgetConfig, stats: &SystemStats) -> Content {
        let used = stats.ram_used as f32 / stats.ram_total.max(1) as f32;
        Content {
            label: format!("RAM · {} / {}", gigabytes(stats.ram_used), gigabytes(stats.ram_total)),
            value: format!("{:.0}%", used * 100.),
            bar: Some(used),
            graph: Some(Graph { points: stats.ram_history.to_vec(), max: 100. }),
            rows: Vec::new(),
            player: None,
        }
    }

    fn has_graph(&self) -> bool {
        true
    }
}

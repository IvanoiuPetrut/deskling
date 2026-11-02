use super::{Content, Graph, WidgetType};
use crate::dashboard::WidgetConfig;
use crate::stats::SystemStats;

pub struct Cpu;

impl WidgetType for Cpu {
    fn name(&self) -> &'static str {
        "CPU"
    }

    fn content(&self, config: &WidgetConfig, stats: &SystemStats) -> Content {
        let label = match (config.show_temperature, stats.cpu_temperature) {
            (false, _) => "CPU".to_string(),
            (true, Some(celsius)) => format!("CPU · {celsius:.0} °C"),
            (true, None) => "CPU · temp n/a".to_string(),
        };
        Content {
            label,
            value: format!("{:.0}%", stats.cpu_usage),
            bar: Some(stats.cpu_usage / 100.),
            graph: Some(Graph { points: stats.cpu_history.to_vec(), max: 100. }),
            rows: Vec::new(),
            player: None,
        }
    }

    fn has_graph(&self) -> bool {
        true
    }
}

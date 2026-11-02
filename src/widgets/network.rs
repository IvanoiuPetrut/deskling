use super::{Content, Graph, WidgetType};
use crate::dashboard::WidgetConfig;
use crate::stats::SystemStats;

pub struct Network;

/// Bytes per second as a short text, e.g. `850 KB/s` or `1.2 MB/s`.
fn speed(bytes_per_second: f32) -> String {
    const KB: f32 = 1024.;
    match bytes_per_second {
        b if b >= KB * KB => format!("{:.1} MB/s", b / (KB * KB)),
        b if b >= KB => format!("{:.0} KB/s", b / KB),
        b => format!("{b:.0} B/s"),
    }
}

impl WidgetType for Network {
    fn name(&self) -> &'static str {
        "Network"
    }

    fn content(&self, _: &WidgetConfig, stats: &SystemStats) -> Content {
        let downloads = stats.net_down_history.to_vec();
        // Scale the graph to the busiest second shown (at least 64 KB/s, so idle isn't all noise).
        let max = downloads.iter().copied().fold(64. * 1024., f32::max);
        Content {
            label: format!("Network · ↑ {}", speed(stats.net_up)),
            value: format!("↓ {}", speed(stats.net_down)),
            bar: None,
            graph: Some(Graph { points: downloads, max }),
            rows: Vec::new(),
            player: None,
        }
    }

    fn has_graph(&self) -> bool {
        true
    }
}

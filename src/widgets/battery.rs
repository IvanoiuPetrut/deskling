use super::{Content, Row, WidgetType};
use crate::dashboard::WidgetConfig;
use crate::stats::SystemStats;

pub struct Batteries;

impl WidgetType for Batteries {
    fn name(&self) -> &'static str {
        "Batteries"
    }

    fn content(&self, config: &WidgetConfig, stats: &SystemStats) -> Content {
        let options = &config.battery;
        let rows: Vec<Row> = stats
            .batteries
            .iter()
            .filter(|battery| !options.hidden.contains(&battery.name))
            .filter(|battery| battery.connected || options.show_disconnected)
            .map(|battery| {
                let name = options.names.get(&battery.name).filter(|name| !name.trim().is_empty());
                let name = name.unwrap_or(&battery.name);
                Row {
                    name: if battery.charging { format!("{name} · charging") } else { name.clone() },
                    value: format!("{}%", battery.level),
                    fill: Some(battery.level as f32 / 100.),
                    dimmed: !battery.connected,
                    icon: None,
                }
            })
            .collect();

        let value = if rows.is_empty() { "–".into() } else { String::new() };
        let label = if rows.is_empty() { "No batteries connected".into() } else { "Batteries".into() };
        Content { label, value, bar: None, graph: None, rows, player: None, icon: None }
    }

    fn warns_when_low(&self) -> bool {
        true
    }
}
